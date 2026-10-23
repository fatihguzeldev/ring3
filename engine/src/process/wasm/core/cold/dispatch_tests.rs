use super::*;
use crate::{
    cpu::{
        core::{CompileOptions, compile},
        x86::State32,
    },
    memory::GuestAddress,
};

const COLD: u32 = 0x4000_0000;
const STOP: u32 = 0x8000_0000;

struct Fixture {
    instance: EngineInstance,
    context: Context32,
    numeric: NumericBank,
    selected: Option<RegionRef>,
    target: RegionRef,
    cache: CoreCache32,
    cold: ColdState,
}

fn fixture(bytes: &[u8], budget: u32) -> Fixture {
    let mut instance = EngineInstance::new(4, 7).unwrap();
    instance.map(0x1000, 1, 7).unwrap();
    instance.map(0x3000, 1, 3).unwrap();
    let memory = instance.memory.as_mut().unwrap();
    memory.write(GuestAddress(0x1000), &[0xff, 0xe6]).unwrap();
    memory.write(GuestAddress(0x1020), bytes).unwrap();
    memory
        .write(GuestAddress(0x1100), &[0xd8, 0x4b, 4, 0xd9, 0x5b, 8])
        .unwrap();
    memory
        .write(GuestAddress(0x3000), &3_f32.to_le_bytes())
        .unwrap();
    memory
        .write(GuestAddress(0x3004), &2_f32.to_le_bytes())
        .unwrap();
    let mut cache = CoreCache32::default();
    let options = |instructions| CompileOptions {
        instructions,
        blocks: 2,
        ..CompileOptions::default()
    };
    let source = cache.insert(compile(memory, GuestAddress(0x1000), options(1)).unwrap());
    let target = cache.insert(compile(memory, GuestAddress(0x1100), options(2)).unwrap());
    cache.install_link(&source, 1).unwrap();
    cache.install_link(&target, 2).unwrap();
    let numeric = NumericBank::from_instance(&instance).unwrap();
    let mut context = Context32::new(State32 {
        registers: [7, 0, 0, 0x3000, 0x3800, 0, 0x1020, 0x1100],
        eip: 0x1020,
        eflags: 2,
    });
    context.set_word(40, 10);
    context.set_word(44, budget);
    context.set_word(112, 1);
    context.bytes_mut()[120..128].copy_from_slice(&source.identity().to_le_bytes());
    Fixture {
        instance,
        context,
        numeric,
        selected: Some(source),
        target,
        cache,
        cold: ColdState::default(),
    }
}

fn next(f: &mut Fixture) -> u32 {
    chain_next(
        &mut f.instance,
        &mut f.context,
        &mut f.numeric,
        &mut f.selected,
        &mut f.cache,
        &mut f.cold,
    )
}

#[test]
fn active_chain_interprets_a_prefix_then_selects_the_real_linked_numeric_owner() {
    for linked in [false, true] {
        let mut f = fixture(&[0xd9, 3, 0x40, 0xff, 0xe7], 5);
        if !linked {
            f.cache.unlink(2, f.target.identity()).unwrap();
        }
        let arena = f.instance.arena().to_vec();
        let mut expected = f.context.clone();
        let mut state = expected.state();
        state.registers[0] = 8;
        state.eip = 0x1100;
        expected.set_state(state);
        expected.set_word(40, 13);
        expected.set_word(44, 2);
        expected.bytes_mut()[120..128]
            .copy_from_slice(&if linked { f.target.identity() } else { 0 }.to_le_bytes());
        assert_eq!(next(&mut f), COLD | if linked { 2 } else { 0 });
        assert_eq!(f.context.bytes(), expected.bytes());
        assert_eq!(f.instance.arena(), arena);
        assert_eq!(
            f.numeric.architectural_state().0.registers[7],
            [0, 0, 0, 0, 0, 0, 0, 0xc0, 0, 0x40]
        );
        assert_eq!(
            f.selected.as_ref().map(|code| code.identity()),
            linked.then(|| f.target.identity())
        );
        if linked {
            let instructions = f.selected.as_ref().unwrap().native_instructions();
            assert_eq!(instructions[0].pc, 0x1100);
            for instruction in instructions {
                assert_eq!(
                    f.numeric
                        .execute(&mut f.instance, &mut f.context, instruction),
                    0
                );
            }
            let mut value = [0; 4];
            f.instance
                .memory
                .as_ref()
                .unwrap()
                .read(GuestAddress(0x3008), &mut value)
                .unwrap();
            assert_eq!(value, 6_f32.to_le_bytes());
            assert_eq!(f.numeric.architectural_state().0.tag, 0xffff);
            assert_eq!(f.instance.arena(), arena);
        }
    }
}

