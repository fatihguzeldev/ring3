use super::*;
use crate::{
    process::windows::user32::{User32Context32, memory},
    windows::{gdi::Region32, user32::Rect32, window::WindowExtent32},
};

impl EngineInstance {
    pub(super) fn create_static_image(
        &mut self,
        window: WindowHandle32,
        structure: u32,
        wide: bool,
        call: User32CallContext32,
    ) -> Result<u32, HostError> {
        let values: [u32; 12] =
            memory::words(self.memory()?, structure).map_err(HostError::User32)?;
        if values[9] == 0 {
            return Ok(0);
        }
        let data = self
            .user32
            .as_ref()
            .unwrap()
            .state
            .record(window)
            .map_err(HostError::User32)?;
        let kind = static_image_kind(data.style).ok_or(HostError::InvalidRequest)?;
        let name = self.user32_resource_identifier(values[9], wide)?;
        let queue = self.prepare_user32_queue_change_for(call.thread_id)?;
        let handle = self.load_static_control_image(data.instance, name, kind, data.style)?;
        let image = ControlImage32 { kind, handle };
        let bound = self.static_image_extent(image).and_then(|extent| {
            self.user32
                .as_mut()
                .unwrap()
                .bind_static_image(window, Some(image), extent, true)
                .map_err(HostError::User32)
        });
        if let Err(error) = bound {
            self.release_static_control_image(handle, kind)?;
            return Err(error);
        }
        self.commit_user32_queue_change(queue);
        Ok(0)
    }

    pub(super) fn release_static_image_window(
        &mut self,
        window: WindowHandle32,
    ) -> Result<(), HostError> {
        let state = self
            .user32
            .as_ref()
            .unwrap()
            .state
            .control_state(window)
            .map_err(HostError::User32)?;
        if let Some(image) = state.image.filter(|_| state.image_owned) {
            self.retire_static_control_image(image)?;
        }
        let state = self
            .user32
            .as_mut()
            .unwrap()
            .state
            .control(window)
            .map_err(HostError::User32)?;
        state.image = None;
        state.image_owned = false;
        Ok(())
    }
}

impl User32Context32 {
    pub(super) fn bind_static_image(
        &mut self,
        window: WindowHandle32,
        image: Option<ControlImage32>,
        extent: (u32, u32),
        owned: bool,
    ) -> Result<u32, E> {
        let mut data = self.state.record(window)?;
        let mut description = self.windows().get(window)?.description;
        let previous = self
            .state
            .control_state(window)?
            .image
            .map_or(0, |image| image.handle);
        self.state.check_paint_change()?;
        if image.is_some() && data.style & (0x200 | 0x40) == 0 {
            let width = i32::try_from(extent.0).map_err(|_| E::Win32(87))?;
            let height = i32::try_from(extent.1).map_err(|_| E::Win32(87))?;
            let old_width = data
                .outer
                .right
                .checked_sub(data.outer.left)
                .ok_or(E::Win32(87))?;
            let old_height = data
                .outer
                .bottom
                .checked_sub(data.outer.top)
                .ok_or(E::Win32(87))?;
            let border_width = old_width
                .checked_sub(description.extent.width as i32)
                .ok_or(E::Win32(87))?;
            let border_height = old_height
                .checked_sub(description.extent.height as i32)
                .ok_or(E::Win32(87))?;
            let outer_width = width.checked_add(border_width).ok_or(E::Win32(87))?;
            let outer_height = height.checked_add(border_height).ok_or(E::Win32(87))?;
            data.outer.right = data
                .outer
                .left
                .checked_add(outer_width)
                .ok_or(E::Win32(87))?;
            data.outer.bottom = data
                .outer
                .top
                .checked_add(outer_height)
                .ok_or(E::Win32(87))?;
            description.extent = WindowExtent32 {
                width: extent.0,
                height: extent.1,
            };
        }
        let region = Region32::rectangle(Rect32 {
            left: 0,
            top: 0,
            right: description.extent.width as i32,
            bottom: description.extent.height as i32,
        });
        self.state
            .windows_mut()
            .update_description(window, description)?;
        self.state.update(data)?;
        self.state.set_update_region(window, region, true)?;
        let state = self.state.control(window)?;
        state.image = image;
        state.image_owned = image.is_some() && owned;
        Ok(previous)
    }
}
