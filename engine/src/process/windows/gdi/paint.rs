use super::{memory, windows::GdiWindow32};
use crate::{
    memory::GuestAddress,
    process::{EngineInstance, HostError, calls::call::PendingOwner},
    windows::{
        CallFrame32,
        gdi::{GdiApi32, GdiError32, Region32},
        user32::Rect32,
        window::WindowHandle32,
    },
};
pub(super) struct PaintLease32 {
    pub window: u32,
    pub thread: u32,
    dc: u32,
    clip: Option<Region32>,
}
impl EngineInstance {
    pub(super) fn complete_gdi_paint(
        &mut self,
        owner: PendingOwner,
        token: u32,
        api: GdiApi32,
        frame: CallFrame32,
    ) -> Result<bool, HostError> {
        self.validate_new_windows_reply(owner, token)?;
        self.reserve_windows_reply()?;
        let thread = self.current_windows_thread_id()?;
        let args = frame.arguments();
        let window = self
            .user32
            .as_ref()
            .ok_or(HostError::InvalidRequest)?
            .gdi_window(args[0], false);
        let projection = match window {
            Ok(window) => window,
            Err(error) => {
                let code = match error {
                    crate::windows::user32::User32Error32::Win32(code) => code,
                    _ => 1400,
                };
                self.publish_user32_result(
                    owner,
                    token,
                    frame,
                    super::super::user32::User32Outcome32::Return(0),
                    Some(code),
                )?;
                return Ok(true);
            }
        };
        if projection.thread != thread
            && matches!(api, GdiApi32::BeginPaint | GdiApi32::UpdateWindow)
        {
            self.publish_user32_result(
                owner,
                token,
                frame,
                super::super::user32::User32Outcome32::Return(0),
                Some(50),
            )?;
            return Ok(true);
        }
        let queue = self.prepare_user32_queue_change_for(projection.thread)?;
        let result = self.execute_gdi_paint(api, args, projection, thread);
        match result {
            Ok(outcome) => {
                self.commit_user32_queue_change(queue);
                self.publish_user32_result(owner, token, frame, outcome, None)?;
            }
            Err(HostError::Gdi(GdiError32::Memory(
                error @ (crate::memory::MemoryError::VersionExhausted
                | crate::memory::MemoryError::Allocation),
            ))) => return Err(HostError::Memory(error)),
            Err(HostError::Gdi(error)) => self.publish_user32_result(
                owner,
                token,
                frame,
                super::super::user32::User32Outcome32::Return(0),
                Some(error.win32()),
            )?,
            Err(HostError::User32(crate::windows::user32::User32Error32::Win32(error))) => self
                .publish_user32_result(
                    owner,
                    token,
                    frame,
                    super::super::user32::User32Outcome32::Return(0),
                    Some(error),
                )?,
            Err(error) => return Err(error),
        }
        Ok(true)
    }
    fn execute_gdi_paint(
        &mut self,
        api: GdiApi32,
        args: &[u32],
        window: GdiWindow32,
        thread: u32,
    ) -> Result<super::super::user32::User32Outcome32, HostError> {
        use super::super::user32::User32Outcome32 as Outcome;
        let handle =
            WindowHandle32::try_from(window.window).map_err(|_| HostError::InvalidRequest)?;
        let (region, erase) = self
            .user32
            .as_ref()
            .unwrap()
            .gdi_update_region(handle)
            .map_err(HostError::User32)?;
        let bounds = Rect32 {
            left: 0,
            top: 0,
            right: window.width as i32,
            bottom: window.height as i32,
        };
        match api {
            GdiApi32::InvalidateRect | GdiApi32::ValidateRect => {
                let rect = if args[1] == 0 {
                    bounds
                } else {
                    memory::rect(self.memory()?, args[1]).map_err(HostError::Gdi)?
                };
                let next = region
                    .combine(
                        &Region32::rectangle(rect),
                        if api == GdiApi32::InvalidateRect {
                            2
                        } else {
                            4
                        },
                    )
                    .and_then(|region| region.combine(&Region32::rectangle(bounds), 1))
                    .map_err(HostError::Gdi)?;
                self.user32
                    .as_mut()
                    .unwrap()
                    .gdi_set_update_region(
                        handle,
                        next,
                        erase || api == GdiApi32::InvalidateRect && args[2] != 0,
                    )
                    .map_err(HostError::User32)?;
                Ok(Outcome::Return(1))
            }
            GdiApi32::GetUpdateRect => {
                if args[2] != 0 && erase {
                    return Err(HostError::Gdi(GdiError32::Unsupported));
                }
                if args[1] != 0 {
                    memory::write_rect(self.memory.as_mut().unwrap(), args[1], region.bounds())
                        .map_err(HostError::Gdi)?;
                }
                Ok(Outcome::Return(u32::from(region.complexity() != 1)))
            }
            GdiApi32::UpdateWindow => self
                .user32
                .as_mut()
                .unwrap()
                .gdi_update_window(handle)
                .map_err(HostError::User32),
            GdiApi32::BeginPaint => {
                self.user32
                    .as_mut()
                    .unwrap()
                    .gdi_check_begin_paint(handle, erase)
                    .map_err(HostError::User32)?;
                let output = super::super::user32::User32Context32::gdi_prepare_paint_output(
                    self.memory.as_mut().ok_or(HostError::Closed)?,
                    args[1],
                )
                .map_err(HostError::User32)?;
                self.memory()?
                    .ensure_transaction_versions(1)
                    .map_err(HostError::Memory)?;
                let runtime = self.gdi.as_mut().ok_or(HostError::InvalidRequest)?;
                if runtime.paints.len() == runtime.config.dcs {
                    return Err(HostError::Gdi(GdiError32::Capacity));
                }
                let dc = runtime.acquire_dc(window, thread).map_err(HostError::Gdi)?;
                let clip = runtime
                    .model
                    .state(dc)
                    .map_err(HostError::Gdi)?
                    .clip
                    .clone();
                let mut bytes = [0u8; 64];
                for (part, value) in bytes[..24].chunks_exact_mut(4).zip([
                    dc,
                    u32::from(erase),
                    region.bounds().left as u32,
                    region.bounds().top as u32,
                    region.bounds().right as u32,
                    region.bounds().bottom as u32,
                ]) {
                    part.copy_from_slice(&value.to_le_bytes());
                }
                self.memory
                    .as_mut()
                    .unwrap()
                    .write(GuestAddress(args[1]), &bytes)
                    .expect("paint output and version preflight");
                runtime
                    .model
                    .set_clip(dc, Some(region))
                    .expect("accepted paint DC");
                runtime.paints.push(PaintLease32 {
                    window: window.window,
                    thread,
                    dc,
                    clip,
                });
                self.user32
                    .as_mut()
                    .unwrap()
                    .gdi_begin_paint(handle, output, dc, erase)
                    .map_err(HostError::User32)
            }
            GdiApi32::EndPaint => {
                let [dc]: [u32; 1] =
                    memory::words(self.memory()?, args[1]).map_err(HostError::Gdi)?;
                let runtime = self.gdi.as_mut().ok_or(HostError::InvalidRequest)?;
                let index = runtime
                    .paints
                    .iter()
                    .rposition(|paint| {
                        paint.window == window.window && paint.thread == thread && paint.dc == dc
                    })
                    .ok_or(HostError::Gdi(GdiError32::InvalidHandle))?;
                runtime.model.dc_kind(dc).map_err(HostError::Gdi)?;
                let paint = runtime.paints.remove(index);
                runtime
                    .model
                    .set_clip(dc, paint.clip)
                    .expect("accepted paint DC");
                runtime
                    .model
                    .release_dc(window.window, dc, thread)
                    .map_err(HostError::Gdi)?;
                Ok(Outcome::Return(1))
            }
            _ => Err(HostError::InvalidRequest),
        }
    }
    pub(super) fn gdi_erase_background(&mut self, args: &[u32]) -> Result<u32, GdiError32> {
        let window = self
            .user32
            .as_ref()
            .ok_or(GdiError32::InvalidConfig)?
            .gdi_window(args[0], false)
            .map_err(|_| GdiError32::InvalidHandle)?;
        if window.background_brush == 0 {
            return Ok(0);
        }
        let runtime = self.gdi.as_mut().ok_or(GdiError32::InvalidConfig)?;
        runtime.fill_color_rect(
            self.memory.as_mut().unwrap(),
            args[2],
            Rect32 {
                left: 0,
                top: 0,
                right: window.width as i32,
                bottom: window.height as i32,
            },
            window.background_brush,
        )?;
        Ok(1)
    }
}
