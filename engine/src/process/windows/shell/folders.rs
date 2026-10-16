use crate::{
    memory::{Access, GuestAddress},
    process::{EngineInstance, HostError},
    windows::shell::{ShellApi32 as Api, ShellError32 as E},
};
impl EngineInstance {
    pub(super) fn shell_folder(&mut self, api: Api, args: &[u32]) -> Result<u32, HostError> {
        let special = matches!(
            api,
            Api::SHGetSpecialFolderPathA | Api::SHGetSpecialFolderPathW
        );
        let wide = api.name().ends_with('W');
        let (csidl, token, kind, pointer, create) = if special {
            (args[2], 0, 0, args[1], args[3] != 0)
        } else {
            (args[1], args[2], args[3], args[4], args[1] & 0x8000 != 0)
        };
        if csidl & !0xd0ff != 0 || kind > 1 || !matches!(token, 0 | u32::MAX) || pointer == 0 {
            return Err(HostError::Shell(E::InvalidParameter));
        }
        let windows;
        let system;
        let (win, sys) = if let Some(files) = self.files.as_ref() {
            (
                files.virtual_windows_directory(),
                files.virtual_system_directory(),
            )
        } else {
            windows = "C:\\Windows".encode_utf16().collect::<Vec<_>>();
            system = "C:\\Windows\\System32".encode_utf16().collect::<Vec<_>>();
            (windows.as_slice(), system.as_slice())
        };
        let path = self
            .shell
            .folders
            .path(csidl & 0xff, token == u32::MAX, win, sys)
            .map_err(HostError::Shell)?;
        let mut bytes = if wide {
            path.iter()
                .flat_map(|u| u.to_le_bytes())
                .collect::<Vec<_>>()
        } else {
            self.windows_ansi_code_page()
                .encode_windows(&path, 0, b'?')
                .map_err(|_| HostError::Shell(E::InvalidParameter))?
                .0
        };
        let width = if wide { 2 } else { 1 };
        if bytes.len() / width >= 260 {
            return Err(HostError::Shell(E::Capacity));
        }
        bytes.resize(bytes.len() + width, 0);
        self.memory()?
            .check_span(GuestAddress(pointer), bytes.len(), Access::Write)
            .map_err(HostError::Memory)?;
        if create {
            self.provider_create_directories(&path)?;
        } else if csidl & 0x4000 == 0 {
            let info = self.provider_path_info(&path)?;
            if info.attributes & 0x10 == 0 {
                return Err(HostError::Shell(E::Win32(3)));
            }
        }
        self.memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(pointer), &bytes)
            .map_err(HostError::Memory)?;
        Ok(u32::from(special))
    }
}
