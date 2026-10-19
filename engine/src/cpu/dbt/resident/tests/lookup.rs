use super::*;
use crate::{
    cpu::x86::{SegmentContext, decode::DecodeAuthority},
    memory::{PageRange, Permissions},
};

fn context() -> CodeFetchContext {
    CodeFetchContext::from_segments(&SegmentContext::default()).unwrap()
}

fn fixture() -> (AddressSpace, ResidentRegistry) {
    let mut memory = AddressSpace::new(8).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 8).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    for page in 1..=8 {
        memory
            .write(GuestAddress(page * 4096), &[0x90, 0xeb, 0])
            .unwrap();
    }
    let registry = ResidentRegistry::new(&memory, RegistryLimits::default()).unwrap();
    (memory, registry)
}

fn compile(registry: &mut ResidentRegistry, memory: &AddressSpace, pc: u32) -> UnitId {
    registry
        .compile(
            memory,
            &[BlockSpec {
                entry: GuestAddress(pc),
                byte_length: 3,
            }],
            CompileLimits::default(),
        )
        .unwrap()
}

#[test]
fn exact_candidates_reject_other_units_instruction_interiors_and_holes() {
    let (memory, mut registry) = fixture();
    let ids = (1..=8)
        .map(|page| compile(&mut registry, &memory, page * 4096))
        .collect::<Vec<_>>();
    for (index, &id) in ids.iter().enumerate() {
        let pc = (index as u32 + 1) * 4096;
        assert_eq!(registry.entry_index.candidates(pc), &[id]);
        assert!(registry.entry_index.candidates(pc + 4095).is_empty());
        assert_eq!(
            registry.lookup_bound(&memory, GuestAddress(pc), context()),
            Ok(id)
        );
        assert_eq!(registry.lookup(&memory, GuestAddress(pc + 1)), Ok(id));
        for hole in [pc + 2, pc + 4095] {
            assert_eq!(
                registry.lookup(&memory, GuestAddress(hole)),
                Err(RegistryError::NotFound {
                    pc: GuestAddress(hole)
                })
            );
            assert_eq!(
                registry.lookup_bound(&memory, GuestAddress(hole), context()),
                Err(RegistryError::NotFound {
                    pc: GuestAddress(hole)
                })
            );
        }
    }
    assert!(registry.entry_index.candidates(0).is_empty());
    assert!(registry.entry_index.candidates(u32::MAX).is_empty());
    let foreign = AddressSpace::new(1).unwrap();
    for pc in [0, 0x1000] {
        assert_eq!(
            registry.lookup_bound(&foreign, GuestAddress(pc), context()),
            Err(RegistryError::WrongAddressSpace)
        );
    }
}

#[test]
fn dense_page_entries_select_exact_offsets_and_retire_without_other_candidates() {
    let (mut memory, _) = fixture();
    let mut registry = ResidentRegistry::new(
        &memory,
        RegistryLimits {
            units: 32,
            wasm_bytes: 4 * 1024 * 1024,
        },
    )
    .unwrap();
    for index in 0..32 {
        let pc = 0x1000 + index * 4;
        memory.write(GuestAddress(pc), &[0x90, 0xeb, 0]).unwrap();
    }
    let entries = (0..32)
        .map(|index| {
            let pc = 0x1000 + index * 4;
            (pc, compile(&mut registry, &memory, pc))
        })
        .collect::<Vec<_>>();
    for &(pc, id) in &entries {
        for entry in [pc, pc + 1] {
            assert_eq!(registry.entry_index.candidates(entry), &[id]);
            assert_eq!(registry.lookup(&memory, GuestAddress(entry)), Ok(id));
        }
        for hole in [pc + 2, pc + 3] {
            assert!(registry.entry_index.candidates(hole).is_empty());
        }
    }
    for &(pc, id) in entries.iter().step_by(2) {
        registry.discard_current_raw(&memory, id.get()).unwrap();
        assert!(registry.entry_index.candidates(pc).is_empty());
        assert!(registry.entry_index.candidates(pc + 1).is_empty());
    }
    for &(pc, id) in entries.iter().skip(1).step_by(2) {
        assert_eq!(registry.entry_index.candidates(pc), &[id]);
        assert_eq!(registry.lookup(&memory, GuestAddress(pc + 1)), Ok(id));
    }
}

