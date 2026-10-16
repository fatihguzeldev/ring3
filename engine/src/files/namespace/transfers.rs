use super::*;

impl Vfs32 {
    pub(crate) fn prepare_move(
        &self,
        source: &[u16],
        target: &[u16],
        replace: bool,
    ) -> Result<PreparedNamespace32, FileError32> {
        self.prepare_move_except(source, target, replace, None)
    }
    pub(crate) fn prepare_handle_rename(
        &self,
        open: OpenFileId,
        target: &[u16],
        replace: bool,
    ) -> Result<PreparedNamespace32, FileError32> {
        let description = self.open(open)?;
        if !description.options.access.contains(FileAccess32::DELETE) {
            return Err(FileError32::AccessDenied);
        }
        let source = self.file_path(description.file)?;
        self.prepare_move_except(&source, target, replace, Some(open))
    }
    fn prepare_move_except(
        &self,
        source: &[u16],
        target: &[u16],
        replace: bool,
        except: Option<OpenFileId>,
    ) -> Result<PreparedNamespace32, FileError32> {
        let source = self.full_path(source)?;
        let target = self.full_path(target)?;
        let source_key = path::fold(&source);
        let target_key = path::fold(&target);
        if path::fold(&source[..path::root_len(&source)?])
            != path::fold(&target[..path::root_len(&target)?])
        {
            return Err(FileError32::NotSameDevice);
        }
        let file = self.lookup(&source)?;
        let node = self.node(file)?;
        if node.delete_pending || path::root_len(&source)? == source.len() {
            return Err(FileError32::AccessDenied);
        }
        if target_key.starts_with(&source_key) && target_key.get(source_key.len()) == Some(&92) {
            return Err(FileError32::AccessDenied);
        }
        self.check_parent(&target)?;
        self.check_sharing_except(file, FileAccess32::DELETE, FileShare32::ALL, except)?;
        let replaced = match self.lookup(&target) {
            Ok(id) if id == file => None,
            Ok(id) => {
                if !replace {
                    return Err(FileError32::AlreadyExists);
                }
                let target_node = self.node(id)?;
                if self.has_file_section(id) {
                    return Err(FileError32::AccessDenied);
                }
                if node.directory()
                    || target_node.directory()
                    || target_node.attributes & ATTR_READONLY != 0
                    || target_node.delete_pending
                {
                    return Err(FileError32::AccessDenied);
                }
                self.check_sharing(id, FileAccess32::DELETE, FileShare32::ALL)?;
                Some(id)
            }
            Err(_) => None,
        };
        let mut updates = Vec::new();
        let mut old = Vec::new();
        for entry in &self.entries {
            if entry.key == source_key
                || (entry.key.starts_with(&source_key)
                    && entry.key.get(source_key.len()) == Some(&92))
            {
                let mut path = target.clone();
                path.extend_from_slice(&entry.path[source.len()..]);
                if path.len() > 32767 {
                    return Err(FileError32::InvalidName);
                }
                if source_key != target_key {
                    old.try_reserve(1).map_err(|_| FileError32::Allocation)?;
                    old.push(entry.clone());
                }
                updates
                    .try_reserve(1)
                    .map_err(|_| FileError32::Allocation)?;
                updates.push(NamespaceEntry32 {
                    key: path::fold(&path),
                    path,
                    file: entry.file,
                });
            }
        }
        let remaining = self
            .deleted
            .iter()
            .filter(|entry| {
                updates
                    .binary_search_by(|update| update.key.cmp(&entry.key))
                    .is_err()
            })
            .count();
        if remaining
            .checked_add(old.len())
            .ok_or(FileError32::Capacity)?
            > self.limits.nodes
        {
            return Err(FileError32::Capacity);
        }
        self.namespace_receipt(Operation32::Move {
            updates,
            old,
            replaced,
        })
    }
    pub(crate) fn prepare_copy(
        &self,
        source: &[u16],
        target: &[u16],
        fail_exists: bool,
    ) -> Result<FileIo32<PreparedNamespace32>, FileError32> {
        let source = self.full_path(source)?;
        let target = self.full_path(target)?;
        let file = self.lookup(&source)?;
        let original = self.node(file)?;
        if original.directory() || original.delete_pending {
            return Err(FileError32::AccessDenied);
        }
        self.check_sharing(file, FileAccess32::READ, FileShare32::ALL)?;
        self.check_parent(&target)?;
        let existing = self.lookup(&target).ok();
        if existing == Some(file) {
            return Err(FileError32::SharingViolation);
        }
        let (id, content_revision, entry, old_pages, descriptions) = if let Some(id) = existing {
            if fail_exists {
                return Err(FileError32::FileExists);
            }
            let node = self.node(id)?;
            if node.directory()
                || node.attributes & (ATTR_READONLY | ATTR_HIDDEN) != 0
                || node.delete_pending
            {
                return Err(FileError32::AccessDenied);
            }
            self.check_sharing(id, FileAccess32::WRITE, FileShare32::ALL)?;
            (
                id,
                node.content_revision
                    .checked_add(1)
                    .ok_or(FileError32::Exhausted)?,
                None,
                node.content.pages.len(),
                node.descriptions,
            )
        } else {
            self.check_node_capacity(1)?;
            self.next_file
                .checked_add(1)
                .ok_or(FileError32::Exhausted)?;
            let id = FileId(self.next_file);
            (
                id,
                1,
                Some(NamespaceEntry32 {
                    key: path::fold(&target),
                    path: target,
                    file: id,
                }),
                0,
                0,
            )
        };
        if self.overlay_pages - old_pages + original.content.pages.len() > self.limits.overlay_pages
        {
            return Err(FileError32::DiskFull);
        }
        let content = original.content.snapshot()?;
        let node = Node32 {
            id,
            attributes: original.attributes,
            times: original.times,
            changed: self.now,
            size: original.size,
            linked: true,
            delete_pending: false,
            delete_requested: false,
            descriptions,
            content_revision,
            dirty: true,
            content,
            security: original.security.snapshot()?,
        };
        Ok(FileIo32::Ready(
            self.namespace_receipt(Operation32::Copy { node, entry })?,
        ))
    }
}

impl Vfs32 {
    pub(crate) fn prepare_copy_file(
        &self,
        source: FileId,
        target: FileId,
    ) -> Result<PreparedNamespace32, FileError32> {
        if source == target {
            return Err(FileError32::SharingViolation);
        }
        let original = self.node(source)?;
        let destination = self.node(target)?;
        if original.directory() || destination.directory() {
            return Err(FileError32::AccessDenied);
        }
        let content_revision = destination
            .content_revision
            .checked_add(1)
            .ok_or(FileError32::Exhausted)?;
        if self.overlay_pages - destination.content.pages.len() + original.content.pages.len()
            > self.limits.overlay_pages
        {
            return Err(FileError32::DiskFull);
        }
        let content = original.content.snapshot()?;
        let node = Node32 {
            id: target,
            attributes: original.attributes,
            times: original.times,
            changed: self.now,
            size: original.size,
            linked: destination.linked,
            delete_pending: destination.delete_pending,
            delete_requested: destination.delete_requested,
            descriptions: destination.descriptions,
            content_revision,
            dirty: true,
            content,
            security: original.security.snapshot()?,
        };
        self.namespace_receipt(Operation32::Copy { node, entry: None })
    }
}
