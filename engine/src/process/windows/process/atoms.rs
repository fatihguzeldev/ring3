use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct AtomTable32 {
    entries: BTreeMap<u16, (String, u32)>,
}
impl AtomTable32 {
    pub(super) fn find(&self, name: &str) -> Option<u16> {
        let key = crate::windows::text::ordinal_key(&name.encode_utf16().collect::<Vec<_>>());
        self.entries.iter().find_map(|(&atom, (value, _))| {
            (crate::windows::text::ordinal_key(&value.encode_utf16().collect::<Vec<_>>()) == key)
                .then_some(atom)
        })
    }
    pub(super) fn add(&mut self, name: String) -> Result<u16, u32> {
        if let Some(atom) = self.find(&name) {
            let entry = self.entries.get_mut(&atom).unwrap();
            entry.1 = entry.1.checked_add(1).ok_or(8u32)?;
            return Ok(atom);
        }
        let atom = (0xc000..=0xffff)
            .find(|atom| !self.entries.contains_key(atom))
            .ok_or(8u32)?;
        self.entries.insert(atom, (name, 1));
        Ok(atom)
    }
    pub(super) fn name(&self, atom: u16) -> Option<&str> {
        self.entries.get(&atom).map(|(name, _)| name.as_str())
    }
    pub(super) fn delete(&mut self, atom: u16) -> Result<(), u32> {
        if (1..0xc000).contains(&atom) {
            return Ok(());
        }
        let entry = self.entries.get_mut(&atom).ok_or(6u32)?;
        entry.1 -= 1;
        if entry.1 == 0 {
            self.entries.remove(&atom);
        }
        Ok(())
    }
}

impl crate::process::EngineInstance {
    pub(in crate::process) fn windows_global_atom_name(&self, atom: u16) -> Option<Vec<u16>> {
        if (1..0xc000).contains(&atom) {
            Some(format!("#{atom}").encode_utf16().collect())
        } else {
            self.process_runtime
                .atoms
                .name(atom)
                .map(|name| name.encode_utf16().collect())
        }
    }
}
