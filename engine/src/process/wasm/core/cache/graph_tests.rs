use super::*;
use crate::{
    cpu::{
        core::{Context32, compile_with_frontier},
        x86::State32,
    },
    memory::{PageRange, Permissions},
    process::{EngineInstance, core_numeric_tests::NumericBank},
};

fn options() -> CompileOptions {
    CompileOptions {
        instructions: 64,
        blocks: 8,
        ..CompileOptions::default()
    }
}

fn insert(cache: &mut CoreCache32, memory: &AddressSpace, pc: u32, slot: u32) -> RegionRef {
    let owner = cache
        .admit(
            memory,
            compile_with_frontier(
                memory,
                GuestAddress(pc),
                options(),
                &[],
                if pc == 0x1000 {
                    &[GuestAddress(0x2000)]
                } else {
                    &[]
                },
            )
            .unwrap(),
        )
        .unwrap();
    cache.install_link(&owner, slot).unwrap();
    owner
}

#[test]
fn changing_linked_ordinal_owners_preserves_the_persistent_numeric_bank() {
    let mut instance = EngineInstance::new(2, 7).unwrap();
    instance.map(0x1000, 1, 7).unwrap();
    instance.map(0x3000, 1, 3).unwrap();
    let memory = instance.memory.as_mut().unwrap();
    memory
        .write(GuestAddress(0x1000), &[0xd9, 0x03, 0xff, 0xe6])
        .unwrap();
    memory
        .write(
            GuestAddress(0x1040),
            &[0xd8, 0x4b, 4, 0xd9, 0x5b, 8, 0xff, 0xe2],
        )
        .unwrap();
    memory
        .write(GuestAddress(0x3000), &3_f32.to_le_bytes())
        .unwrap();
    memory
        .write(GuestAddress(0x3004), &2_f32.to_le_bytes())
        .unwrap();
    let mut cache = CoreCache32::default();
    let a = insert(&mut cache, memory, 0x1000, 1);
    let b = insert(&mut cache, memory, 0x1040, 2);
    assert_eq!(
        a.native_instructions()
            .iter()
            .map(|op| op.pc)
            .collect::<Vec<_>>(),
        [0x1000]
    );
    assert_eq!(
        b.native_instructions()
            .iter()
            .map(|op| op.pc)
            .collect::<Vec<_>>(),
        [0x1040, 0x1043]
    );
    let mut bank = NumericBank::from_instance(&instance).unwrap();
    let mut context = Context32::new(State32 {
        registers: [0, 0, 0x1000, 0x3000, 0, 0, 0x1040, 0],
        eip: 0x1000,
        ..State32::default()
    });
    assert_eq!(
        bank.execute(&mut instance, &mut context, &a.native_instructions()[0]),
        0
    );
    let state = bank.architectural_state();
    let arena = instance.arena().to_vec();
    let before_context = *context.bytes();
    let snapshot = cache.snapshot();
    for _ in 0..8193 {
        assert!(
            matches!(cache.lookup_transition(instance.memory.as_ref().unwrap(), &a, 0x1040),
            TransitionLookup::Linked(ref owner, 2) if owner.identity() == b.identity())
        );
    }
    assert_eq!(cache.snapshot(), snapshot);
    assert_eq!(bank.architectural_state(), state);
    assert_eq!(instance.arena(), arena);
    assert_eq!(*context.bytes(), before_context);
    for operation in b.native_instructions() {
        assert_eq!(bank.execute(&mut instance, &mut context, operation), 0);
    }
    let state = bank.architectural_state().0;
    assert_eq!(state.tag, 0xffff);
    assert_eq!(state.registers[7], [0, 0, 0, 0, 0, 0, 0, 0xc0, 1, 0x40]);
    assert_eq!(
        (state.instruction_pointer, state.data_pointer),
        (0x1043, 0x3008)
    );
    let mut output = [0; 4];
    instance
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(0x3008), &mut output)
        .unwrap();
    assert_eq!(output, 6_f32.to_le_bytes());
    assert_eq!(&cache.link_owners[1..3], &[a.identity(), b.identity()]);
}

