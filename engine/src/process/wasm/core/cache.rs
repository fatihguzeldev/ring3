use crate::{cpu::core::CompiledCode, memory::AddressSpace};
use std::{
    collections::{HashMap, VecDeque},
    rc::{Rc, Weak},
};

const MAX_REGIONS: usize = 256;
const MAX_CODE_BYTES: usize = 32 << 20;

#[derive(Default)]
pub(super) struct CoreCache32 {
    regions: VecDeque<Rc<CompiledCode>>,
    entries: HashMap<u32, Weak<CompiledCode>>,
    bytes: usize,
}

impl CoreCache32 {
    pub(super) fn lookup(&mut self, memory: &AddressSpace, pc: u32) -> Option<Rc<CompiledCode>> {
        let code = self.entries.get(&pc)?.upgrade()?;
        if !code.is_current(memory) {
            self.remove(code.identity());
            return None;
        }
        if let Some(index) = self
            .regions
            .iter()
            .position(|entry| entry.identity() == code.identity())
        {
            let entry = self.regions.remove(index).unwrap();
            self.regions.push_back(entry);
        }
        Some(code)
    }

    pub(super) fn insert(&mut self, code: CompiledCode) -> Rc<CompiledCode> {
        let code = Rc::new(code);
        while self.regions.len() >= MAX_REGIONS || self.bytes + code.wasm().len() > MAX_CODE_BYTES {
            let Some(old) = self.regions.front() else {
                break;
            };
            self.remove(old.identity());
        }
        for entry in code.entrypoints() {
            self.entries.insert(entry.0, Rc::downgrade(&code));
        }
        self.bytes += code.wasm().len();
        self.regions.push_back(Rc::clone(&code));
        code
    }

    fn remove(&mut self, identity: u64) {
        let Some(index) = self
            .regions
            .iter()
            .position(|entry| entry.identity() == identity)
        else {
            return;
        };
        let code = self.regions.remove(index).unwrap();
        self.bytes -= code.wasm().len();
        for entry in code.entrypoints() {
            if self
                .entries
                .get(&entry.0)
                .is_some_and(|weak| weak.ptr_eq(&Rc::downgrade(&code)))
            {
                self.entries.remove(&entry.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cpu::core::{CompileOptions, compile},
        memory::{GuestAddress, PageRange, Permissions},
    };

    fn memory() -> AddressSpace {
        let mut memory = AddressSpace::new(2).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x3000), 1).unwrap(),
                Permissions::READ_WRITE,
            )
            .unwrap();
        let mut code = [0; 4096];
        code[..15].copy_from_slice(&[
            0x83, 0xc0, 1, 0x75, 5, 0x83, 0xc0, 2, 0xeb, 3, 0x83, 0xc0, 3, 0x0f, 0x0b,
        ]);
        for index in 0..MAX_REGIONS + 32 {
            code[0x100 + index * 8..0x103 + index * 8].copy_from_slice(&[0x90, 0x0f, 0x0b]);
        }
        memory.write(GuestAddress(0x1000), &code).unwrap();
        memory
    }

    fn compiled(memory: &AddressSpace, pc: u32) -> CompiledCode {
        compile(
            memory,
            GuestAddress(pc),
            CompileOptions {
                instructions: 32,
                blocks: 8,
                ..CompileOptions::default()
            },
        )
        .unwrap()
    }

    fn filler(index: usize) -> u32 {
        0x1100 + index as u32 * 8
    }

    fn bounds(cache: &CoreCache32) {
        assert!(cache.regions.len() <= MAX_REGIONS);
        assert!(cache.bytes <= MAX_CODE_BYTES);
        assert_eq!(
            cache.bytes,
            cache
                .regions
                .iter()
                .map(|code| code.wasm().len())
                .sum::<usize>()
        );
        assert!(
            cache
                .entries
                .values()
                .all(|entry| entry.upgrade().is_some())
        );
    }