#[test]
fn active_cold_terminal_results_keep_identity_zero_and_the_exact_committed_prefix() {
    for (bytes, budget, destination, reason, retired, remaining, pc, eax) in [
        (
            &[0x40, 0xff, 0xe7][..],
            1,
            0x1100,
            StopReason::Budget,
            11,
            0,
            0x1021,
            8,
        ),
        (
            &[0xf3, 0xa4][..],
            5,
            0x1100,
            StopReason::NeedCode,
            10,
            5,
            0x1020,
            7,
        ),
        (
            &[0x40, 0x8b, 7][..],
            5,
            0x5000,
            StopReason::MemoryFault,
            11,
            4,
            0x1021,
            8,
        ),
        (
            &[0x40, 0xc6, 7, 0x90][..],
            5,
            0x1080,
            StopReason::CodeInvalidated,
            12,
            3,
            0x1024,
            8,
        ),
        (
            &[0xcd, 0x80][..],
            5,
            0x1100,
            StopReason::SystemEntry,
            11,
            4,
            0x1022,
            7,
        ),
    ] {
        let mut f = fixture(bytes, budget);
        let mut state = f.context.state();
        state.registers[7] = destination;
        f.context.set_state(state);
        let numeric = f.numeric.architectural_state();
        let arena = f.instance.arena().to_vec();
        let mut expected = f.context.clone();
        let mut state = expected.state();
        state.eip = pc;
        state.registers[0] = eax;
        expected.set_state(state);
        expected.set_word(40, retired);
        expected.set_word(44, remaining);
        expected.bytes_mut()[120..128].fill(0);
        if reason == StopReason::MemoryFault {
            expected.set_word(48, 0x5000);
            expected.set_word(52, 0x10104);
            expected.set_word(116, 0x40e);
        }
        if reason == StopReason::SystemEntry {
            expected.set_word(48, 0x1020);
            expected.set_word(116, 0x80);
        }
        assert_eq!(
            next(&mut f),
            COLD | if reason == StopReason::NeedCode {
                0
            } else {
                STOP | reason as u32
            }
        );
        assert_eq!(f.context.bytes(), expected.bytes());
        assert!(f.selected.is_none());
        assert_eq!(f.numeric.architectural_state(), numeric);
        assert_eq!(f.instance.arena(), arena);
        if reason == StopReason::CodeInvalidated {
            let mut value = [0];
            f.instance
                .memory
                .as_ref()
                .unwrap()
                .read(GuestAddress(0x1080), &mut value)
                .unwrap();
            assert_eq!(value, [0x90]);
        }
    }
}

#[test]
fn active_chain_lookup_and_lease_rejections_do_not_execute_a_cold_prefix() {
    for rejection in [0, 1, 2] {
        let mut f = fixture(&[0x40, 0xff, 0xe7], 5);
        match rejection {
            0 => f.context.set_word(112, 0),
            1 => f.context.bytes_mut()[120..128].fill(0),
            2 => f
                .instance
                .memory
                .as_mut()
                .unwrap()
                .write(GuestAddress(0x1000), &[0x90])
                .unwrap(),
            _ => unreachable!(),
        }
        let before = *f.context.bytes();
        let numeric = f.numeric.architectural_state();
        let arena = f.instance.arena().to_vec();
        let identity = f.selected.as_ref().unwrap().identity();
        assert_eq!(next(&mut f), if rejection < 2 { u32::MAX } else { 0 });
        assert_eq!(*f.context.bytes(), before);
        assert_eq!(f.numeric.architectural_state(), numeric);
        assert_eq!(f.instance.arena(), arena);
        assert_eq!(f.selected.as_ref().unwrap().identity(), identity);
    }
    let mut f = fixture(&[0x40, 0xff, 0xe7], 5);
    f.context.set_word(32, 0x1100);
    let mut expected = *f.context.bytes();
    expected[120..128].copy_from_slice(&f.target.identity().to_le_bytes());
    assert_eq!(next(&mut f), 2);
    assert_eq!(*f.context.bytes(), expected);
    assert_eq!(f.cold.diagnostics_snapshot()[0..5], [1, 0, 0, 0, 0]);
}
