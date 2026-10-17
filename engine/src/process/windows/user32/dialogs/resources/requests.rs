use super::preparation::{Prepared32, logical_font};
use crate::{
    memory::GuestAddress,
    process::{EngineInstance, HostError, windows::user32::User32CallContext32},
    windows::user32::{User32Api32 as Api, User32Error32 as E, dialogs::DialogTemplate32},
};

pub(in crate::process::windows::user32) struct Creation32 {
    pub(super) prepared: Prepared32,
    pub(super) args: [u32; 5],
    pub(super) wide: bool,
    pub(super) modal: bool,
    pub(in crate::process::windows::user32) font: u32,
    pub(super) context: User32CallContext32,
    pub(super) foreign_owner: bool,
}

impl Creation32 {
    pub(in crate::process::windows::user32) fn internal_fonts(&self) -> [u32; 2] {
        let mut fonts = self
            .prepared
            .windows
            .iter()
            .filter(|window| window.dialog_base.is_some())
            .map(|window| window.font);
        [fonts.next().unwrap_or(0), fonts.next().unwrap_or(0)]
    }
}

pub(in crate::process::windows::user32) struct InternalDialog32 {
    pub template: DialogTemplate32,
    pub owner: u32,
    pub procedure: GuestAddress,
    pub parameter: u32,
    pub wide: bool,
    pub child: Option<DialogTemplate32>,
    pub instance: u32,
}

impl EngineInstance {
    pub(in crate::process::windows::user32) fn prepare_resource_dialog_creation(
        &mut self,
        api: Api,
        args: &[u32],
        context: User32CallContext32,
    ) -> Result<Creation32, HostError> {
        let args: [u32; 5] = args.try_into().map_err(|_| HostError::InvalidRequest)?;
        self.bind_user32_dialog_class()?;
        let user32 = self.user32.as_ref().ok_or(HostError::InvalidRequest)?;
        let limit = (user32.scratch_bounds.count * 4096).min(1 << 20);
        let item_limit = user32.state.window_limit().saturating_sub(1);
        let template = if api.dialog_indirect() {
            let memory = self.memory.as_ref().ok_or(HostError::Closed)?;
            DialogTemplate32::read(
                |offset, output| {
                    let address = args[1]
                        .checked_add(u32::try_from(offset).map_err(|_| E::Win32(87))?)
                        .ok_or(E::Win32(87))?;
                    memory.read(GuestAddress(address), output)?;
                    Ok(())
                },
                limit,
                item_limit,
            )
            .map_err(HostError::User32)?
        } else {
            let bytes = self.user32_resource_bytes(args[0], 5, args[1], api.wide(), limit)?;
            DialogTemplate32::parse(&bytes, item_limit).map_err(HostError::User32)?
        };
        self.prepare_dialog_template(
            template,
            args,
            api.wide(),
            api.dialog_modal(),
            context,
            false,
        )
    }