    #[test]
    fn source_changes_and_identical_remapping_invalidate_cached_entries() {
        let mut memory = memory();
        let mut cache = CoreCache32::default();
        let first = cache.insert(compiled(&memory, 0x1000));
        memory.write(GuestAddress(0x3000), &[7]).unwrap();
        assert!(Rc::ptr_eq(&cache.lookup(&memory, 0x1000).unwrap(), &first));
        memory.write(GuestAddress(0x1002), &[4]).unwrap();
        assert!(cache.lookup(&memory, 0x1000).is_none());
        assert!(cache.regions.is_empty());
        for entry in first.entrypoints() {
            assert!(!cache.entries.contains_key(&entry.0));
        }

        let second = cache.insert(compiled(&memory, 0x1000));
        let mut code = [0; 4096];
        memory.read(GuestAddress(0x1000), &mut code).unwrap();
        let range = PageRange::new(GuestAddress(0x1000), 1).unwrap();
        memory.unmap(range).unwrap();
        memory.map_zeroed(range, Permissions::ALL).unwrap();
        memory.write(GuestAddress(0x1000), &code).unwrap();
        assert!(!second.is_current(&memory));
        assert!(cache.lookup(&memory, 0x1005).is_none());
        assert!(cache.regions.is_empty());
        bounds(&cache);
    }

    #[test]
    fn actual_compiled_side_entries_reuse_the_same_retained_region() {
        let memory = memory();
        let mut cache = CoreCache32::default();
        let code = cache.insert(compiled(&memory, 0x1000));
        assert_eq!(
            code.entrypoints(),
            &[
                GuestAddress(0x1000),
                GuestAddress(0x1005),
                GuestAddress(0x100a),
                GuestAddress(0x100d)
            ]
        );
        for pc in [0x1000, 0x1005, 0x100a, 0x100d] {
            assert!(Rc::ptr_eq(&cache.lookup(&memory, pc).unwrap(), &code));
        }
        assert!(cache.lookup(&memory, 0x1001).is_none());
        assert_eq!(cache.regions.len(), 1);
        bounds(&cache);
    }

    #[test]
    fn lru_touch_preserves_the_touched_region_and_eviction_stays_bounded() {
        let memory = memory();
        let mut cache = CoreCache32::default();
        let first = cache.insert(compiled(&memory, filler(0)));
        for index in 1..MAX_REGIONS {
            cache.insert(compiled(&memory, filler(index)));
        }
        assert_eq!(cache.regions.len(), MAX_REGIONS);
        assert!(Rc::ptr_eq(
            &cache.lookup(&memory, filler(0)).unwrap(),
            &first
        ));
        cache.insert(compiled(&memory, filler(MAX_REGIONS)));
        assert!(cache.lookup(&memory, filler(1)).is_none());
        assert!(Rc::ptr_eq(
            &cache.lookup(&memory, filler(0)).unwrap(),
            &first
        ));
        for index in MAX_REGIONS + 1..MAX_REGIONS + 16 {
            cache.insert(compiled(&memory, filler(index)));
            bounds(&cache);
        }
        assert_eq!(cache.regions.len(), MAX_REGIONS);
    }

    #[test]
    fn newer_side_entry_alias_survives_eviction_of_the_previous_region() {
        let memory = memory();
        let mut cache = CoreCache32::default();
        let old = cache.insert(compiled(&memory, 0x1000));
        let newer = cache.insert(compiled(&memory, 0x1005));
        assert_ne!(old.identity(), newer.identity());
        assert_eq!(
            newer.entrypoints(),
            &[GuestAddress(0x1005), GuestAddress(0x100d)]
        );
        for index in 0..MAX_REGIONS - 1 {
            cache.insert(compiled(&memory, filler(index)));
        }
        assert!(
            cache
                .regions
                .iter()
                .all(|entry| entry.identity() != old.identity())
        );
        assert!(cache.lookup(&memory, 0x1000).is_none());
        assert!(cache.lookup(&memory, 0x100a).is_none());
        for pc in [0x1005, 0x100d] {
            assert!(Rc::ptr_eq(&cache.lookup(&memory, pc).unwrap(), &newer));
        }
        bounds(&cache);
    }
}
