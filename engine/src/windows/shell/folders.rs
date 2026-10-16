use super::ShellError32 as E;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellFolderProfile32 {
    pub user_directory: Vec<u16>,
    pub default_user_directory: Vec<u16>,
    pub common_directory: Vec<u16>,
    pub program_files_directory: Vec<u16>,
}
impl Default for ShellFolderProfile32 {
    fn default() -> Self {
        Self {
            user_directory: "C:\\Users\\Player".encode_utf16().collect(),
            default_user_directory: "C:\\Users\\Default".encode_utf16().collect(),
            common_directory: "C:\\ProgramData".encode_utf16().collect(),
            program_files_directory: "C:\\Program Files".encode_utf16().collect(),
        }
    }
}
impl ShellFolderProfile32 {
    pub fn validate(&self) -> Result<(), E> {
        for path in [
            &self.user_directory,
            &self.default_user_directory,
            &self.common_directory,
            &self.program_files_directory,
        ] {
            if path.len() < 3
                || path.len() >= 260
                || path.contains(&0)
                || path[1] != 58
                || path[2] != 92
                || !u8::try_from(path[0]).is_ok_and(|c| c.is_ascii_alphabetic())
            {
                return Err(E::InvalidParameter);
            }
        }
        Ok(())
    }
    pub(crate) fn path(
        &self,
        csidl: u32,
        default_user: bool,
        windows: &[u16],
        system: &[u16],
    ) -> Result<Vec<u16>, E> {
        let user = if default_user {
            &self.default_user_directory
        } else {
            &self.user_directory
        };
        let (base, leaf) = match csidl {
            0 | 0x10 => (user.as_slice(), "Desktop"),
            0x2 => (
                user.as_slice(),
                "AppData\\Roaming\\Microsoft\\Windows\\Start Menu\\Programs",
            ),
            0x5 => (user.as_slice(), "Documents"),
            0x6 => (user.as_slice(), "Favorites"),
            0x7 => (
                user.as_slice(),
                "AppData\\Roaming\\Microsoft\\Windows\\Start Menu\\Programs\\Startup",
            ),
            0x8 => (
                user.as_slice(),
                "AppData\\Roaming\\Microsoft\\Windows\\Recent",
            ),
            0x9 => (
                user.as_slice(),
                "AppData\\Roaming\\Microsoft\\Windows\\SendTo",
            ),
            0xb => (
                user.as_slice(),
                "AppData\\Roaming\\Microsoft\\Windows\\Start Menu",
            ),
            0xd => (user.as_slice(), "Music"),
            0xe => (user.as_slice(), "Videos"),
            0x14 => (windows, "Fonts"),
            0x15 => (
                user.as_slice(),
                "AppData\\Roaming\\Microsoft\\Windows\\Templates",
            ),
            0x16 => (
                self.common_directory.as_slice(),
                "Microsoft\\Windows\\Start Menu",
            ),
            0x17 => (
                self.common_directory.as_slice(),
                "Microsoft\\Windows\\Start Menu\\Programs",
            ),
            0x18 => (
                self.common_directory.as_slice(),
                "Microsoft\\Windows\\Start Menu\\Programs\\Startup",
            ),
            0x19 => (self.common_directory.as_slice(), "Desktop"),
            0x1a => (user.as_slice(), "AppData\\Roaming"),
            0x1c => (user.as_slice(), "AppData\\Local"),
            0x20 => (
                user.as_slice(),
                "AppData\\Local\\Microsoft\\Windows\\INetCache",
            ),
            0x21 => (
                user.as_slice(),
                "AppData\\Local\\Microsoft\\Windows\\INetCookies",
            ),
            0x22 => (
                user.as_slice(),
                "AppData\\Local\\Microsoft\\Windows\\History",
            ),
            0x23 => (self.common_directory.as_slice(), ""),
            0x24 => (windows, ""),
            0x25 | 0x29 => (system, ""),
            0x26 | 0x2a => (self.program_files_directory.as_slice(), ""),
            0x27 => (user.as_slice(), "Pictures"),
            0x28 => (user.as_slice(), ""),
            0x2b | 0x2c => (self.program_files_directory.as_slice(), "Common Files"),
            0x2d => (
                self.common_directory.as_slice(),
                "Microsoft\\Windows\\Templates",
            ),
            0x2e => (self.common_directory.as_slice(), "Documents"),
            0x2f => (
                self.common_directory.as_slice(),
                "Microsoft\\Windows\\Start Menu\\Programs\\Administrative Tools",
            ),
            0x30 => (
                user.as_slice(),
                "AppData\\Roaming\\Microsoft\\Windows\\Start Menu\\Programs\\Administrative Tools",
            ),
            0x35 => (self.common_directory.as_slice(), "Music"),
            0x36 => (self.common_directory.as_slice(), "Pictures"),
            0x37 => (self.common_directory.as_slice(), "Videos"),
            _ => return Err(E::InvalidParameter),
        };
        let mut out = base.to_vec();
        if !leaf.is_empty() {
            if out.last() != Some(&92) {
                out.push(92);
            }
            out.extend(leaf.encode_utf16());
        }
        if out.len() >= 260 {
            return Err(E::Capacity);
        }
        Ok(out)
    }
}
