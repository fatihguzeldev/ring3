use super::{ColorChange32, Stage32};
use crate::{
    process::{
        EngineInstance, HostError,
        windows::user32::{User32Outcome32, memory},
    },
    windows::{
        gdi::Region32,
        user32::{Rect32, User32Error32 as Error},
    },
};

impl EngineInstance {
    pub(in crate::process::windows::user32) fn execute_system_colors(
        &mut self,
        args: &[u32],
        thread: u32,
    ) -> Result<User32Outcome32, HostError> {
        self.user32
            .as_mut()
            .ok_or(HostError::InvalidRequest)?
            .last_error = None;
        if args.len() != 3 || args[0] > 65535 {
            return Err(HostError::User32(Error::Win32(87)));
        }
        let mut colors = self
            .gdi
            .as_ref()
            .ok_or(HostError::InvalidRequest)?
            .system_colors();
        for i in 0..args[0] {
            let index = args[1]
                .checked_add(i * 4)
                .ok_or(HostError::User32(Error::Win32(87)))?;
            let value = args[2]
                .checked_add(i * 4)
                .ok_or(HostError::User32(Error::Win32(87)))?;
            let index = memory::words::<1>(self.memory()?, index).map_err(HostError::User32)?[0];
            let value = memory::words::<1>(self.memory()?, value).map_err(HostError::User32)?[0];
            colors.set(index, value);
        }
        let result = self
            .user32
            .as_mut()
            .unwrap()
            .begin_system_colors(thread)
            .map_err(HostError::User32)?;
        self.gdi.as_mut().unwrap().set_system_colors(colors);
        Ok(result)
    }

    pub(in crate::process) fn advance_system_colors_repaint(
        &mut self,
        token: u64,
    ) -> Result<bool, HostError> {
        let Some(call) = self
            .user32
            .as_ref()
            .and_then(|context| context.system_colors_wait(token))
        else {
            return Ok(false);
        };
        if call.stage != Stage32::Repaint {
            return Ok(false);
        }
        let context = self.user32.as_ref().unwrap();
        let next = context
            .state
            .next_color_window(call.after, call.through, true);
        if let Some(window) = next {
            let record = context
                .windows()
                .get(window)
                .map_err(Error::from)
                .map_err(HostError::User32)?;
            let queue = self.prepare_user32_queue_change_for(record.description.thread_id)?;
            let region = Region32::rectangle(Rect32 {
                left: 0,
                top: 0,
                right: record.description.extent.width as i32,
                bottom: record.description.extent.height as i32,
            });
            let context = self.user32.as_mut().unwrap();
            context
                .state
                .set_update_region(window, region, true)
                .map_err(HostError::User32)?;
            context.publish_system_colors(
                token,
                ColorChange32 {
                    after: window.get(),
                    ..call
                },
            );
            self.commit_user32_queue_change(queue);
        } else {
            self.user32.as_mut().unwrap().publish_system_colors(
                token,
                ColorChange32 {
                    stage: Stage32::Finish,
                    ..call
                },
            );
        }
        Ok(true)
    }
}