fn call_memory() -> AddressSpace {
    let mut memory = AddressSpace::new(2).unwrap();
    for pc in [0x1000, 0x2000] {
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(pc), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
    }
    memory
        .write(
            GuestAddress(0x1000),
            &[0xe8, 0xfb, 0x0f, 0, 0, 0x49, 0x75, 0xf8, 0x0f, 0x0b],
        )
        .unwrap();
    memory.write(GuestAddress(0x2000), &[0x40, 0xc3]).unwrap();
    memory
}

#[test]
fn hot_cross_page_call_and_return_links_keep_both_roots_and_the_caller_continuation() {
    let memory = call_memory();
    let mut cache = CoreCache32::default();
    let caller = insert(&mut cache, &memory, 0x1000, 1);
    let callee = insert(&mut cache, &memory, 0x2000, 2);
    let snapshot = cache.snapshot();
    let caller_bytes = caller.wasm().to_vec();
    let callee_bytes = callee.wasm().to_vec();
    for _ in 0..8193 {
        for (from, pc, to, slot) in [(&caller, 0x2000, &callee, 2), (&callee, 0x1005, &caller, 1)] {
            assert!(matches!(cache.lookup_transition(&memory, from, pc),
                TransitionLookup::Linked(ref owner, selected) if owner.identity() == to.identity() && selected == slot));
        }
    }
    assert_eq!(cache.snapshot(), snapshot);
    assert_eq!(caller.wasm(), caller_bytes);
    assert_eq!(callee.wasm(), callee_bytes);
    assert_eq!(caller.formation_roots(), &[GuestAddress(0x1000)]);
    assert_eq!(callee.formation_roots(), &[GuestAddress(0x2000)]);
    for (pc, id) in [
        (0x1000, caller.identity()),
        (0x1005, caller.identity()),
        (0x2000, callee.identity()),
    ] {
        assert_eq!(cache.lookup(&memory, pc).unwrap().identity(), id);
    }
    assert_eq!(
        &cache.link_owners[1..3],
        &[caller.identity(), callee.identity()]
    );
}

#[test]
fn cross_page_graph_source_or_permission_changes_retire_only_the_changed_owner() {
    for changed in [0x1000, 0x2000] {
        for revoke_execute in [false, true] {
            let mut memory = call_memory();
            let mut cache = CoreCache32::default();
            let caller = insert(&mut cache, &memory, 0x1000, 1);
            let callee = insert(&mut cache, &memory, 0x2000, 2);
            assert!(matches!(
                cache
                    .prepare_owned_entry(&memory, 0x1006, options(), &[])
                    .unwrap(),
                OwnedEntryLookup::Ready(_)
            ));
            assert!(matches!(
                cache.lookup_transition(&memory, &caller, 0x2000),
                TransitionLookup::Linked(_, 2)
            ));
            assert!(matches!(
                cache.lookup_transition(&memory, &callee, 0x1006),
                TransitionLookup::Linked(_, 1)
            ));
            if revoke_execute {
                memory
                    .protect(
                        PageRange::new(GuestAddress(changed), 1).unwrap(),
                        Permissions::READ,
                    )
                    .unwrap();
            } else {
                memory.write(GuestAddress(changed), &[0x90]).unwrap();
            }
            let (invalid, current, pc, slot) = if changed == 0x1000 {
                (&caller, &callee, 0x1006, 1)
            } else {
                (&callee, &caller, 0x2000, 2)
            };
            assert!(!invalid.is_current(&memory));
            assert!(current.is_current(&memory));
            assert!(matches!(
                cache.lookup_transition(&memory, current, pc),
                TransitionLookup::Miss
            ));
            assert_eq!(cache.link_owners[slot], 0);
            assert_eq!(cache.snapshot()[5], 1);
            assert_eq!(cache.snapshot()[4], 1);
            assert!(matches!(
                cache.lookup_transition(&memory, invalid, current.entrypoints()[0].0),
                TransitionLookup::Stop
            ));
            assert_eq!(
                cache
                    .lookup(&memory, current.entrypoints()[0].0)
                    .unwrap()
                    .identity(),
                current.identity()
            );
        }
    }
}
