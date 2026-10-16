use crate::windows::registry::{
    PreparedRegistry32, Registry32, RegistryChangeId32, RegistryChangeKind32 as Kind,
    RegistryError32 as E, RegistryKeyId32 as Id, RegistryView32 as View,
    model::{Change32, Hive32},
    path,
};
use crate::windows::text::ordinal_key;

impl Registry32 {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::windows::registry) fn stage_change(
        &self,
        prepared: &mut PreparedRegistry32,
        kind: Kind,
        hive: Hive32,
        view: View,
        path: &[u16],
        name: &[u16],
        key: Option<Id>,
        modified: u64,
    ) -> Result<(), E> {
        let ordinal_path = ordinal_key(path);
        let ordinal_name = ordinal_key(name);
        for change in &self.changes {
            let same = change.hive == hive
                && change.view == view
                && (if kind == Kind::DeleteTree {
                    path::starts_with(&ordinal_key(&change.path), &ordinal_path)
                } else {
                    change.kind == kind
                        && ordinal_key(&change.path) == ordinal_path
                        && ordinal_key(&change.name) == ordinal_name
                        || matches!(kind, Kind::SetValue | Kind::DeleteValue)
                            && matches!(change.kind, Kind::SetValue | Kind::DeleteValue)
                            && ordinal_key(&change.path) == ordinal_path
                            && ordinal_key(&change.name) == ordinal_name
                });
            if same && !prepared.remove_changes.contains(&change.id.sequence) {
                prepared
                    .remove_changes
                    .try_reserve(1)
                    .map_err(|_| E::NotEnoughMemory)?;
                prepared.remove_changes.push(change.id.sequence);
                prepared.bytes -= change.bytes();
            }
        }
        if self.changes.len() - prepared.remove_changes.len() + prepared.changes.len()
            >= self.config.limits.changes as usize
        {
            return Err(E::NotEnoughMemory);
        }
        prepared.next_change = prepared
            .next_change
            .checked_add(1)
            .ok_or(E::NotEnoughMemory)?;
        let change = Change32 {
            id: RegistryChangeId32 {
                epoch: self.epoch,
                sequence: prepared.next_change,
                revision: self.revision + 1,
            },
            kind,
            hive,
            view,
            path: path.to_vec(),
            name: name.to_vec(),
            key,
            modified,
        };
        prepared.bytes = prepared
            .bytes
            .checked_add(change.bytes())
            .ok_or(E::NotEnoughMemory)?;
        prepared
            .changes
            .try_reserve(1)
            .map_err(|_| E::NotEnoughMemory)?;
        prepared.changes.push(change);
        Ok(())
    }
}
