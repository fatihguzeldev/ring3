use super::template;
use crate::{
    memory::{AddressSpace, GuestAddress},
    process::windows::user32::{User32Context32, User32Outcome32, memory},
    windows::{user32::User32Error32 as E, window::WindowHandle32},
};
impl User32Context32 {
    pub(super) fn common_dialog_message(
        &mut self,
        root: WindowHandle32,
        args: &[u32],
        ram: &mut AddressSpace,
    ) -> Result<Option<User32Outcome32>, E> {
        let index = self.file_dialogs.window(root)?;
        let plan = &self.file_dialogs.plans[index];
        let value = match args[1] {
            0x468 => {
                let control = self.file_dialog_control(root, args[2])?;
                let text =
                    memory::string(ram, args[3], plan.request.wide, 32768, plan.request.ansi)?;
                self.state.check_control_changes(1)?;
                self.state.set_text(control, text)?;
                self.state.invalidate_control(control)?;
                0
            }
            0x46a => {
                let extension =
                    memory::string(ram, args[3], plan.request.wide, 32768, plan.request.ansi)?;
                self.file_dialogs.plans[index].request.default_extension = extension;
                0
            }
            0x464..=0x466 => {
                let text = if args[1] == 0x466 {
                    plan.directory.clone()
                } else {
                    let edit = self.file_dialog_control(root, template::NAME)?;
                    let name = self.state.text(edit)?;
                    if args[1] == 0x464 {
                        name.to_vec()
                    } else {
                        crate::files::path::FilePathScope32::from_directory(plan.directory.clone())
                            .normalize(name)
                            .map_err(|error| E::Win32(error.win32()))?
                    }
                };
                let bytes = super::super::super::commdlg::publication::encode(
                    &text,
                    plan.request.wide,
                    plan.request.ansi,
                )
                .map_err(|error| match error {
                    crate::windows::commdlg::ComdlgError32::Memory(error) => E::Memory(error),
                    crate::windows::commdlg::ComdlgError32::Allocation => E::Allocation,
                    _ => E::Win32(87),
                })?;
                let units = bytes.len() / if plan.request.wide { 2 } else { 1 };
                if args[2] as usize >= units && args[3] != 0 {
                    ram.write(GuestAddress(args[3]), &bytes)?;
                }
                u32::try_from(units).map_err(|_| E::Capacity)?
            }
            _ => return Ok(None),
        };
        Ok(Some(User32Outcome32::Return(value)))
    }
}
