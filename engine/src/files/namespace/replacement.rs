use super::*;

impl Vfs32 {
    pub(crate) fn prepare_replace(
        &self,
        replaced: &[u16],
        replacement: &[u16],
        backup: Option<&[u16]>,
    ) -> Result<PreparedNamespace32, FileError32> {
        let replaced = self.full_path(replaced)?;
        let replacement = self.full_path(replacement)?;
        let original_id = self.lookup(&replaced)?;
        let replacement_id = self.lookup(&replacement)?;
        if original_id == replacement_id {
            return Err(FileError32::InvalidParameter);
        }
        let original = self.node(original_id)?;
        let new = self.node(replacement_id)?;
        if original.directory()
            || new.directory()
            || original.delete_pending
            || new.delete_pending
            || original.attributes & ATTR_READONLY != 0
            || new.attributes & ATTR_READONLY != 0
        {
            return Err(FileError32::AccessDenied);
        }
        let volume = path::fold(&replaced[..path::root_len(&replaced)?]);
        if path::fold(&replacement[..path::root_len(&replacement)?]) != volume {
            return Err(FileError32::NotSameDevice);
        }
        self.check_sharing(
            original_id,
            FileAccess32(FileAccess32::READ.0 | FileAccess32::DELETE.0),
            FileShare32::ALL,
        )?;
        self.check_sharing(
            replacement_id,
            FileAccess32(FileAccess32::READ.0 | FileAccess32::WRITE.0 | FileAccess32::DELETE.0),
            FileShare32(0),
        )?;
        let mut discard_backup = None;
        let backup = if let Some(backup) = backup {
            let backup = self.full_path(backup)?;
            self.check_parent(&backup)?;
            let key = path::fold(&backup);
            if key == path::fold(&replaced) || key == path::fold(&replacement) {
                return Err(FileError32::InvalidParameter);
            }
            if path::fold(&backup[..path::root_len(&backup)?]) != volume {
                return Err(FileError32::NotSameDevice);
            }
            if let Ok(id) = self.lookup(&backup) {
                let node = self.node(id)?;
                if node.directory() || node.delete_pending || node.attributes & ATTR_READONLY != 0 {
                    return Err(FileError32::AccessDenied);
                }
                if self.has_file_section(id) {
                    return Err(FileError32::UnableToRemoveReplaced);
                }
                self.check_sharing(id, FileAccess32::DELETE, FileShare32::ALL)?;
                discard_backup = Some(id);
            }
            Some(NamespaceEntry32 {
                path: backup,
                key,
                file: original_id,
            })
        } else {
            None
        };
        if self.deleted.len() >= self.limits.nodes {
            return Err(FileError32::Capacity);
        }
        let old = self
            .entries
            .iter()
            .find(|e| e.file == replacement_id)
            .unwrap()
            .clone();
        self.namespace_receipt(Operation32::Replace {
            replaced: original_id,
            replacement: replacement_id,
            target: NamespaceEntry32 {
                key: path::fold(&replaced),
                path: replaced,
                file: replacement_id,
            },
            backup,
            discard_backup,
            old,
            created: original.times.created,
        })
    }
}
