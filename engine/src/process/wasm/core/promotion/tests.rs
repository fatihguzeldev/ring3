use super::*;
use crate::{
    cpu::{core::compile, x86::State32},
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

fn memory() -> AddressSpace {
    let mut memory = AddressSpace::new(3).unwrap();
    for (pc, permissions) in [
        (0x1000, Permissions::ALL),
        (0x2000, Permissions::ALL),
        (0x3000, Permissions::READ_WRITE),
    ] {
        memory
            .map_zeroed(PageRange::new(GuestAddress(pc), 1).unwrap(), permissions)
            .unwrap();
    }
    memory
        .write(
            GuestAddress(0x1000),
            &[0xe8, 0x3b, 0, 0, 0, 0x49, 0x75, 0xf8, 0x0f, 0x0b],
        )
        .unwrap();
    memory
        .write(GuestAddress(0x1040), &[0xe8, 0x3b, 0, 0, 0, 0xc3])
        .unwrap();
    memory.write(GuestAddress(0x1080), &[0x40, 0xc3]).unwrap();
    memory.write(GuestAddress(0x2000), &[0x40, 0xc3]).unwrap();
    memory
}

fn insert(cache: &mut CoreCache32, memory: &AddressSpace, pc: u32, slot: u32) -> RegionRef {
    let code = cache.insert(compile(memory, GuestAddress(pc), options()).unwrap());
    cache.install_link(&code, slot).unwrap();
    code
}

fn mature(
    cache: &mut CoreCache32,
    memory: &AddressSpace,
    source: &RegionRef,
    target: &RegionRef,
    pc: u32,
) {
    assert!(!cache.observe_transition(memory, source, target, pc, 2));
    assert!(cache.observe_transition(memory, source, target, pc, 2));
}

#[test]
fn a_mature_observed_pair_is_formed_only_by_the_closed_prepare() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let a = insert(&mut cache, &memory, 0x1000, 1);
    let b = insert(&mut cache, &memory, 0x1040, 2);
    let before = cache.snapshot();
    mature(&mut cache, &memory, &a, &b, 0x1040);
    assert_eq!(cache.snapshot(), before);
    assert_eq!(&cache.link_owners[1..3], &[a.identity(), b.identity()]);
    let merged = cache
        .prepare_transition(&memory, 0x1040, options(), &[])
        .unwrap();
    assert_ne!(merged.identity(), a.identity());
    assert_ne!(merged.identity(), b.identity());
    assert_eq!(
        merged.formation_roots(),
        &[GuestAddress(0x1000), GuestAddress(0x1040)]
    );
    assert!(Rc::ptr_eq(
        &cache.lookup(&memory, 0x1005).unwrap().code,
        &merged.code
    ));
    assert!(Rc::ptr_eq(
        &cache.lookup(&memory, 0x1040).unwrap().code,
        &merged.code
    ));
    assert_eq!(&cache.link_owners[1..3], &[a.identity(), b.identity()]);
    assert!(
        cache
            .prepare_transition(&memory, 0x1040, options(), &[])
            .is_none()
    );
    assert_eq!(cache.snapshot()[0], before[0] + 1);
}

#[test]
fn a_return_into_an_interior_entry_keeps_all_previously_formed_roots() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let a = insert(&mut cache, &memory, 0x1000, 1);
    let b = insert(&mut cache, &memory, 0x1040, 2);
    let c = insert(&mut cache, &memory, 0x1080, 3);
    mature(&mut cache, &memory, &a, &b, 0x1040);
    let ab = cache
        .prepare_transition(&memory, 0x1040, options(), &[])
        .unwrap();
    cache.install_link(&ab, 4).unwrap();
    mature(&mut cache, &memory, &c, &ab, 0x1045);
    let abc = cache
        .prepare_transition(&memory, 0x1045, options(), &[])
        .unwrap();
    assert_eq!(
        abc.formation_roots(),
        &[
            GuestAddress(0x1000),
            GuestAddress(0x1040),
            GuestAddress(0x1045),
            GuestAddress(0x1080),
        ]
    );
    for pc in [0x1000, 0x1005, 0x1040, 0x1045, 0x1080] {
        assert!(abc.entrypoints().contains(&GuestAddress(pc)));
    }
    assert_eq!(
        cache.lookup(&memory, 0x1000).unwrap().identity(),
        abc.identity()
    );
    assert!(a.is_current(&memory) && b.is_current(&memory) && c.is_current(&memory));
}

