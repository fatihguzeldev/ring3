use super::*;

pub(crate) struct PreparedImageOpens32 {
    epoch: u64,
    revision: u64,
    next: u64,
    opens: Vec<(OpenFileId, FileSection32)>,
}
impl PreparedImageOpens32 {
    pub(crate) fn validate(&self, vfs: &Vfs32) -> Result<(), FileError32> {
        vfs.validate(self.epoch, self.revision)
    }
}
impl Vfs32 {
    pub(crate) fn prepare_image_opens(
        &self,
        opens: &[(OpenFileId, FileSection32)],
    ) -> Result<PreparedImageOpens32, FileError32> {
        let mut held = Vec::new();
        held.try_reserve_exact(opens.len())
            .map_err(|_| FileError32::Allocation)?;
        for (index, &(id, section)) in opens.iter().enumerate() {
            if opens[..index].iter().any(|(previous, _)| *previous == id) {
                return Err(FileError32::InvalidParameter);
            }
            let open = self.open(id)?;
            let node = self.node(open.file)?;
            if open.section.is_some()
                || !open.options.access.contains(FileAccess32::READ)
                || node.directory()
                || node.delete_pending
                || self
                    .opens
                    .iter()
                    .flatten()
                    .any(|other| other.file == open.file && other.options.delete_on_close)
            {
                return Err(FileError32::AccessDenied);
            }
            self.check_sharing(open.file, FileAccess32::READ, section.share())?;
            held.push((id, section));
        }
        let next = self
            .revision
            .checked_add(opens.len() as u64)
            .ok_or(FileError32::Exhausted)?;
        Ok(PreparedImageOpens32 {
            epoch: self.epoch,
            revision: self.revision,
            next,
            opens: held,
        })
    }
    pub(crate) fn commit_image_opens(&mut self, prepared: PreparedImageOpens32) {
        debug_assert!(prepared.validate(self).is_ok());
        for (id, section) in prepared.opens {
            let open = self
                .open_mut(id)
                .expect("image open was prepared before mapping publication");
            open.section = Some(section);
            open.options.share = section.share();
        }
        self.revision = prepared.next;
    }
    pub(in crate::files) fn has_read_only_section(&self, file: FileId) -> bool {
        self.opens.iter().flatten().any(|open| {
            open.file == file
                && open
                    .section
                    .is_some_and(|section| section != FileSection32::SharedData)
        })
    }
    pub(crate) fn has_file_section(&self, file: FileId) -> bool {
        self.opens
            .iter()
            .flatten()
            .any(|open| open.file == file && open.section.is_some())
    }
}
