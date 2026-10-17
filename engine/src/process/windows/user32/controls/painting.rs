use super::super::{User32Context32, User32Outcome32, callback::Continuation32};
use crate::{
    memory::AddressSpace,
    process::{
        EngineInstance, HostError,
        windows::gdi::{GdiRuntime32, controls::ControlVisual32},
    },
    windows::{
        controls::ControlKind32,
        gdi::{GdiError32, Region32},
        user32::User32Error32,
        window::WindowHandle32,
    },
};

#[derive(Clone, Copy)]
pub(in crate::process::windows::user32) struct ControlPaint32 {
    pub window: WindowHandle32,
    pub parent: WindowHandle32,
    pub thread: u32,
    pub dc: u32,
    pub erase: bool,
}

impl EngineInstance {
    pub(super) fn execute_builtin_control_paint(
        &mut self,
        args: &[u32],
    ) -> Result<User32Outcome32, HostError> {
        let thread = self.current_windows_thread_id()?;
        let queue = self.prepare_user32_queue_change_for(thread)?;
        let result = self
            .user32
            .as_mut()
            .ok_or(HostError::InvalidRequest)?
            .begin_control_paint(
                self.gdi.as_mut().ok_or(HostError::InvalidRequest)?,
                self.memory.as_mut().ok_or(HostError::Closed)?,
                args,
                thread,
            )?;
        self.commit_user32_queue_change(queue);
        Ok(result)
    }
}