#[test]
fn a_declined_cap_preserves_the_target_and_requires_new_heat() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let a = insert(&mut cache, &memory, 0x1000, 1);
    let b = insert(&mut cache, &memory, 0x1040, 2);
    mature(&mut cache, &memory, &a, &b, 0x1040);
    let before = cache.snapshot();
    assert!(
        cache
            .prepare_transition(
                &memory,
                0x1040,
                CompileOptions {
                    instructions: 1,
                    ..options()
                },
                &[]
            )
            .is_none()
    );
    assert_eq!(cache.snapshot(), before);
    assert_eq!(
        cache.lookup_linked(&memory, 0x1040).unwrap().0.identity(),
        b.identity()
    );
    assert!(
        cache
            .prepare_transition(&memory, 0x1040, options(), &[])
            .is_none()
    );
    mature(&mut cache, &memory, &a, &b, 0x1040);
    assert!(
        cache
            .prepare_transition(&memory, 0x1040, options(), &[])
            .is_some()
    );
}

#[test]
fn source_epoch_or_identity_changes_cannot_resurrect_pending_owners() {
    let mut memory = memory();
    let mut cache = CoreCache32::default();
    let a = insert(&mut cache, &memory, 0x1000, 1);
    let b = insert(&mut cache, &memory, 0x1040, 2);
    mature(&mut cache, &memory, &a, &b, 0x1040);
    memory.write(GuestAddress(0x2000), &[0x48]).unwrap();
    assert!(
        cache
            .prepare_transition(&memory, 0x1040, options(), &[])
            .is_none()
    );
    assert_eq!(
        cache.lookup(&memory, 0x1040).unwrap().identity(),
        b.identity()
    );
    mature(&mut cache, &memory, &a, &b, 0x1040);
    cache.remove(a.key);
    assert!(
        cache
            .prepare_transition(&memory, 0x1040, options(), &[])
            .is_none()
    );
    assert_eq!(
        cache.lookup(&memory, 0x1040).unwrap().identity(),
        b.identity()
    );
    let fresh_a = insert(&mut cache, &memory, 0x1000, 1);
    mature(&mut cache, &memory, &fresh_a, &b, 0x1040);
    memory.write(GuestAddress(0x1040), &[0x90]).unwrap();
    assert!(
        cache
            .prepare_transition(&memory, 0x1040, options(), &[])
            .is_none()
    );
    assert!(cache.lookup(&memory, 0x1040).is_none());
    assert!(!b.is_current(&memory));
}

#[test]
fn only_the_current_linked_indexed_pair_can_accumulate_heat() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let a = insert(&mut cache, &memory, 0x1000, 1);
    let b = insert(&mut cache, &memory, 0x1040, 2);
    let c = insert(&mut cache, &memory, 0x1080, 3);
    assert!(!cache.observe_transition(&memory, &a, &a, 0x1000, 1));
    assert!(!cache.observe_transition(&memory, &a, &b, 0x1041, 1));
    cache.unlink(2, b.identity()).unwrap();
    assert!(!cache.observe_transition(&memory, &a, &b, 0x1040, 1));
    cache.install_link(&b, 2).unwrap();
    assert!(!cache.observe_transition(&memory, &a, &b, 0x1040, 2));
    assert!(!cache.observe_transition(&memory, &a, &c, 0x1080, 2));
    assert!(!cache.observe_transition(&memory, &a, &b, 0x1040, 2));
    assert!(cache.observe_transition(&memory, &a, &b, 0x1040, 2));
    cache.clear_transition();
    assert!(
        cache
            .prepare_transition(&memory, 0x1040, options(), &[])
            .is_none()
    );
}

