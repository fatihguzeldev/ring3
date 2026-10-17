use crate::{
    memory::{AddressSpace, GuestAddress},
    process::windows::user32::{
        User32CallContext32, User32Context32, User32Outcome32, callback::Continuation32, memory,
    },
    windows::{
        user32::{Point32, User32Api32 as Api, User32Error32 as E},
        window::WindowHandle32,
    },
};

impl User32Context32 {
    pub(in crate::process::windows::user32) fn def_dialog_proc(
        &mut self,
        args: &[u32],
        wide: bool,
        ram: &mut AddressSpace,
        context: User32CallContext32,
    ) -> Result<User32Outcome32, E> {
        let window = self.owned(args[0], context.thread_id)?;
        let Ok(mut dialog) = self.state.dialog(window) else {
            return self.def_window_proc_encoded(args, wide, ram, context);
        };
        if args[1] == 0x30 {
            dialog.font = args[2];
            self.state.set_dialog(window, dialog)?;
        }
        let procedure = self.state.extra(window, 4)?;
        if procedure == 0 {
            return self.default_dialog_message(args, wide, ram, context);
        }
        self.state.set_extra(window, 0, 0)?;
        self.start(Continuation32::ResourceProcedure {
            window,
            procedure: GuestAddress(procedure),
            arguments: [args[0], args[1], args[2], args[3]],
            wide,
        })
    }

    pub(in crate::process::windows::user32) fn complete_dialog_procedure(
        &mut self,
        window: WindowHandle32,
        arguments: [u32; 4],
        wide: bool,
        result: u32,
        _ram: &mut AddressSpace,
    ) -> Result<User32Outcome32, E> {
        let value = if result != 0 {
            if matches!(
                arguments[1],
                0x2e | 0x2f | 0x37 | 0x39 | 0x110 | 0x132..=0x138
            ) {
                result
            } else if self.windows().get(window).is_ok() {
                self.state.extra(window, 0)?
            } else {
                0
            }
        } else if self.windows().get(window).is_ok() {
            if matches!(arguments[1], 0x30 | 0x31 | 0x110) {
                if arguments[1] == 0x31 {
                    self.state.dialog(window)?.font
                } else {
                    0
                }
            } else {
                let defaults = self.resource_dialogs.defaults.ok_or(E::InvalidRequest)?;
                self.pending.last_mut().unwrap().continuation = Continuation32::Call {
                    procedure: defaults[usize::from(wide)],
                    arguments,
                };
                return self.next_outcome();
            }
        } else {
            0
        };
        self.pending.pop();
        Ok(User32Outcome32::Return(value))
    }

    fn default_dialog_message(
        &mut self,
        args: &[u32],
        wide: bool,
        ram: &mut AddressSpace,
        context: User32CallContext32,
    ) -> Result<User32Outcome32, E> {
        let window = self.owned(args[0], context.thread_id)?;
        match args[1] {
            0x0f | 0x14 => {
                let defaults = self.resource_dialogs.defaults.ok_or(E::InvalidRequest)?;
                self.start(Continuation32::Call {
                    procedure: defaults[usize::from(wide)],
                    arguments: [args[0], args[1], args[2], args[3]],
                })
            }
            0x31 => Ok(User32Outcome32::Return(self.state.dialog(window)?.font)),
            0x30 | 0x110 => Ok(User32Outcome32::Return(0)),
            _ => self.def_window_proc_encoded(args, wide, ram, context),
        }
    }

    pub(in crate::process::windows::user32) fn resource_dialog_query(
        &mut self,
        api: Api,
        args: &[u32],
        ram: &mut AddressSpace,
        context: User32CallContext32,
        system_base: Point32,
    ) -> Result<User32Outcome32, E> {
        let value = match api {
            Api::GetDialogBaseUnits => {
                (system_base.x as u32 & 0xffff) | ((system_base.y as u32) << 16)
            }
            Api::GetDlgItem => self
                .state
                .dialog_item(WindowHandle32::try_from(args[0])?, args[1])?
                .map_or(0, WindowHandle32::get),
            Api::GetDlgCtrlID => self.state.record(WindowHandle32::try_from(args[0])?)?.menu,
            Api::GetWindowContextHelpId => self
                .state
                .window_context_help(WindowHandle32::try_from(args[0])?)?,
            Api::SetWindowContextHelpId => {
                let window = self.owned(args[0], context.thread_id)?;
                self.state.set_window_context_help(window, args[1])?;
                1
            }
            Api::EndDialog => {
                let window = self.owned(args[0], context.thread_id)?;
                let mut dialog = self.state.dialog(window)?;
                if !dialog.modal {
                    return Err(E::Win32(1420));
                }
                dialog.ended = Some(args[1]);
                self.state.set_dialog(window, dialog)?;
                1
            }
            Api::MapDialogRect => {
                let window = WindowHandle32::try_from(args[0])?;
                let base = self.state.dialog(window)?.base;
                let rect = memory::rect(ram, args[1])?;
                memory::write_rect(
                    ram,
                    args[1],
                    crate::windows::user32::Rect32 {
                        left: super::preparation::mul_div(rect.left, base.x, 4)?,
                        right: super::preparation::mul_div(rect.right, base.x, 4)?,
                        top: super::preparation::mul_div(rect.top, base.y, 8)?,
                        bottom: super::preparation::mul_div(rect.bottom, base.y, 8)?,
                    },
                )?;
                1
            }
            _ => return Err(E::InvalidRequest),
        };
        Ok(User32Outcome32::Return(value))
    }
}