    pub(in crate::process::windows::user32) fn prepare_internal_dialog(
        &mut self,
        input: InternalDialog32,
        context: User32CallContext32,
    ) -> Result<Creation32, HostError> {
        self.bind_user32_builtin_classes()?;
        self.bind_user32_dialog_class()?;
        if input.owner != 0 {
            let owner = crate::windows::window::WindowHandle32::try_from(input.owner)
                .map_err(E::from)
                .map_err(HostError::User32)?;
            self.user32
                .as_ref()
                .ok_or(HostError::InvalidRequest)?
                .windows()
                .get(owner)
                .map_err(E::from)
                .map_err(HostError::User32)?;
        }
        let args = [
            if input.instance == 0 {
                context.instance
            } else {
                input.instance
            },
            0,
            input.owner,
            input.procedure.0,
            input.parameter,
        ];
        if input.child.is_some() && input.template.font.is_some() {
            return Err(HostError::User32(E::Win32(50)));
        }
        let mut creation =
            self.prepare_dialog_template(input.template, args, input.wide, true, context, true)?;
        if let Some(template) = input.child {
            let limit = self.user32.as_ref().unwrap().scratch_bounds.count * 4096;
            let gdi = self.gdi.as_ref().ok_or(HostError::InvalidRequest)?;
            let logical = template
                .font
                .as_ref()
                .map(|font| logical_font(font, gdi.display_dpi()))
                .transpose()
                .map_err(HostError::User32)?;
            let fallback = gdi.system_font().map_err(HostError::Gdi)?;
            let base = gdi
                .dialog_base_units(logical.as_ref().unwrap_or(&fallback))
                .map_err(HostError::Gdi)?;
            let child = Prepared32::new(template, base, input.wide, context.ansi, limit)
                .map_err(HostError::User32)?;
            let first = creation.prepared.windows.len();
            creation
                .prepared
                .append_child(child, 0x04a0, limit)
                .map_err(HostError::User32)?;
            let font = logical
                .map(|font| {
                    self.gdi
                        .as_mut()
                        .unwrap()
                        .create_logical_font(font)
                        .map_err(HostError::Gdi)
                })
                .transpose()?
                .unwrap_or(0);
            for window in &mut creation.prepared.windows[first..] {
                window.font = font;
            }
        }
        Ok(creation)
    }

    fn prepare_dialog_template(
        &mut self,
        template: DialogTemplate32,
        args: [u32; 5],
        wide: bool,
        modal: bool,
        context: User32CallContext32,
        foreign_owner: bool,
    ) -> Result<Creation32, HostError> {
        let user32 = self.user32.as_ref().ok_or(HostError::InvalidRequest)?;
        let limit = (user32.scratch_bounds.count * 4096).min(1 << 20);
        let gdi = self.gdi.as_ref().ok_or(HostError::InvalidRequest)?;
        let font = template
            .font
            .as_ref()
            .map(|font| logical_font(font, gdi.display_dpi()))
            .transpose()
            .map_err(HostError::User32)?;
        let logical = if let Some(font) = &font {
            font.clone()
        } else {
            gdi.system_font().map_err(HostError::Gdi)?
        };
        let base = gdi.dialog_base_units(&logical).map_err(HostError::Gdi)?;
        let mut prepared = Prepared32::new(template, base, wide, context.ansi, limit)
            .map_err(HostError::User32)?;
        let font = font
            .map(|font| {
                self.gdi
                    .as_mut()
                    .unwrap()
                    .create_logical_font(font)
                    .map_err(HostError::Gdi)
            })
            .transpose()?
            .unwrap_or(0);
        prepared.set_font(font);
        Ok(Creation32 {
            prepared,
            args,
            wide,
            modal,
            font,
            context,
            foreign_owner,
        })
    }

    pub(in crate::process::windows::user32) fn bind_user32_dialog_class(
        &mut self,
    ) -> Result<(), HostError> {
        if self
            .user32
            .as_ref()
            .is_some_and(|context| context.resource_dialogs.procedures.is_some())
        {
            return Ok(());
        }
        let mut procedures = [GuestAddress(0); 2];
        let mut defaults = [GuestAddress(0); 2];
        for gate in self
            .loaded_windows_modules()
            .flat_map(|module| module.gates())
        {
            match Api::from_id(gate.id) {
                Some(Api::DefDlgProcA) => procedures[0] = gate.entry,
                Some(Api::DefDlgProcW) => procedures[1] = gate.entry,
                Some(Api::DefWindowProcA) => defaults[0] = gate.entry,
                Some(Api::DefWindowProcW) => defaults[1] = gate.entry,
                _ => {}
            }
        }
        if procedures
            .iter()
            .chain(&defaults)
            .any(|procedure| procedure.0 == 0)
        {
            return Err(HostError::User32(E::Win32(1407)));
        }
        let context = self.user32.as_mut().ok_or(HostError::InvalidRequest)?;
        context
            .install_dialog_class(procedures)
            .map_err(HostError::User32)?;
        context.resource_dialogs.defaults = Some(defaults);
        Ok(())
    }
}
