use super::*;

impl Vfs32 {
    pub(crate) fn ensure_open_capacity(&self, additional: usize) -> Result<(), FileError32> {
        if self
            .opens
            .iter()
            .flatten()
            .count()
            .checked_add(additional)
            .ok_or(FileError32::Capacity)?
            > self.limits.opens
        {
            return Err(FileError32::Capacity);
        }
        self.next_open
            .checked_add(additional as u64)
            .ok_or(FileError32::Exhausted)?;
        self.revision
            .checked_add(additional as u64)
            .ok_or(FileError32::Exhausted)?;
        Ok(())
    }
    pub(crate) fn check_retain_open(&self, open: OpenFileId) -> Result<(), FileError32> {
        self.open(open)?
            .references
            .checked_add(1)
            .ok_or(FileError32::Exhausted)?;
        Ok(())
    }
    pub(crate) fn retain_open(&mut self, open: OpenFileId) -> Result<(), FileError32> {
        self.check_retain_open(open)?;
        self.open_mut(open)?.references += 1;
        Ok(())
    }
    pub(crate) fn check_close_open(&self, id: OpenFileId) -> Result<(), FileError32> {
        self.check_close_open_after_mutations(id, 0)
    }
    pub(crate) fn check_close_open_after_mutations(
        &self,
        id: OpenFileId,
        mutations: u64,
    ) -> Result<(), FileError32> {
        let open = self.open(id)?;
        if open.references > 1 {
            return Ok(());
        }
        let node = self.node(open.file)?;
        if (self.deletes_on_close(open)? || node.delete_pending)
            && node.descriptions == 1
            && self.deleted.len() >= self.limits.nodes
        {
            return Err(FileError32::Capacity);
        }
        self.revision
            .checked_add(mutations)
            .and_then(|r| r.checked_add(1))
            .ok_or(FileError32::Exhausted)?;
        Ok(())
    }
    pub(crate) fn close_open(&mut self, id: OpenFileId) -> Result<(), FileError32> {
        self.check_close_open(id)?;
        let open = self.open(id)?;
        if open.references > 1 {
            self.open_mut(id)?.references -= 1;
            return Ok(());
        }
        let file = open.file;
        let delete = self.deletes_on_close(open)?;
        let node = self.node(file)?;
        let unlink = (delete || node.delete_pending) && node.descriptions == 1;
        if unlink && self.deleted.len() >= self.limits.nodes {
            return Err(FileError32::Capacity);
        }
        let (_, next) = self.stamp()?;
        let index = self
            .opens
            .iter()
            .position(|o| o.as_ref().is_some_and(|o| o.id == id))
            .unwrap();
        self.opens[index] = None;
        let marked = self
            .opens
            .iter()
            .flatten()
            .any(|open| open.file == file && open.disposition_delete);
        let node = self.node_mut(file).unwrap();
        node.descriptions -= 1;
        node.delete_requested |= delete;
        node.delete_pending = node.delete_requested || marked;
        if unlink {
            self.unlink(file);
        }
        self.revision = next;
        self.collect(file);
        Ok(())
    }
    pub(crate) fn flush(&mut self, id: OpenFileId) -> Result<(), FileError32> {
        self.open(id)?;
        self.flushed_revision = self.revision;
        Ok(())
    }
    pub(in crate::files) fn unlink(&mut self, file: FileId) {
        if let Some(index) = self.entries.iter().position(|e| e.file == file) {
            let entry = self.entries.remove(index);
            self.deleted.push(entry);
        }
        self.node_mut(file).unwrap().linked = false;
    }
}
