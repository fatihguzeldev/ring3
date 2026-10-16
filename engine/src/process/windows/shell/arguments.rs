use crate::{
    memory::{Access, GuestAddress},
    process::{EngineInstance, HostError, windows::strings::Input32},
    windows::shell::{ShellError32 as E, command_line_arguments},
};
impl EngineInstance {
    pub(super) fn shell_arguments(&mut self, args: &[u32]) -> Result<u32, HostError> {
        if args[1] == 0 {
            return Err(HostError::Shell(E::InvalidParameter));
        }
        self.memory()?
            .check_span(GuestAddress(args[1]), 4, Access::Write)
            .map_err(HostError::Memory)?;
        let command = Input32::read(
            self.memory()?,
            args[0],
            true,
            self.windows_ansi_code_page(),
            32768,
        )
        .map_err(|e| HostError::Shell(E::from(e)))?;
        let executable = if command.units.is_empty() {
            self.virtual_module_file_name_units(0)?
        } else {
            Vec::new()
        };
        let arguments =
            command_line_arguments(&command.units, &executable).map_err(HostError::Shell)?;
        let size = arguments.iter().try_fold(
            arguments
                .len()
                .checked_mul(4)
                .ok_or(HostError::Shell(E::Capacity))?,
            |size, arg| {
                arg.len()
                    .checked_add(1)
                    .and_then(|n| n.checked_mul(2))
                    .and_then(|n| size.checked_add(n))
                    .ok_or(HostError::Shell(E::Capacity))
            },
        )?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(size)
            .map_err(|_| HostError::Shell(E::Allocation))?;
        bytes.resize(size, 0);
        let prepared = self.prepare_windows_local_initialized(
            u32::try_from(size).map_err(|_| HostError::Shell(E::Capacity))?,
        )?;
        let base = prepared.address();
        let mut at = arguments.len() * 4;
        for (index, argument) in arguments.iter().enumerate() {
            let pointer = base
                .checked_add(at as u32)
                .ok_or(HostError::Shell(E::Capacity))?;
            bytes[index * 4..index * 4 + 4].copy_from_slice(&pointer.to_le_bytes());
            for unit in argument.iter().copied().chain([0]) {
                bytes[at..at + 2].copy_from_slice(&unit.to_le_bytes());
                at += 2;
            }
        }
        self.commit_windows_local_initialized(
            prepared,
            &bytes,
            &[(
                GuestAddress(args[1]),
                &(arguments.len() as u32).to_le_bytes(),
            )],
        )
    }
}