impl User32Context32 {
    fn control_visual(&self, window: WindowHandle32) -> Result<ControlVisual32<'_>, User32Error32> {
        let data = self.state.record(window)?;
        let description = self.windows().get(window)?.description;
        if data.destroying {
            return Err(User32Error32::Win32(1400));
        }
        Ok(ControlVisual32 {
            kind: self
                .state
                .standard_kind(data.class_atom)
                .ok_or(User32Error32::Win32(1407))?,
            style: data.style,
            enabled: description.enabled,
            focused: self.windows().focus() == Some(window),
            text: self.state.text(window)?,
            control: self.state.control_state(window)?,
        })
    }
    fn begin_control_paint(
        &mut self,
        gdi: &mut GdiRuntime32,
        memory: &mut AddressSpace,
        args: &[u32],
        thread: u32,
    ) -> Result<User32Outcome32, HostError> {
        let window = self.owned(args[0], thread).map_err(HostError::User32)?;
        let data = self.state.record(window).map_err(HostError::User32)?;
        let visual = self.control_visual(window).map_err(HostError::User32)?;
        gdi.check_control_visual(&visual).map_err(HostError::Gdi)?;
        let font = visual.control.font;
        let kind = visual.kind;
        let erase = args[1] == 0x14;
        let (region, _) = self
            .state
            .update_region(window)
            .map_err(HostError::User32)?;
        if !erase && region.complexity() == 1 {
            return Ok(User32Outcome32::Return(0));
        }
        let parent = match data.parent {
            Some(parent) => {
                let record = self
                    .windows()
                    .get(parent)
                    .map_err(|error| HostError::User32(error.into()))?;
                (record.description.thread_id == thread).then_some(parent)
            }
            None => None,
        };
        self.state.check_paint_change().map_err(HostError::User32)?;
        if parent.is_some() {
            self.prepare_thread_slots().map_err(HostError::User32)?;
            self.check_continuation().map_err(HostError::User32)?;
        }
        let projection = self
            .gdi_window(window.get(), false)
            .map_err(HostError::User32)?;
        let dc = if erase {
            gdi.check_control_erase_dc(args[2])
                .map_err(HostError::Gdi)?;
            args[2]
        } else {
            gdi.begin_control_dc(projection, region, font, kind)
                .map_err(HostError::Gdi)?
        };
        if let Some(parent) = parent {
            if !erase {
                self.state
                    .set_update_region(window, Region32::default(), false)
                    .expect("control paint change was preflighted");
            }
            self.runtime_input.thread_id = thread;
            return self
                .start(Continuation32::ControlPaint(ControlPaint32 {
                    window,
                    parent,
                    thread,
                    dc,
                    erase,
                }))
                .map_err(HostError::User32);
        }
        let visual = self.control_visual(window).map_err(HostError::User32)?;
        let result = if erase {
            gdi.erase_control(
                memory,
                dc,
                visual.kind,
                crate::windows::user32::Rect32 {
                    left: 0,
                    top: 0,
                    right: projection.width as i32,
                    bottom: projection.height as i32,
                },
                0,
            )
        } else {
            gdi.render_control(memory, dc, &visual, 0)
        };
        if !erase {
            gdi.release_control_dc(dc, window.get(), thread)
                .expect("owned control DC survived local raster preparation");
        }
        result.map_err(HostError::Gdi)?;
        if !erase {
            self.state
                .set_update_region(window, Region32::default(), false)
                .expect("control paint change was preflighted");
        }
        Ok(User32Outcome32::Return(u32::from(erase)))
    }
    pub(in crate::process) fn complete_with_gdi(
        &mut self,
        token: u64,
        result: u32,
        memory: &mut AddressSpace,
        gdi: Option<&mut GdiRuntime32>,
    ) -> Result<User32Outcome32, HostError> {
        if let Some(paint) = self
            .pending
            .last()
            .filter(|pending| pending.token == token)
            .and_then(|pending| match pending.continuation {
                Continuation32::QueryPaint(paint) => Some(paint),
                _ => None,
            })
        {
            return self.complete_query_paint(
                paint,
                result,
                memory,
                gdi.ok_or(HostError::Gdi(GdiError32::InvalidRequest))?,
            );
        }
        let paint = self
            .pending
            .last()
            .filter(|pending| pending.token == token)
            .and_then(|pending| match pending.continuation {
                Continuation32::ControlPaint(paint) => Some(paint),
                _ => None,
            });
        let Some(paint) = paint else {
            let outcome = self
                .complete(token, result, memory)
                .map_err(HostError::User32)?;
            if let Some(gdi) = gdi {
                self.retire_resource_dialog_fonts(gdi)?;
            }
            return Ok(outcome);
        };
        let gdi = gdi.ok_or(HostError::Gdi(GdiError32::InvalidRequest))?;
        if self.windows().get(paint.window).is_err() {
            if !paint.erase
                && gdi
                    .check_control_dc(paint.dc, paint.window.get(), paint.thread)
                    .is_ok()
            {
                gdi.release_control_dc(paint.dc, paint.window.get(), paint.thread)
                    .expect("checked control DC");
            }
            self.pending.pop();
            return Ok(User32Outcome32::Return(0));
        }
        let projection = self
            .gdi_window(paint.window.get(), false)
            .map_err(HostError::User32)?;
        let visual = self
            .control_visual(paint.window)
            .map_err(HostError::User32)?;
        if paint.erase {
            gdi.erase_control(
                memory,
                paint.dc,
                visual.kind,
                crate::windows::user32::Rect32 {
                    left: 0,
                    top: 0,
                    right: projection.width as i32,
                    bottom: projection.height as i32,
                },
                result,
            )
            .map_err(HostError::Gdi)?;
        } else {
            gdi.check_control_dc(paint.dc, paint.window.get(), paint.thread)
                .map_err(HostError::Gdi)?;
            gdi.render_control(memory, paint.dc, &visual, result)
                .map_err(HostError::Gdi)?;
            gdi.release_control_dc(paint.dc, paint.window.get(), paint.thread)
                .expect("owned control DC survived raster publication");
        }
        self.pending.pop();
        Ok(User32Outcome32::Return(u32::from(paint.erase)))
    }
    pub(in crate::process::windows::user32) fn control_paint_callback(
        &self,
        paint: ControlPaint32,
    ) -> Result<(crate::memory::GuestAddress, [u32; 4]), User32Error32> {
        let visual = self.control_visual(paint.window)?;
        let message = match visual.kind {
            ControlKind32::Button => 0x135,
            ControlKind32::Edit if !visual.enabled || visual.style & 0x800 != 0 => 0x138,
            ControlKind32::Edit => 0x133,
            ControlKind32::Static => 0x138,
            ControlKind32::ListBox => 0x134,
            ControlKind32::ComboBox if visual.style & 3 == 3 => 0x138,
            ControlKind32::ComboBox => 0x133,
            ControlKind32::ScrollBar => 0x137,
        };
        Ok((
            self.windows().get(paint.parent)?.description.procedure,
            [paint.parent.get(), message, paint.dc, paint.window.get()],
        ))
    }
}
