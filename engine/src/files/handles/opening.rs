use super::*;

impl Vfs32 {
    pub(crate) fn prepare_open(
        &mut self,
        input: &[u16],
        options: FileOpenOptions32,
    ) -> Result<PreparedOpen32, FileError32> {
        if options.access.0 & !FileAccess32::ALL.0 != 0
            || options.share.0 & !7 != 0
            || options.attributes & ATTR_DIRECTORY != 0
        {
            return Err(FileError32::InvalidParameter);
        }
        if options.delete_on_close && !options.access.contains(FileAccess32::DELETE) {
            return Err(FileError32::AccessDenied);
        }
        if self.opens.iter().flatten().count() >= self.limits.opens {
            return Err(FileError32::Capacity);
        }
        let absolute = self.full_path(input)?;
        let existing = self.lookup(&absolute).ok();
        let opened_path = absolute.clone();
        let (revision, next) = self.stamp()?;
        self.next_open
            .checked_add(1)
            .ok_or(FileError32::Exhausted)?;
        let mut created = None;
        let mut truncate = false;
        let (file, content_revision) = if let Some(file) = existing {
            let node = self.node(file)?;
            if node.delete_pending {
                return Err(FileError32::DeletePending);
            }
            if options.disposition == FileDisposition32::CreateNew {
                return Err(FileError32::FileExists);
            }
            if node.directory()
                && options.delete_on_close
                && path::root_len(&absolute)? == absolute.len()
            {
                return Err(FileError32::AccessDenied);
            }
            if node.directory()
                && (!options.directory || options.disposition != FileDisposition32::OpenExisting)
            {
                return Err(FileError32::AccessDenied);
            }
            if node.attributes & ATTR_READONLY != 0
                && !node.directory()
                && options.access.0
                    & (FileAccess32::WRITE.0 | FileAccess32::APPEND.0 | FileAccess32::DELETE.0)
                    != 0
            {
                return Err(FileError32::AccessDenied);
            }
            self.check_sharing(file, options.access, options.share)?;
            truncate = matches!(
                options.disposition,
                FileDisposition32::CreateAlways | FileDisposition32::TruncateExisting
            );
            if truncate && !options.access.contains(FileAccess32::WRITE) {
                return Err(FileError32::AccessDenied);
            }
            if options.disposition == FileDisposition32::CreateAlways
                && node.attributes & (ATTR_HIDDEN | ATTR_SYSTEM) & !options.attributes != 0
            {
                return Err(FileError32::AccessDenied);
            }
            node.descriptions
                .checked_add(1)
                .ok_or(FileError32::Exhausted)?;
            (
                file,
                if truncate {
                    node.content_revision
                        .checked_add(1)
                        .ok_or(FileError32::Exhausted)?
                } else {
                    node.content_revision
                },
            )
        } else {
            if matches!(
                options.disposition,
                FileDisposition32::OpenExisting | FileDisposition32::TruncateExisting
            ) {
                self.check_parent(&absolute)?;
                return Err(FileError32::NotFound);
            }
            self.check_parent(&absolute)?;
            self.check_node_capacity(1)?;
            self.next_file
                .checked_add(1)
                .ok_or(FileError32::Exhausted)?;
            let file = FileId(self.next_file);
            let times = FileTimes32 {
                created: self.now,
                accessed: self.now,
                written: self.now,
            };
            let attributes = if options.attributes == 0 {
                ATTR_NORMAL
            } else {
                options.attributes
            };
            created = Some((
                Node32 {
                    id: file,
                    attributes,
                    times,
                    changed: self.now,
                    size: 0,
                    linked: true,
                    delete_pending: false,
                    descriptions: 0,
                    content_revision: 1,
                    dirty: true,
                    content: Content32::default(),
                    security: self.inherited_file_security(
                        &absolute,
                        None,
                        &crate::windows::security::Token32::virtual_process(),
                        false,
                    )?,
                },
                NamespaceEntry32 {
                    key: path::fold(&absolute),
                    path: absolute,
                    file,
                },
            ));
            (file, 1)
        };
        let open = Open32 {
            id: OpenFileId(self.next_open),
            file,
            references: 1,
            position: 0,
            cursor_revision: 1,
            options,
            suppress_access: false,
            suppress_write: false,
            opened_path,
            maximum_allowed: false,
        };
        Ok(PreparedOpen32 {
            epoch: self.epoch,
            revision,
            next,
            result: FileOpenResult32 {
                open: open.id,
                file,
                existed: existing.is_some(),
            },
            open,
            created,
            truncate,
            content_revision,
            granted_access: super::super::security::windows_access(options.access),
        })
    }
    pub(crate) fn prepare_open_with_security(
        &mut self,
        input: &[u16],
        mut options: FileOpenOptions32,
        desired: u32,
        token: &crate::windows::security::Token32,
        explicit: Option<crate::windows::security::SecurityDescriptor32>,
    ) -> Result<PreparedOpen32, FileError32> {
        use crate::windows::security::FILE_GENERIC_MAPPING;
        let absolute = self.full_path(input)?;
        let granted = if let Ok(file) = self.lookup(&absolute) {
            self.node(file)?
                .security
                .check_access(token, desired, FILE_GENERIC_MAPPING)?
        } else {
            let parent = self.node(self.lookup(path::parent(&absolute)?)?)?;
            parent
                .security
                .check_access(token, 2, FILE_GENERIC_MAPPING)?;
            if desired & 0x01000000 != 0 {
                token.check_security_privilege()?;
            }
            if desired & 0x02000000 != 0 {
                FILE_GENERIC_MAPPING.all | (desired & 0x01000000)
            } else {
                FILE_GENERIC_MAPPING.map(desired)
            }
        };
        options.access = FileAccess32::from_windows(granted);
        let mut prepared = self.prepare_open(input, options)?;
        if let Some((node, _)) = &mut prepared.created {
            node.security = self.inherited_file_security(&absolute, explicit, token, false)?;
        }
        prepared.granted_access = granted;
        prepared.open.maximum_allowed = desired & 0x0200_0000 != 0;
        Ok(prepared)
    }
    pub(crate) fn commit_open(&mut self, prepared: PreparedOpen32) -> FileOpenResult32 {
        prepared
            .validate(self)
            .expect("prepared file open authority");
        if let Some((node, entry)) = prepared.created {
            self.deleted.retain(|p| p.key != entry.key);
            self.insert_node(node, entry);
            self.next_file += 1;
        }
        if prepared.truncate {
            let now = self.now;
            let node = self.node_mut(prepared.result.file).unwrap();
            let old = node.content.pages.len();
            node.content = Content32::default();
            node.size = 0;
            node.content_revision = prepared.content_revision;
            node.dirty = true;
            node.attributes = (node.attributes & !ATTR_NORMAL) | ATTR_ARCHIVE;
            node.times.written = now;
            node.changed = now;
            self.overlay_pages -= old;
            self.cache.invalidate(prepared.result.file);
        }
        self.node_mut(prepared.result.file).unwrap().descriptions += 1;
        if let Some(slot) = self.opens.iter_mut().find(|o| o.is_none()) {
            *slot = Some(prepared.open);
        } else {
            self.opens.push(Some(prepared.open));
        }
        self.next_open += 1;
        self.revision = prepared.next;
        prepared.result
    }
}