#[test]
fn unordered_unsigned_material_gate_and_source_free_fault_pages_are_all_indexed() {
    let mut memory = AddressSpace::new(2).unwrap();
    for page in [0, 0xffff_f000] {
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(page), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
    }
    memory.write(GuestAddress(0), &[0x90, 0xeb, 0xfe]).unwrap();
    let gate = GuestAddress(u32::MAX - 1);
    memory.write(gate, &[0x0f, 0x0b]).unwrap();
    let denied = GuestAddress(0x3000);
    let mut registry = ResidentRegistry::new(&memory, RegistryLimits::default()).unwrap();
    let id = registry
        .compile_entries_bound(
            &memory,
            &[gate, GuestAddress(0)],
            &[GateSpec {
                entry: gate,
                id: 17,
            }],
            CompileLimits::default(),
            7,
            Some(context()),
            DecodeAuthority::default(),
        )
        .unwrap();
    let fault = registry
        .compile_runtime_bound(
            &memory,
            denied,
            CompileLimits::default(),
            7,
            context(),
            DecodeAuthority::default(),
        )
        .unwrap();
    assert_eq!(registry.entries[0].entry_pages, [0, 0xfffff]);
    assert_eq!(registry.entries[1].entry_pages, [3]);
    for (pc, unit) in [(0, id), (gate.0, id), (denied.0, fault)] {
        assert_eq!(registry.entry_index.candidates(pc), &[unit]);
        assert_eq!(
            registry.lookup_bound(&memory, GuestAddress(pc), context()),
            Ok(unit)
        );
    }
    for pc in [2, 0x1000, 0x8000_0000, u32::MAX] {
        assert_eq!(
            registry.lookup_bound(&memory, GuestAddress(pc), context()),
            Err(RegistryError::NotFound {
                pc: GuestAddress(pc)
            })
        );
    }
    registry.discard_quiescent_raw(&memory, id.get()).unwrap();
    registry
        .discard_quiescent_raw(&memory, fault.get())
        .unwrap();
    for pc in [0, gate.0, denied.0] {
        assert!(registry.entry_index.candidates(pc).is_empty());
    }
}

#[test]
fn candidate_ids_survive_vector_holes_stale_overlap_and_republication_without_resurrection() {
    let (mut memory, mut registry) = fixture();
    let first = compile(&mut registry, &memory, 0x1000);
    let middle = compile(&mut registry, &memory, 0x2000);
    let last = compile(&mut registry, &memory, 0x3000);
    memory.write(GuestAddress(0x1000), &[0x40]).unwrap();
    let replacement = compile(&mut registry, &memory, 0x1000);
    assert_eq!(
        registry.entry_index.candidates(0x1000),
        &[first, replacement]
    );
    assert_eq!(
        registry.lookup(&memory, GuestAddress(0x1000)),
        Ok(replacement)
    );
    assert_eq!(
        registry.lookup_bound(&memory, GuestAddress(0x1000), context()),
        Ok(replacement)
    );
    registry.retire_stale(&memory, first).unwrap();
    assert_eq!(registry.entry_index.candidates(0x1000), &[replacement]);
    registry.discard_current_raw(&memory, middle.get()).unwrap();
    assert!(registry.entry_index.candidates(0x2000).is_empty());
    assert_eq!(registry.lookup(&memory, GuestAddress(0x3000)), Ok(last));
    registry.discard_current_raw(&memory, last.get()).unwrap();
    registry
        .discard_current_raw(&memory, replacement.get())
        .unwrap();
    let newest = compile(&mut registry, &memory, 0x1000);
    assert!(newest.get() > replacement.get());
    assert_eq!(registry.entry_index.candidates(0x1000), &[newest]);
    assert_eq!(registry.lookup(&memory, GuestAddress(0x1000)), Ok(newest));
    for old in [first, middle, last, replacement] {
        assert_eq!(
            registry.get_raw(&memory, old.get()).unwrap_err(),
            RegistryError::InvalidUnit
        );
    }
}

#[test]
fn abandoned_page_preparation_and_failed_admission_publish_no_candidates() {
    let (memory, mut registry) = fixture();
    let kept = compile(&mut registry, &memory, 0x1000);
    let usage = registry.usage();
    let region = compile_region(
        &memory,
        &[0x2000, 0x1000].map(|pc| BlockSpec {
            entry: GuestAddress(pc),
            byte_length: 3,
        }),
        CompileLimits::default(),
    )
    .unwrap();
    let prepared = registry.entry_index.prepare(&region).unwrap();
    drop(prepared);
    assert_eq!(registry.usage(), usage);
    assert_eq!(registry.entry_index.candidates(0x1000), &[kept]);
    assert!(registry.entry_index.candidates(0x2000).is_empty());
    assert_eq!(
        registry.compile(
            &memory,
            &[BlockSpec {
                entry: GuestAddress(0x1000),
                byte_length: 3
            }],
            CompileLimits::default(),
        ),
        Err(RegistryError::InstructionOverlap {
            pc: GuestAddress(0x1000)
        })
    );
    assert_eq!(registry.usage(), usage);
    assert_eq!(registry.entry_index.candidates(0x1000), &[kept]);
    assert!(registry.entry_index.candidates(0x2000).is_empty());
    assert_eq!(registry.lookup(&memory, GuestAddress(0x1000)), Ok(kept));
}

#[path = "lookup/union.rs"]
mod union;
