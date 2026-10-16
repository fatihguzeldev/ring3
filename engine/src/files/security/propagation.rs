use super::{metadata::PreparedFileSecurity32, *};
use crate::files::{FileId, FileShare32};

impl Vfs32 {
    pub(crate) fn prepare_file_security_with_inheritance(
        &self,
        file: FileId,
        information: u32,
        supplied: SecurityDescriptor32,
        token: &Token32,
        granted: Option<u32>,
        propagate: bool,
    ) -> Result<PreparedFileSecurity32, FileError32> {
        let mut prepared =
            self.prepare_file_security(file, information, supplied, token, granted)?;
        if !propagate || information & 4 == 0 || !self.node(file)?.directory() {
            return Ok(prepared);
        }
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.file == file)
            .ok_or(FileError32::NotFound)?;
        let mut children = Vec::new();
        children
            .try_reserve_exact(self.entries.len())
            .map_err(|_| FileError32::Allocation)?;
        for child in &self.entries {
            if child.key.starts_with(&entry.key)
                && (entry.key.last() == Some(&92) || child.key.get(entry.key.len()) == Some(&92))
                && child.file != file
            {
                children.push(child);
            }
        }
        children.sort_unstable_by_key(|entry| entry.path.len());
        prepared
            .updates
            .try_reserve(children.len())
            .map_err(|_| FileError32::Allocation)?;
        let mut planned = Vec::new();
        planned
            .try_reserve_exact(self.nodes.len())
            .map_err(|_| FileError32::Allocation)?;
        planned.resize(self.nodes.len(), None);
        let slot = |id| {
            self.node_index
                .binary_search_by_key(&id, |(id, _)| *id)
                .map(|index| self.node_index[index].1)
                .unwrap()
        };
        planned[slot(file)] = Some(0);
        for child in children {
            let parent = self.lookup(path::parent(&child.path)?)?;
            let Some(parent_index) = planned[slot(parent)] else {
                continue;
            };
            let node = self.node(child.file)?;
            if node.security.control & 0x1000 != 0 {
                continue;
            }
            match self.check_sharing(child.file, FileAccess32::READ, FileShare32::ALL) {
                Err(FileError32::SharingViolation) => continue,
                result => result?,
            }
            let mut descriptor = node.security.snapshot()?;
            let parent_descriptor = if parent_index == usize::MAX {
                &self.node(parent)?.security
            } else {
                &prepared.updates[parent_index].1
            };
            descriptor.inherit_parent_dacl(
                parent_descriptor,
                node.directory(),
                FILE_GENERIC_MAPPING,
            )?;
            if descriptor != node.security {
                planned[slot(child.file)] = Some(prepared.updates.len());
                prepared.updates.push((child.file, descriptor));
            } else {
                planned[slot(child.file)] = Some(usize::MAX);
            }
        }
        Ok(prepared)
    }
}
