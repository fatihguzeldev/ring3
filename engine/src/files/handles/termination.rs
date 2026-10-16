use super::*;

pub(crate) struct PreparedFileExit32 {
    epoch: u64,
    revision: u64,
    next: u64,
    closes: Vec<(OpenFileId, u32)>,
    files: Vec<(FileId, bool)>,
}
impl PreparedFileExit32 {
    pub(crate) fn validate(&self, vfs: &Vfs32) -> Result<(), FileError32> {
        vfs.validate(self.epoch, self.revision)?;
        if vfs.opens.iter().flatten().count() != self.closes.len()
            || self.closes.iter().any(|&(id, references)| {
                !vfs.open(id).is_ok_and(|open| open.references == references)
            })
        {
            return Err(FileError32::Stale);
        }
        Ok(())
    }
}
impl Vfs32 {
    pub(crate) fn prepare_process_exit(&self) -> Result<PreparedFileExit32, FileError32> {
        let count = self.opens.iter().flatten().count();
        let mut closes = Vec::new();
        closes
            .try_reserve_exact(count)
            .map_err(|_| FileError32::Allocation)?;
        let mut files = Vec::new();
        files
            .try_reserve_exact(count)
            .map_err(|_| FileError32::Allocation)?;
        for open in self.opens.iter().flatten() {
            let node = self.node(open.file)?;
            closes.push((open.id, open.references));
            // nonempty directory handles close before child handles at process exit.
            files.push((
                open.file,
                node.delete_pending || self.deletes_on_close(open)?,
            ));
        }
        files.sort_unstable_by_key(|&(file, _)| file);
        let mut retained = 0;
        for index in 0..files.len() {
            let (file, delete) = files[index];
            if retained != 0 && files[retained - 1].0 == file {
                files[retained - 1].1 |= delete;
            } else {
                files[retained] = (file, delete);
                retained += 1;
            }
        }
        files.truncate(retained);
        let unlinks = files
            .iter()
            .filter(|&&(file, delete)| delete && self.node(file).is_ok_and(|node| node.linked))
            .count();
        if self
            .deleted
            .len()
            .checked_add(unlinks)
            .ok_or(FileError32::Capacity)?
            > self.limits.nodes
        {
            return Err(FileError32::Capacity);
        }
        let next = self
            .revision
            .checked_add(count as u64)
            .ok_or(FileError32::Exhausted)?;
        Ok(PreparedFileExit32 {
            epoch: self.epoch,
            revision: self.revision,
            next,
            closes,
            files,
        })
    }
    pub(crate) fn commit_process_exit(&mut self, prepared: PreparedFileExit32) {
        debug_assert!(prepared.validate(self).is_ok());
        for open in &mut self.opens {
            *open = None;
        }
        for (file, delete) in prepared.files {
            let node = self.node_mut(file).unwrap();
            node.descriptions = 0;
            if delete {
                node.delete_pending = true;
                node.delete_requested = true;
                self.unlink(file);
            }
            self.collect(file);
        }
        self.revision = prepared.next;
        self.pending_writes.clear();
        self.cache
            .stages
            .retain(|stage| stage.owner == FileIoOwner32::HostExport);
        let stages = &self.cache.stages;
        self.cache.pages.retain(|page| {
            page.supplied == page.request.length as usize
                || stages
                    .iter()
                    .any(|stage| stage.waiting == Some(page.request))
        });
    }
}
