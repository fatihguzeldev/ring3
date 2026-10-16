use super::*;
use crate::files::FileId;

pub(crate) struct PreparedFileSecurity32 {
    epoch: u64,
    revision: u64,
    next: u64,
    pub(super) updates: Vec<(FileId, SecurityDescriptor32)>,
}
impl PreparedFileSecurity32 {
    pub(crate) fn validate(&self, vfs: &Vfs32) -> Result<(), FileError32> {
        vfs.validate(self.epoch, self.revision)
    }
}
impl Vfs32 {
    pub(crate) fn file_security(
        &self,
        file: FileId,
        information: u32,
        token: &Token32,
        granted: Option<u32>,
    ) -> Result<SecurityDescriptor32, FileError32> {
        crate::windows::files::security::query(
            &self.node(file)?.security,
            information,
            token,
            granted,
        )
    }
    pub(crate) fn prepare_file_security(
        &self,
        file: FileId,
        information: u32,
        supplied: SecurityDescriptor32,
        token: &Token32,
        granted: Option<u32>,
    ) -> Result<PreparedFileSecurity32, FileError32> {
        let descriptor = crate::windows::files::security::change(
            &self.node(file)?.security,
            information,
            supplied,
            token,
            granted,
        )?;
        let (revision, next) = self.stamp()?;
        let mut updates = Vec::new();
        updates
            .try_reserve_exact(1)
            .map_err(|_| FileError32::Allocation)?;
        updates.push((file, descriptor));
        Ok(PreparedFileSecurity32 {
            epoch: self.epoch,
            revision,
            next,
            updates,
        })
    }
    pub(crate) fn commit_file_security(&mut self, prepared: PreparedFileSecurity32) {
        prepared
            .validate(self)
            .expect("prepared file security authority");
        let now = self.now;
        for (file, descriptor) in prepared.updates {
            let node = self.node_mut(file).unwrap();
            node.security = descriptor;
            node.changed = now;
            node.dirty = true;
        }
        self.revision = prepared.next;
    }
}
