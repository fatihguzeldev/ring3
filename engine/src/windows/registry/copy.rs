use super::{
    PreparedRegistry32, Registry32, RegistryChangeKind32 as Kind, RegistryError32 as E,
    RegistryKeyId32 as Id,
    model::{Key32, Target32, Value32},
    path,
    prepared::Action32,
};
use crate::windows::text::ordinal_key;
struct Node32 {
    relative: Vec<u16>,
    class: Vec<u16>,
    values: Vec<Value32>,
}
fn copy_value(value: &Value32) -> Result<Value32, E> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(value.bytes.len())
        .map_err(|_| E::NotEnoughMemory)?;
    bytes.extend_from_slice(&value.bytes);
    Ok(Value32 {
        name: value.name.clone(),
        ordinal: value.ordinal.clone(),
        kind: value.kind,
        bytes,
    })
}
impl Registry32 {
    fn copy_nodes(&self, source: Target32) -> Result<Vec<Node32>, E> {
        let mut work = Vec::new();
        work.try_reserve(1).map_err(|_| E::NotEnoughMemory)?;
        work.push((source, Vec::new()));
        let mut nodes = Vec::new();
        let mut cursor = 0;
        while cursor < work.len() {
            let (target, relative) = &work[cursor];
            let target = self.effective_target(*target)?;
            let key = self.live_key(target.primary)?;
            let mut values = Vec::new();
            for value in self.values_for(target)? {
                values.try_reserve(1).map_err(|_| E::NotEnoughMemory)?;
                values.push(copy_value(value)?);
            }
            let children = self.keys_for(target)?;
            let mut next = Vec::new();
            next.try_reserve_exact(children.len())
                .map_err(|_| E::NotEnoughMemory)?;
            for (child, start) in children {
                let name = &child.path[start..];
                let child_target = if target.classes {
                    self.classes_target(
                        &path::joined(&self.relative_classes(target)?, name)?,
                        key.view,
                    )?
                } else {
                    Target32 {
                        primary: self.lookup(child.hive, child.view, &child.path).unwrap(),
                        fallback: None,
                        classes: false,
                    }
                };
                next.push((child_target, path::joined(relative, name)?));
            }
            nodes.try_reserve(1).map_err(|_| E::NotEnoughMemory)?;
            nodes.push(Node32 {
                relative: relative.clone(),
                class: key.class.clone(),
                values,
            });
            work.try_reserve(next.len())
                .map_err(|_| E::NotEnoughMemory)?;
            work.extend(next);
            cursor += 1;
            if work.len() > self.config.limits.keys as usize {
                return Err(E::NotEnoughMemory);
            }
        }
        Ok(nodes)
    }
    pub fn prepare_copy_tree(
        &mut self,
        source: u32,
        subkey: &[u16],
        destination: u32,
        modified: u64,
    ) -> Result<PreparedRegistry32, E> {
        self.target(source, 9)?;
        let (source, _) = self.resolved(source, subkey, 0)?;
        let (destination, _) = self.target(destination, 6)?;
        let destination = self.effective_target(destination)?;
        let base = self.live_key(destination.primary)?;
        let (hive, view, base_path, base_volatile) =
            (base.hive, base.view, base.path.clone(), base.volatile);
        let source_key = self.live_key(source.primary)?;
        if source_key.hive == hive
            && source_key.view == view
            && path::starts_with(&ordinal_key(&base_path), &source_key.ordinal)
        {
            return Err(E::InvalidParameter);
        }
        let nodes = self.copy_nodes(source)?;
        let mut prepared = self.preparation()?;
        let mut used = Vec::new();
        for node in nodes {
            let full = path::joined(&base_path, &node.relative)?;
            let existing = self.lookup(hive, view, &full);
            let volatile = if let Some(id) = existing {
                self.live_key(id)?.volatile
            } else {
                let parent = &full[..full.iter().rposition(|&unit| unit == 92).unwrap_or(0)];
                if let Some(id) = self.lookup(hive, view, parent) {
                    self.live_key(id)?.volatile
                } else {
                    let ordinal = ordinal_key(parent);
                    prepared
                        .actions
                        .iter()
                        .find_map(|action| match action {
                            Action32::Create(_, key)
                                if key.hive == hive
                                    && key.view == view
                                    && key.ordinal == ordinal =>
                            {
                                Some(key.volatile)
                            }
                            _ => None,
                        })
                        .unwrap_or(base_volatile)
                }
            };
            let mut created = false;
            let id = if let Some(id) = existing {
                id
            } else {
                let slot = self
                    .keys
                    .iter()
                    .enumerate()
                    .find(|(slot, key)| key.is_none() && !used.contains(slot))
                    .map(|(slot, _)| slot)
                    .ok_or(E::NotEnoughMemory)?;
                prepared.next_key = prepared.next_key.checked_add(1).ok_or(E::NotEnoughMemory)?;
                let id = Id {
                    slot: slot as u32,
                    generation: prepared.next_key,
                };
                let key = Key32 {
                    generation: id.generation,
                    hive,
                    view,
                    path: full.clone(),
                    ordinal: ordinal_key(&full),
                    class: node.class,
                    modified,
                    values: Vec::new(),
                    deleted: false,
                    handles: 0,
                    volatile,
                };
                prepared.bytes = prepared
                    .bytes
                    .checked_add(key.bytes())
                    .ok_or(E::NotEnoughMemory)?;
                if !volatile {
                    self.stage_change(
                        &mut prepared,
                        Kind::CreateKey,
                        hive,
                        view,
                        &full,
                        &[],
                        Some(id),
                        modified,
                    )?;
                }
                prepared
                    .actions
                    .try_reserve(1)
                    .map_err(|_| E::NotEnoughMemory)?;
                prepared.actions.push(Action32::Create(slot, key));
                used.try_reserve(1).map_err(|_| E::NotEnoughMemory)?;
                used.push(slot);
                created = true;
                id
            };
            let mut values = Vec::new();
            if !created {
                let key = self.live_key(id)?;
                values
                    .try_reserve_exact(key.values.len())
                    .map_err(|_| E::NotEnoughMemory)?;
                for value in &key.values {
                    values.push(copy_value(value)?);
                }
                prepared.bytes -= values.iter().map(Value32::bytes).sum::<usize>();
                prepared.values -= values.len();
            }
            for value in node.values {
                if !volatile {
                    self.stage_change(
                        &mut prepared,
                        Kind::SetValue,
                        hive,
                        view,
                        &full,
                        &value.name,
                        Some(id),
                        modified,
                    )?;
                }
                match values.binary_search_by(|old| old.ordinal.cmp(&value.ordinal)) {
                    Ok(at) => values[at] = value,
                    Err(at) => {
                        values.try_reserve(1).map_err(|_| E::NotEnoughMemory)?;
                        values.insert(at, value);
                    }
                }
            }
            prepared.bytes = prepared
                .bytes
                .checked_add(values.iter().map(Value32::bytes).sum::<usize>())
                .ok_or(E::NotEnoughMemory)?;
            prepared.values = prepared
                .values
                .checked_add(values.len())
                .ok_or(E::NotEnoughMemory)?;
            if prepared.bytes > self.config.limits.bytes as usize
                || prepared.values > self.config.limits.values as usize
            {
                return Err(E::NotEnoughMemory);
            }
            prepared
                .actions
                .try_reserve(1)
                .map_err(|_| E::NotEnoughMemory)?;
            prepared
                .actions
                .push(Action32::ReplaceValues(id, values, modified));
        }
        self.namespace
            .try_reserve(used.len())
            .map_err(|_| E::NotEnoughMemory)?;
        self.changes
            .try_reserve(prepared.changes.len())
            .map_err(|_| E::NotEnoughMemory)?;
        Ok(prepared)
    }
}
