use crate::files::{
    path,
    tree::{Content32, NamespaceEntry32, Node32},
    *,
};

impl Vfs32 {
    pub(crate) fn mount_directory(
        &mut self,
        input: &[u16],
        attributes: u32,
        times: FileTimes32,
    ) -> Result<FileId, FileError32> {
        self.mount(input, None, attributes | ATTR_DIRECTORY, times)
    }
    pub(crate) fn mount_asset(
        &mut self,
        input: &[u16],
        asset: AssetRef32,
        attributes: u32,
        times: FileTimes32,
    ) -> Result<FileId, FileError32> {
        if asset.id == 0 || attributes & ATTR_DIRECTORY != 0 {
            return Err(FileError32::InvalidParameter);
        }
        if self.nodes.iter().flatten().any(|node| {
            node.content
                .asset
                .is_some_and(|known| known.id == asset.id && known.length != asset.length)
        }) {
            return Err(FileError32::InvalidParameter);
        }
        self.mount(input, Some(asset), attributes, times)
    }
    fn mount(
        &mut self,
        input: &[u16],
        asset: Option<AssetRef32>,
        attributes: u32,
        times: FileTimes32,
    ) -> Result<FileId, FileError32> {
        let absolute = self.full_path(input)?;
        if let Ok(file) = self.lookup(&absolute) {
            if self.node(file)?.delete_pending {
                return Err(FileError32::DeletePending);
            }
            if asset.is_none() && self.node(file)?.directory() {
                return Ok(file);
            }
            return Err(FileError32::AlreadyExists);
        }
        let (revision, next) = self.stamp()?;
        let root = path::root_len(&absolute)?;
        let mut paths = Vec::new();
        let mut end = root;
        loop {
            let candidate = &absolute[..end];
            match self.lookup(candidate) {
                Ok(file) => {
                    if self.node(file)?.delete_pending {
                        return Err(FileError32::DeletePending);
                    }
                    if !self.node(file)?.directory() && end < absolute.len() {
                        return Err(FileError32::PathNotFound);
                    }
                }
                Err(_) => {
                    paths.push(candidate.to_vec());
                }
            }
            if end == absolute.len() {
                break;
            }
            let start = if absolute.get(end) == Some(&92) {
                end + 1
            } else {
                end
            };
            end = absolute[start..]
                .iter()
                .position(|u| *u == 92)
                .map(|p| start + p)
                .unwrap_or(absolute.len());
        }
        self.check_node_capacity(paths.len())?;
        let last = self
            .next_file
            .checked_add(paths.len() as u64)
            .ok_or(FileError32::Exhausted)?;
        let mut prepared: Vec<(Node32, NamespaceEntry32)> = Vec::new();
        prepared
            .try_reserve_exact(paths.len())
            .map_err(|_| FileError32::Allocation)?;
        for (index, path) in paths.into_iter().enumerate() {
            let final_node = path == absolute;
            let source = if final_node { asset } else { None };
            let size = source.map_or(0, |a| a.length);
            let id = FileId(self.next_file + index as u64);
            let token = crate::windows::security::Token32::virtual_process();
            let security = if let Some((parent, _)) = prepared.last() {
                crate::windows::security::SecurityDescriptor32::inherited(
                    &parent.security,
                    None,
                    &token,
                    source.is_none(),
                    crate::windows::security::FILE_GENERIC_MAPPING,
                )?
            } else if path::root_len(&path)? == path.len() {
                crate::windows::security::SecurityDescriptor32::for_token(&token)?
            } else {
                self.inherited_file_security(&path, None, &token, source.is_none())?
            };
            prepared.push((
                Node32 {
                    id,
                    attributes: if final_node {
                        attributes
                    } else {
                        ATTR_DIRECTORY
                    },
                    times,
                    changed: times.written.max(times.created),
                    size,
                    linked: true,
                    delete_pending: false,
                    delete_requested: false,
                    descriptions: 0,
                    content_revision: 1,
                    dirty: false,
                    content: Content32 {
                        asset: source,
                        visible: size,
                        pages: Vec::new(),
                        allocation: 0,
                    },
                    security,
                },
                NamespaceEntry32 {
                    key: path::fold(&path),
                    path,
                    file: id,
                },
            ));
        }
        self.validate(self.epoch, revision)?;
        let id = prepared
            .last()
            .map(|(n, _)| n.id)
            .ok_or(FileError32::AlreadyExists)?;
        for (node, entry) in prepared {
            self.insert_node(node, entry);
        }
        self.next_file = last;
        self.revision = next;
        Ok(id)
    }
}
