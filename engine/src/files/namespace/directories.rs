use super::*;

impl Vfs32 {
    pub(crate) fn prepare_create_directory_with_security(
        &self,
        input: &[u16],
        attributes: u32,
        token: &crate::windows::security::Token32,
        explicit: Option<crate::windows::security::SecurityDescriptor32>,
    ) -> Result<PreparedNamespace32, FileError32> {
        self.check_parent_access(input, 4, token)?;
        let absolute = self.full_path(input)?;
        let mut prepared = self.prepare_create_directory(input, attributes)?;
        let Operation32::Create(node, _) = &mut prepared.operation else {
            unreachable!()
        };
        node.security = self.inherited_file_security(&absolute, explicit, token, true)?;
        Ok(prepared)
    }
    pub(crate) fn prepare_create_directory(
        &self,
        input: &[u16],
        attributes: u32,
    ) -> Result<PreparedNamespace32, FileError32> {
        let absolute = self.full_path(input)?;
        if self.lookup(&absolute).is_ok() {
            return Err(FileError32::AlreadyExists);
        }
        self.check_parent(&absolute)?;
        self.check_node_capacity(1)?;
        self.next_file
            .checked_add(1)
            .ok_or(FileError32::Exhausted)?;
        let id = FileId(self.next_file);
        let node = Node32 {
            id,
            attributes: (attributes & !ATTR_NORMAL) | ATTR_DIRECTORY,
            times: FileTimes32 {
                created: self.now,
                accessed: self.now,
                written: self.now,
            },
            changed: self.now,
            size: 0,
            linked: true,
            delete_pending: false,
            delete_requested: false,
            descriptions: 0,
            content_revision: 1,
            dirty: true,
            content: Content32::default(),
            security: self.inherited_file_security(
                &absolute,
                None,
                &crate::windows::security::Token32::virtual_process(),
                true,
            )?,
        };
        let entry = NamespaceEntry32 {
            key: path::fold(&absolute),
            path: absolute,
            file: id,
        };
        self.namespace_receipt(Operation32::Create(node, entry))
    }
    pub(crate) fn prepare_remove_directory(
        &self,
        input: &[u16],
    ) -> Result<PreparedNamespace32, FileError32> {
        let absolute = self.full_path(input)?;
        let file = self.lookup(&absolute)?;
        let node = self.node(file)?;
        if !node.directory() {
            return Err(FileError32::NotDirectory);
        }
        if path::root_len(&absolute)? == absolute.len() {
            return Err(FileError32::AccessDenied);
        }
        let cwd = path::fold(&self.cwd);
        let key = path::fold(&absolute);
        if cwd == key || (cwd.starts_with(&key) && cwd.get(key.len()) == Some(&92)) {
            return Err(FileError32::SharingViolation);
        }
        if self.entries.iter().any(|e| {
            path::parent(&e.path)
                .ok()
                .is_some_and(|p| path::fold(p) == path::fold(&absolute))
        }) {
            return Err(FileError32::DirectoryNotEmpty);
        }
        self.prepare_delete_node(file)
    }
    pub(crate) fn prepare_current_directory(
        &self,
        input: &[u16],
    ) -> Result<PreparedNamespace32, FileError32> {
        let absolute = self.full_path(input)?;
        let node = self.node(self.lookup(&absolute)?)?;
        if !node.directory() || node.delete_pending {
            return Err(FileError32::PathNotFound);
        }
        self.check_sharing(
            node.id,
            FileAccess32(0),
            FileShare32(FileShare32::READ.0 | FileShare32::WRITE.0),
        )?;
        let drive_copy = if absolute.get(1) == Some(&58) {
            Some(absolute.clone())
        } else {
            None
        };
        self.namespace_receipt(Operation32::Directory {
            directory: absolute,
            drive_copy,
        })
    }
}
