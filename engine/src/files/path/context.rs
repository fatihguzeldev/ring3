use crate::files::{FileError32, Vfs32};

#[derive(Clone)]
pub(crate) struct FilePathScope32 {
    cwd: Vec<u16>,
    drives: [Option<Vec<u16>>; 26],
}

impl FilePathScope32 {
    pub(crate) fn from_directory(cwd: Vec<u16>) -> Self {
        Self {
            cwd,
            drives: std::array::from_fn(|_| None),
        }
    }

    pub(crate) fn normalize(&self, input: &[u16]) -> Result<Vec<u16>, FileError32> {
        let directory = if input.get(1) == Some(&58) {
            let drive = u8::try_from(input[0]).unwrap_or(0).to_ascii_uppercase();
            if drive.is_ascii_uppercase() {
                self.drives[(drive - b'A') as usize]
                    .as_deref()
                    .unwrap_or(&self.cwd)
            } else {
                &self.cwd
            }
        } else {
            &self.cwd
        };
        super::normalize(input, directory, false)
    }
}

impl Vfs32 {
    pub(crate) fn capture_path_scope(&self) -> FilePathScope32 {
        FilePathScope32 {
            cwd: self.cwd.clone(),
            drives: self.drive_dirs.clone(),
        }
    }
}
