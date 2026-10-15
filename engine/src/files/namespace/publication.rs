use super::*;

impl Vfs32 {
    pub(crate) fn commit_namespace(&mut self, prepared: PreparedNamespace32) {
        prepared
            .validate(self)
            .expect("prepared namespace authority");
        let now = self.now;
        match prepared.operation {
            Operation32::Create(node, entry) => {
                self.deleted.retain(|e| e.key != entry.key);
                self.insert_node(node, entry);
                self.next_file += 1;
            }
            Operation32::Delete { file, immediate } => {
                let node = self.node_mut(file).unwrap();
                node.delete_pending = true;
                node.changed = now;
                if immediate {
                    self.unlink(file);
                    self.collect(file);
                }
            }
            Operation32::Move {
                updates,
                old,
                replaced,
            } => {
                if let Some(file) = replaced {
                    if let Some(index) = self.entries.iter().position(|e| e.file == file) {
                        self.entries.remove(index);
                    }
                    self.node_mut(file).unwrap().linked = false;
                    self.collect(file);
                }
                let renamed_key = updates
                    .iter()
                    .find(|e| e.file == old.file)
                    .map(|e| e.key.as_slice());
                let needs_delete = renamed_key != Some(old.key.as_slice());
                for update in updates {
                    self.deleted.retain(|e| e.key != update.key);
                    let index = self
                        .entries
                        .iter()
                        .position(|e| e.file == update.file)
                        .unwrap();
                    self.entries[index] = update;
                    let file = self.entries[index].file;
                    let node = self.node_mut(file).unwrap();
                    node.dirty = true;
                    node.changed = now;
                }
                self.entries.sort_unstable_by(|a, b| a.key.cmp(&b.key));
                if needs_delete {
                    self.deleted.push(old);
                }
            }
            Operation32::Copy { node, entry } => {
                let file = node.id;
                if let Some(entry) = entry {
                    self.deleted.retain(|e| e.key != entry.key);
                    self.overlay_pages += node.content.pages.len();
                    self.insert_node(node, entry);
                    self.next_file += 1;
                } else {
                    let old = self.node(file).unwrap().content.pages.len();
                    self.overlay_pages = self.overlay_pages - old + node.content.pages.len();
                    *self.node_mut(file).unwrap() = node;
                }
                self.cache.invalidate(file);
                self.pending_writes.retain(|s| s.file != file);
            }
            Operation32::Replace {
                replaced,
                replacement,
                target,
                backup,
                discard_backup,
                old,
                created,
            } => {
                if let Some(file) = discard_backup {
                    let index = self.entries.iter().position(|e| e.file == file).unwrap();
                    self.entries.remove(index);
                    self.node_mut(file).unwrap().linked = false;
                    self.collect(file);
                }
                self.entries
                    .retain(|e| e.file != replaced && e.file != replacement);
                self.deleted.retain(|e| e.key != target.key);
                let index = self
                    .entries
                    .binary_search_by(|e| e.key.cmp(&target.key))
                    .unwrap_err();
                self.entries.insert(index, target);
                if let Some(backup) = backup {
                    self.deleted.retain(|e| e.key != backup.key);
                    let index = self
                        .entries
                        .binary_search_by(|e| e.key.cmp(&backup.key))
                        .unwrap_err();
                    self.entries.insert(index, backup);
                    let node = self.node_mut(replaced).unwrap();
                    node.dirty = true;
                    node.changed = now;
                } else {
                    self.node_mut(replaced).unwrap().linked = false;
                    self.collect(replaced);
                }
                let node = self.node_mut(replacement).unwrap();
                node.times.created = created;
                node.changed = now;
                node.dirty = true;
                self.deleted.push(old);
            }
            Operation32::Attributes { file, attributes } => {
                let node = self.node_mut(file).unwrap();
                node.attributes = attributes;
                node.changed = now;
                node.dirty = true;
            }
            Operation32::Times {
                file,
                created,
                accessed,
                written,
                open,
            } => {
                let node = self.node_mut(file).unwrap();
                node.changed = now;
                if let Some(time) = created {
                    node.times.created = time;
                }
                if let Some(time) = accessed {
                    node.times.accessed = time;
                }
                if let Some(time) = written {
                    node.times.written = time;
                }
                node.dirty = true;
                let (open, access, write) = open;
                let description = self.open_mut(open).unwrap();
                description.suppress_access = access;
                description.suppress_write = write;
            }
            Operation32::Directory {
                directory,
                drive_copy,
            } => {
                if let Some(copy) = drive_copy {
                    self.drive_dirs[(directory[0] as u8 - b'A') as usize] = Some(copy);
                }
                self.cwd = directory;
            }
        }
        self.revision = prepared.next;
    }
}
