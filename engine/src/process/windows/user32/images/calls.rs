use crate::{
    process::{
        CallError, EngineInstance, HostError,
        calls::call::PendingOwner,
        windows::{
            gdi::{GdiImageDraw32, GdiImageKind32},
            resources::ResourceError32,
        },
    },
    windows::{
        CallFrame32,
        gdi::GdiError32,
        user32::{Point32, User32Api32 as Api, User32Error32 as E, images::ImageKind32},
    },
};

pub(super) fn failure_code(error: HostError) -> Result<u32, HostError> {
    match error {
        HostError::User32(E::Win32(code)) => Ok(code),
        HostError::User32(E::Allocation | E::Capacity) => Ok(8),
        HostError::Gdi(GdiError32::Memory(error)) | HostError::User32(E::Memory(error)) => {
            Err(HostError::Memory(error))
        }
        HostError::Gdi(error) => Ok(error.win32()),
        HostError::File(error) => Ok(error.win32()),
        HostError::Resource(
            ResourceError32::Memory(error)
            | ResourceError32::Pe(crate::loader::ResourceError32::Memory(error)),
        ) => Err(HostError::Memory(error)),
        HostError::Resource(error) => Ok(error.win32()),
        error => Err(error),
    }
}

impl EngineInstance {
    pub(in crate::process) fn try_complete_user32_image_call(
        &mut self,
        owner: PendingOwner,
        token: u32,
        id: u32,
        frame: CallFrame32,
    ) -> Result<bool, HostError> {
        let Some(api) = Api::from_id(id).filter(|api| api.image() || *api == Api::SetCursor) else {
            return Ok(false);
        };
        self.validate_windows_provider_owner(owner)?;
        let pending = self.checked_pending_call(owner, token)?;
        if pending.frame != frame
            || frame.convention() != api.convention()
            || frame.stack_words() != api.stack_words()
        {
            return Err(HostError::Call(CallError::InvalidRequest));
        }
        if self.user32.is_none() {
            return Err(HostError::InvalidRequest);
        }
        if let Some(reply) = self.user32.as_ref().unwrap().images.reply.as_ref()
            && (reply.owner != owner || reply.token != token || reply.frame != frame)
        {
            return Err(HostError::Call(CallError::Busy));
        }
        self.prune_user32_images()?;
        self.check_windows_thread_write()?;
        Self::prepare_completed_call(frame.complete(0))?;
        self.memory()?
            .ensure_transaction_versions(2)
            .map_err(HostError::Memory)?;
        let result = match api {
            Api::CopyImage => self.copy_user32_image(frame.arguments()).map(Some),
            Api::DestroyIcon | Api::DestroyCursor => self
                .destroy_user32_image(api, frame.arguments()[0])
                .map(Some),
            Api::DrawIcon | Api::DrawIconEx => {
                self.draw_user32_image(api, frame.arguments()).map(Some)
            }
            Api::SetCursor => self.set_user32_image_cursor(frame.arguments()[0]).map(Some),
            _ => self.load_user32_image(owner, token, api, frame),
        };
        let value = match result {
            Ok(None) => return Ok(true),
            Ok(Some(value)) => value,
            Err(error) => {
                let code = failure_code(error)?;
                let thread = self.current_windows_thread()?.with_last_error(code);
                self.publish_windows_thread(thread)?;
                0
            }
        };
        self.publish_prepared_call(Self::prepare_completed_call(frame.complete(value))?);
        Ok(true)
    }
    fn prune_user32_images(&mut self) -> Result<(), HostError> {
        let memory = self.memory.as_ref().ok_or(HostError::Closed)?;
        let Some(gdi) = self.gdi.as_ref() else {
            return Ok(());
        };
        self.user32.as_mut().unwrap().images.prune(|record| {
            if record.kind == ImageKind32::Bitmap {
                !matches!(
                    gdi.bitmap_info(memory, record.handle),
                    Err(GdiError32::InvalidHandle)
                )
            } else {
                !matches!(
                    gdi.image_info(record.handle),
                    Err(GdiError32::InvalidHandle)
                )
            }
        });
        Ok(())
    }

