use crate::files::{
    path,
    tree::{NamespaceEntry32, Node32},
    *,
};

impl Vfs32 {
    pub(crate) fn content_version(
        &self,
        file: FileId,
    ) -> Result<FileContentVersion32, FileError32> {
        Ok(FileContentVersion32 {
            epoch: self.epoch,
            file,
            revision: self.node(file)?.content_revision,
        })
    }
    pub(crate) fn change_time(&self, file: FileId) -> Result<u64, FileError32> {
        Ok(self.node(file)?.changed)
    }
    pub(crate) fn info(&self, file: FileId) -> Result<FileInfo32, FileError32> {
        Ok(self.node(file)?.info())
    }
    pub(crate) fn path_info(&self, path: &[u16]) -> Result<FileInfo32, FileError32> {
        self.info(self.lookup(&self.full_path(path)?)?)
    }
    pub(crate) fn file_path(&self, file: FileId) -> Result<Vec<u16>, FileError32> {
        self.entries
            .iter()
            .find(|e| e.file == file)
            .map(|e| e.path.clone())
            .ok_or(FileError32::NotFound)
    }
    pub(crate) fn find(&self, input: &[u16]) -> Result<Vec<DirectoryEntry32>, FileError32> {
        self.find_limited(input, self.limits.nodes)
    }
    pub(in crate::files) fn find_limited(
        &self,
        input: &[u16],
        limit: usize,
    ) -> Result<Vec<DirectoryEntry32>, FileError32> {
        let absolute = self.normalize(input, true)?;
        let parent = path::parent(&absolute)?;
        let directory = self.lookup(parent)?;
        if !self.node(directory)?.directory() {
            return Err(FileError32::PathNotFound);
        }
        let parent_key = path::fold(parent);
        let pattern = path::name(&absolute);
        let mut result = Vec::new();
        for entry in &self.entries {
            if path::parent(&entry.path).ok().map(path::fold).as_ref() == Some(&parent_key)
                && path::matches(pattern, path::name(&entry.path))
                && !self.node(entry.file)?.delete_pending
            {
                if result.len() >= limit {
                    return Err(FileError32::Capacity);
                }
                result.try_reserve(1).map_err(|_| FileError32::Allocation)?;
                result.push(DirectoryEntry32 {
                    name: path::name(&entry.path).to_vec(),
                    info: self.info(entry.file)?,
                });
            }
        }
        Ok(result)
    }
    pub(in crate::files) fn node(&self, file: FileId) -> Result<&Node32, FileError32> {
        let index = self
            .node_index
            .binary_search_by_key(&file, |p| p.0)
            .map_err(|_| FileError32::InvalidHandle)?;
        self.nodes[self.node_index[index].1]
            .as_ref()
            .ok_or(FileError32::InvalidHandle)
    }
    pub(in crate::files) fn node_mut(&mut self, file: FileId) -> Result<&mut Node32, FileError32> {
        let index = self
            .node_index
            .binary_search_by_key(&file, |p| p.0)
            .map_err(|_| FileError32::InvalidHandle)?;
        self.nodes[self.node_index[index].1]
            .as_mut()
            .ok_or(FileError32::InvalidHandle)
    }
    pub(in crate::files) fn lookup(&self, path: &[u16]) -> Result<FileId, FileError32> {
        let key = path::fold(path);
        self.entries
            .binary_search_by(|e| e.key.cmp(&key))
            .map(|i| self.entries[i].file)
            .map_err(|_| FileError32::NotFound)
    }
    pub(in crate::files) fn check_parent(&self, path: &[u16]) -> Result<(), FileError32> {
        let parent = path::parent(path)?;
        let id = self.lookup(parent).map_err(|_| FileError32::PathNotFound)?;
        let node = self.node(id)?;
        if !node.directory() || node.delete_pending {
            return Err(FileError32::PathNotFound);
        }
        Ok(())
    }
    pub(in crate::files) fn check_node_capacity(
        &self,
        additional: usize,
    ) -> Result<(), FileError32> {
        if self
            .nodes
            .iter()
            .flatten()
            .count()
            .checked_add(additional)
            .ok_or(FileError32::Capacity)?
            > self.limits.nodes
            || self.entries.len() + additional > self.limits.nodes
        {
            Err(FileError32::Capacity)
        } else {
            Ok(())
        }
    }
    pub(in crate::files) fn insert_node(&mut self, node: Node32, entry: NamespaceEntry32) {
        let id = node.id;
        let slot = if let Some(index) = self.nodes.iter().position(|n| n.is_none()) {
            self.nodes[index] = Some(node);
            index
        } else {
            let index = self.nodes.len();
            self.nodes.push(Some(node));
            index
        };
        let index = self
            .node_index
            .binary_search_by_key(&id, |p| p.0)
            .unwrap_err();
        self.node_index.insert(index, (id, slot));
        let index = self
            .entries
            .binary_search_by(|e| e.key.cmp(&entry.key))
            .unwrap_err();
        self.entries.insert(index, entry);
    }
    pub(in crate::files) fn collect(&mut self, file: FileId) {
        if let Ok(index) = self.node_index.binary_search_by_key(&file, |p| p.0) {
            let slot = self.node_index[index].1;
            if self.nodes[slot]
                .as_ref()
                .is_some_and(|n| !n.linked && n.descriptions == 0)
            {
                let node = self.nodes[slot].take().unwrap();
                self.node_index.remove(index);
                self.overlay_pages -= node.content.pages.len();
                self.cache.invalidate(file);
                self.pending_writes.retain(|s| s.file != file);
            }
        }
    }
}