#[test]
fn forming_a_new_ordinal_table_does_not_reset_the_persistent_numerical_bank() {
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
    assert_eq!(a.native_instructions()[0].pc, 0x1000);
    assert_eq!(b.native_instructions()[0].pc, 0x1040);
    let mut bank = NumericBank::from_instance(&instance).unwrap();
    let mut context = crate::cpu::core::Context32::new(State32 {
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
    let context_before = *context.bytes();
    let memory = instance.memory.as_ref().unwrap();
    mature(&mut cache, memory, &a, &b, 0x1040);
    let merged = cache
        .prepare_transition(memory, 0x1040, options(), &[])
        .unwrap();
    assert_eq!(bank.architectural_state(), state);
    assert_eq!(instance.arena(), arena);
    assert_eq!(*context.bytes(), context_before);
    assert_eq!(
        merged
            .native_instructions()
            .iter()
            .map(|op| op.pc)
            .collect::<Vec<_>>(),
        [0x1000, 0x1040, 0x1043]
    );
    for operation in &merged.native_instructions()[1..] {
        assert_eq!(bank.execute(&mut instance, &mut context, operation), 0);
    }
    let final_state = bank.architectural_state().0;
    assert_eq!(final_state.tag, 0xffff);
    assert_eq!(
        final_state.registers[7],
        [0, 0, 0, 0, 0, 0, 0, 0xc0, 1, 0x40]
    );
    assert_eq!(
        (final_state.instruction_pointer, final_state.data_pointer),
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
}

fn cross_page_call_memory() -> AddressSpace {
    let mut memory = memory();
    memory
        .write(GuestAddress(0x1000), &[0xe8, 0xfb, 0x0f, 0, 0])
        .unwrap();
    memory
}

#[test]
fn cross_page_call_and_return_pairs_retain_both_roots_and_the_continuation() {
    for return_edge in [false, true] {
        let memory = cross_page_call_memory();
        let mut cache = CoreCache32::default();
        let caller = insert(&mut cache, &memory, 0x1000, 1);
        let callee = insert(&mut cache, &memory, 0x2000, 2);
        let before = cache.snapshot();
        let (source, target, pc) = if return_edge {
            (&callee, &caller, 0x1005)
        } else {
            (&caller, &callee, 0x2000)
        };
        mature(&mut cache, &memory, source, target, pc);
        assert_eq!(cache.snapshot(), before);
        let merged = cache
            .prepare_transition(&memory, pc, options(), &[])
            .unwrap();
        let expected_roots = if return_edge {
            vec![
                GuestAddress(0x1000),
                GuestAddress(0x1005),
                GuestAddress(0x2000),
            ]
        } else {
            vec![GuestAddress(0x1000), GuestAddress(0x2000)]
        };
        assert_eq!(merged.formation_roots(), expected_roots);
        assert_eq!(merged.instruction_count(), 6);
        for entry in [0x1000, 0x1005, 0x2000] {
            assert!(merged.entrypoints().contains(&GuestAddress(entry)));
            assert_eq!(
                cache.lookup(&memory, entry).unwrap().identity(),
                merged.identity()
            );
        }
        assert_ne!(merged.identity(), caller.identity());
        assert_ne!(merged.identity(), callee.identity());
        assert_eq!(
            &cache.link_owners[1..3],
            &[caller.identity(), callee.identity()]
        );
        assert!(caller.is_current(&memory) && callee.is_current(&memory));
        assert!(merged.is_current(&memory));
        assert_eq!(cache.snapshot()[0], before[0] + 1);
    }
}

#[test]
fn cross_page_formed_sources_reject_writes_and_permission_changes_on_either_page() {
    for changed_page in [0x1000, 0x2000] {
        for revoke_execute in [false, true] {
            let mut memory = cross_page_call_memory();
            let mut cache = CoreCache32::default();
            let caller = insert(&mut cache, &memory, 0x1000, 1);
            let callee = insert(&mut cache, &memory, 0x2000, 2);
            mature(&mut cache, &memory, &caller, &callee, 0x2000);
            let merged = cache
                .prepare_transition(&memory, 0x2000, options(), &[])
                .unwrap();
            assert!(merged.is_current(&memory));
            if revoke_execute {
                memory
                    .protect(
                        PageRange::new(GuestAddress(changed_page), 1).unwrap(),
                        Permissions::READ,
                    )
                    .unwrap();
            } else {
                memory.write(GuestAddress(changed_page), &[0x90]).unwrap();
            }
            assert!(!merged.is_current(&memory));
            assert_eq!(caller.is_current(&memory), changed_page != 0x1000);
            assert_eq!(callee.is_current(&memory), changed_page != 0x2000);
            assert!(cache.lookup(&memory, 0x2000).is_none());
            assert_eq!(
                &cache.link_owners[1..3],
                &[caller.identity(), callee.identity()]
            );
        }
    }
}

#[test]
fn cross_page_shortening_declines_without_admission_and_requires_fresh_heat() {
    let memory = cross_page_call_memory();
    let mut cache = CoreCache32::default();
    let caller = insert(&mut cache, &memory, 0x1000, 1);
    let callee = insert(&mut cache, &memory, 0x2000, 2);
    mature(&mut cache, &memory, &caller, &callee, 0x2000);
    let before = cache.snapshot();
    assert!(
        cache
            .prepare_transition(
                &memory,
                0x2000,
                CompileOptions {
                    instructions: 1,
                    ..options()
                },
                &[],
            )
            .is_none()
    );
    assert_eq!(cache.snapshot(), before);
    assert_eq!(
        cache.lookup_linked(&memory, 0x2000).unwrap().0.identity(),
        callee.identity()
    );
    assert_eq!(
        &cache.link_owners[1..3],
        &[caller.identity(), callee.identity()]
    );
    assert!(
        cache
            .prepare_transition(&memory, 0x2000, options(), &[])
            .is_none()
    );
    mature(&mut cache, &memory, &caller, &callee, 0x2000);
    assert!(
        cache
            .prepare_transition(&memory, 0x2000, options(), &[])
            .is_some()
    );
}