    pub(super) fn check_user32_image(
        &self,
        handle: u32,
        kind: ImageKind32,
    ) -> Result<(), HostError> {
        let runtime = self.gdi.as_ref().ok_or(HostError::User32(E::Win32(50)))?;
        if kind == ImageKind32::Bitmap {
            runtime
                .bitmap_info(self.memory()?, handle)
                .map_err(HostError::Gdi)?;
        } else {
            let info = runtime.image_info(handle).map_err(HostError::Gdi)?;
            if (info.kind == GdiImageKind32::Icon) != (kind == ImageKind32::Icon) {
                return Err(HostError::User32(E::Win32(6)));
            }
        }
        Ok(())
    }
    fn set_user32_image_cursor(&mut self, handle: u32) -> Result<u32, HostError> {
        if handle != 0 {
            self.check_user32_image(handle, ImageKind32::Cursor)?;
        }
        self.user32
            .as_mut()
            .unwrap()
            .replace_image_cursor(handle)
            .map_err(HostError::User32)
    }
    fn destroy_user32_image(&mut self, api: Api, handle: u32) -> Result<u32, HostError> {
        let runtime = self.gdi.as_ref().ok_or(HostError::User32(E::Win32(50)))?;
        let info = runtime.image_info(handle).map_err(HostError::Gdi)?;
        let kind = if info.kind == GdiImageKind32::Icon {
            ImageKind32::Icon
        } else {
            ImageKind32::Cursor
        };
        if api == Api::DestroyCursor && kind != ImageKind32::Cursor {
            return Err(HostError::User32(E::Win32(6)));
        }
        if self
            .user32
            .as_ref()
            .unwrap()
            .images
            .record(handle, kind)
            .is_ok_and(|record| record.shared)
        {
            return Ok(1);
        }
        if self.user32.as_ref().unwrap().input.cursor_handle == handle {
            return Err(HostError::User32(E::Win32(87)));
        }
        let prepared = runtime
            .prepare_delete_image(self.memory()?, handle)
            .map_err(HostError::Gdi)?;
        runtime
            .check_delete_image(self.memory()?, &prepared)
            .map_err(HostError::Gdi)?;
        self.gdi
            .as_mut()
            .unwrap()
            .commit_delete_image(self.memory.as_mut().unwrap(), prepared)
            .map_err(HostError::Gdi)?;
        self.user32.as_mut().unwrap().images.retire(handle);
        Ok(1)
    }
    fn draw_user32_image(&mut self, api: Api, args: &[u32]) -> Result<u32, HostError> {
        let runtime = self.gdi.as_ref().ok_or(HostError::User32(E::Win32(50)))?;
        let info = runtime.image_info(args[3]).map_err(HostError::Gdi)?;
        let (width, height, flags, background) = if api == Api::DrawIcon {
            (32, 32, 0xf, 0)
        } else {
            let flags = args[8];
            if flags & !0x1f != 0 || flags & 3 == 0 || (args[4] as i32) < 0 || (args[5] as i32) < 0
            {
                return Err(HostError::User32(E::Win32(87)));
            }
            let defaults = if flags & 8 != 0 {
                Self::image_default_extent()
            } else {
                crate::windows::user32::images::ImageExtent32 {
                    width: info.width,
                    height: info.height,
                }
            };
            (
                if args[4] == 0 {
                    defaults.width
                } else {
                    args[4]
                },
                if args[5] == 0 {
                    defaults.height
                } else {
                    args[5]
                },
                flags,
                args[7],
            )
        };
        let draw = GdiImageDraw32 {
            handle: args[3],
            position: Point32 {
                x: args[1] as i32,
                y: args[2] as i32,
            },
            width,
            height,
            flags,
            background,
        };
        let mut canvas = runtime
            .begin_canvas(self.memory()?, args[0])
            .map_err(HostError::Gdi)?;
        canvas.image(runtime, draw).map_err(HostError::Gdi)?;
        self.gdi
            .as_mut()
            .unwrap()
            .publish_canvas(self.memory.as_mut().unwrap(), canvas)
            .map_err(HostError::Gdi)?;
        Ok(1)
    }
}
