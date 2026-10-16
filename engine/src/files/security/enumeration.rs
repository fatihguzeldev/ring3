use crate::{
    files::{FileError32, Vfs32, path},
    windows::security::{FILE_GENERIC_MAPPING, Token32},
};
impl Vfs32 {
    pub(crate) fn check_find_access(
        &self,
        pattern: &[u16],
        token: &Token32,
    ) -> Result<(), FileError32> {
        let absolute = self.normalize(pattern, true)?;
        let directory = self.lookup(path::parent(&absolute)?)?;
        self.node(directory)?
            .security
            .check_access(token, 1, FILE_GENERIC_MAPPING)?;
        Ok(())
    }
}
