use super::*;

impl Vfs32 {
    pub(crate) fn check_close_open_group(
        &self,
        closes: &[(OpenFileId, u32)],
        after_mutations: u64,
        pending_delete: &[FileId],
    ) -> Result<(), FileError32> {
        if closes.len() > self.limits.opens {
            return Err(FileError32::InvalidParameter);
        }
        for &(id, count) in closes {
            if count == 0 {
                return Err(FileError32::InvalidParameter);
            }
            self.open(id)?;
        }
        for &file in pending_delete {
            self.node(file)?;
        }
        let total = |id: OpenFileId| {
            closes
                .iter()
                .filter(|(open, _)| *open == id)
                .try_fold(0u32, |n, (_, count)| n.checked_add(*count))
                .ok_or(FileError32::InvalidParameter)
        };
        let mut descriptions = 0u64;
        let mut unlinks = 0usize;
        for (index, &(id, _)) in closes.iter().enumerate() {
            if closes[..index].iter().any(|(previous, _)| *previous == id) {
                continue;
            }
            let open = self.open(id)?;
            let count = total(id)?;
            if count > open.references {
                return Err(FileError32::InvalidHandle);
            }
            if count == open.references {
                descriptions += 1;
            }
            if closes[..index].iter().any(|(previous, _)| {
                self.open(*previous)
                    .is_ok_and(|previous| previous.file == open.file)
            }) {
                continue;
            }
            let node = self.node(open.file)?;
            let mut removed = 0u32;
            let mut delete = node.delete_pending || pending_delete.contains(&open.file);
            for (position, &(other, _)) in closes.iter().enumerate() {
                if closes[..position]
                    .iter()
                    .any(|(previous, _)| *previous == other)
                {
                    continue;
                }
                let other = self.open(other)?;
                if other.file == open.file && total(other.id)? == other.references {
                    removed += 1;
                    delete |= self.deletes_on_close(other)?;
                }
            }
            if node.linked && removed == node.descriptions && delete {
                unlinks += 1;
            }
        }
        if self
            .deleted
            .len()
            .checked_add(unlinks)
            .ok_or(FileError32::Capacity)?
            > self.limits.nodes
        {
            return Err(FileError32::Capacity);
        }
        self.revision
            .checked_add(after_mutations)
            .and_then(|n| n.checked_add(descriptions))
            .ok_or(FileError32::Exhausted)?;
        Ok(())
    }
    pub(super) fn deletes_on_close(&self, open: &Open32) -> Result<bool, FileError32> {
        Ok(open.options.delete_on_close
            && (!self.node(open.file)?.directory() || self.directory_empty(open.file)?))
    }
    pub(in crate::files) fn directory_empty(&self, file: FileId) -> Result<bool, FileError32> {
        self.node(file)?;
        let Some(entry) = self.entries.iter().find(|e| e.file == file) else {
            return Ok(true);
        };
        Ok(!self.entries.iter().any(|child| {
            child.file != file
                && child.key.starts_with(&entry.key)
                && child.key.get(entry.key.len()) == Some(&92)
        }))
    }
}
