use super::{User32Context32, User32Outcome32};
use crate::{
    memory::GuestAddress,
    process::{EngineInstance, HostError},
    windows::{
        user32::{Point32, Rect32, User32Api32, User32Error32, WindowClass32, WindowData32},
        window::{WindowDescription32, WindowHandle32},
    },
};

pub(super) const DESKTOP_THREAD: u32 = u32::MAX;

impl EngineInstance {
    pub(super) fn bind_user32_desktop(&mut self) -> Result<(), HostError> {
        if self
            .user32
            .as_ref()
            .is_none_or(|context| context.compatibility.desktop.is_some())
        {
            return Ok(());
        }
        let procedure = self
            .loaded_windows_modules()
            .flat_map(|module| module.gates())
            .find(|gate| gate.id == User32Api32::DefWindowProcW.id())
            .map(|gate| gate.entry)
            .ok_or(HostError::InvalidRequest)?;
        let context = self.user32.as_mut().unwrap();
        let rectangle = context.input.desktop;
        context
            .install_desktop(procedure, rectangle)
            .map_err(HostError::User32)?;
        Ok(())
    }
}

impl User32Context32 {
    pub(super) fn install_desktop(
        &mut self,
        procedure: GuestAddress,
        rectangle: Rect32,
    ) -> Result<WindowHandle32, User32Error32> {
        if let Some(window) = self.compatibility.desktop {
            return Ok(window);
        }
        let extent = rectangle.extent()?;
        let description = WindowDescription32 {
            thread_id: DESKTOP_THREAD,
            procedure,
            extent,
            visible: true,
            enabled: true,
        };
        let handle = self.state.check_create(description)?;
        let atom = self.state.install_system_class(WindowClass32 {
            name: "#32769".encode_utf16().collect(),
            procedure,
            unicode: true,
            background_brush: 2,
            ..Default::default()
        })?;
        self.state.publish_window(
            description,
            WindowData32 {
                handle,
                class_atom: atom,
                instance: 0,
                style: 0x9000_0000,
                ex_style: 0,
                user_data: 0,
                parent: None,
                menu: 0,
                outer: rectangle,
                client_origin: Point32 {
                    x: rectangle.left,
                    y: rectangle.top,
                },
                unicode: true,
                destroying: false,
            },
            Box::default(),
        );
        self.compatibility.desktop = Some(handle);
        Ok(handle)
    }

    pub(super) fn desktop_window(&self) -> Result<User32Outcome32, User32Error32> {
        self.compatibility
            .desktop
            .map(|window| User32Outcome32::Return(window.get()))
            .ok_or(User32Error32::Win32(120))
    }
}

impl User32Context32 {
    pub(super) fn update_desktop_bounds(&mut self, rectangle: Rect32) -> Result<(), User32Error32> {
        let Some(window) = self.compatibility.desktop else {
            return Ok(());
        };
        let mut description = self.windows().get(window)?.description;
        description.extent = rectangle.extent()?;
        let mut data = self.state.record(window)?;
        data.outer = rectangle;
        data.client_origin = Point32 {
            x: rectangle.left,
            y: rectangle.top,
        };
        self.state
            .windows_mut()
            .update_description(window, description)?;
        self.state.update(data)
    }
}
