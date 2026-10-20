use super::support::*;
use crate::{
    abi::{arena::*, x86::*},
    cpu::x86::{X87State, simd::SimdState, simd::ymm::YmmUpperState},
    graphics::d3d8::api::{HResult, InterfaceKind, MethodGate},
    windows::thread::ThreadSchedule32,
};

fn seed(fixture: &mut Fixture, control: u16, status: u16) -> [Vec<u8>; 3] {
    let x87 = X87State {
        control,
        status,
        tag: 0x1357,
        opcode: 0x321,
        instruction_pointer: 0x1234_5678,
        data_pointer: 0xabcd_ef00,
        code_selector: 0x1b,
        data_selector: 0x23,
        registers: std::array::from_fn(|index| [index as u8 + 1; 10]),
    };
    let simd = SimdState {
        mxcsr: 0x5f81,
        xmm: std::array::from_fn(|index| [index as u8 + 0x31; 16]),
    };
    let upper = YmmUpperState {
        registers: std::array::from_fn(|index| [index as u8 + 0x71; 16]),
    };
    let arena = fixture.engine.arena_mut().unwrap();
    encode_x87(&x87, &mut arena[X87_OFFSET..X87_OFFSET + X87_SIZE]).unwrap();
    encode_simd(&simd, &mut arena[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE]).unwrap();
    encode_ymm(&upper, &mut arena[YMM_OFFSET..YMM_OFFSET + YMM_SIZE]).unwrap();
    banks(fixture)
}

fn banks(fixture: &Fixture) -> [Vec<u8>; 3] {
    let arena = fixture.engine.arena();
    [
        arena[X87_OFFSET..X87_OFFSET + X87_SIZE].to_vec(),
        arena[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE].to_vec(),
        arena[YMM_OFFSET..YMM_OFFSET + YMM_SIZE].to_vec(),
    ]
}

fn expected(mut original: [Vec<u8>; 3], control: u16, status: u16) -> [Vec<u8>; 3] {
    original[0][16..18].copy_from_slice(&control.to_le_bytes());
    original[0][18..20].copy_from_slice(&status.to_le_bytes());
    original
}

fn create(fixture: &mut Fixture, adapter: u32, behavior: u32, windowed: bool) {
    let window = fixture.window;
    fixture.parameters(window, 640, 480, windowed);
    if windowed {
        write(&mut fixture.engine, PP + 44, &[0, 0]);
    }
    fixture.call(
        MethodGate {
            interface: InterfaceKind::Direct3D,
            slot: 15,
        }
        .id(),
        &[fixture.parent.0, adapter, 1, window, behavior, PP, OUT],
    );
}

#[test]
fn create_device_sets_single_nearest_and_masks_exceptions_from_default_control() {
    let mut fixture = Fixture::new(0x9000_0000);
    let before = seed(&mut fixture, 0x037f, 0);
    create(&mut fixture, 0, 0x80, true);
    fixture.assert_return(7);
    assert_eq!(banks(&fixture), expected(before, 0x007f, 0));
}

#[test]
fn setup_preserves_other_control_bits_sticky_flags_stack_and_simd() {
    let mut fixture = Fixture::new(0x9000_0000);
    let before = seed(&mut fixture, 0x1f40, 0xb8e5);
    create(&mut fixture, 0, 0x80, true);
    fixture.assert_return(7);
    assert_eq!(banks(&fixture), expected(before, 0x107f, 0x3865));
}

#[test]
fn preserve_flag_leaves_every_numerical_byte_unchanged() {
    let mut fixture = Fixture::new(0x9000_0000);
    let before = seed(&mut fixture, 0x1f40, 0xb8e5);
    create(&mut fixture, 0, 0x82, true);
    fixture.assert_return(7);
    assert_eq!(banks(&fixture), before);
}

#[test]
fn early_adapter_rejection_does_not_reach_fpu_setup() {
    let mut fixture = Fixture::new(0x9000_0000);
    let before = seed(&mut fixture, 0x1f40, 0xb8e5);
    let devices = fixture.engine.d3d8.as_ref().unwrap().device_slots.len();
    create(&mut fixture, 1, 0x80, true);
    assert_eq!(state(&fixture.engine).registers[0], HResult::INVALID_CALL.0);
    assert_eq!(banks(&fixture), before);
    assert_eq!(
        fixture.engine.d3d8.as_ref().unwrap().device_slots.len(),
        devices
    );
    assert!(fixture.engine.pending_call.is_none());
}

#[test]
fn admitted_backend_failure_retains_setup_without_publishing_a_device() {
    let mut fixture = Fixture::new(0x9000_0000);
    let before = seed(&mut fixture, 0x1f40, 0xb8e5);
    let runtime = fixture.engine.d3d8.as_mut().unwrap();
    let devices = runtime.device_slots.len();
    runtime.config.device.graphics.resource_bytes = 1024;
    create(&mut fixture, 0, 0x80, true);
    assert_eq!(
        state(&fixture.engine).registers[0],
        HResult::OUT_OF_VIDEO_MEMORY.0
    );
    assert_eq!(banks(&fixture), expected(before, 0x107f, 0x3865));
    assert_eq!(
        fixture.engine.d3d8.as_ref().unwrap().device_slots.len(),
        devices
    );
    assert_eq!(words(&fixture.engine, OUT, 1), [0]);
    assert!(fixture.engine.pending_call.is_none());
}

#[test]
fn fullscreen_callback_owner_and_restored_outer_keep_the_setup_image() {
    let mut fixture = Fixture::new(0x8000_0000);
    let before = seed(&mut fixture, 0x1f40, 0xb8e5);
    create(&mut fixture, 0, 0x80, false);
    let wanted = expected(before, 0x107f, 0x3865);
    assert_eq!(banks(&fixture), wanted);
    assert_eq!(
        fixture
            .engine
            .pending_call
            .as_ref()
            .unwrap()
            .extended
            .x87
            .as_slice(),
        wanted[0]
    );
    let mut callbacks = 0;
    fixture.drain(|engine, _| {
        assert_eq!(
            &engine.arena()[X87_OFFSET..X87_OFFSET + X87_SIZE],
            wanted[0]
        );
        callbacks += 1;
    });
    assert_eq!(callbacks, 3);
    fixture.assert_return(7);
    assert_eq!(banks(&fixture), wanted);
}

#[test]
fn later_callback_abort_restores_the_post_setup_thread_bank() {
    let mut fixture = Fixture::new(0x9000_0000);
    let before = seed(&mut fixture, 0x1f40, 0xb8e5);
    create(&mut fixture, 0, 0x80, true);
    fixture.assert_return(7);
    let wanted = expected(before, 0x107f, 0x3865);
    let (owner, outer) = fixture.capture(0x1234, &[]);
    let callback = fixture
        .engine
        .begin_resident_callback(
            KEY,
            owner,
            fixture.callback,
            outer,
            0x9010,
            0x9020,
            0x200ff,
            &[],
        )
        .unwrap();
    fixture.engine.arena_mut().unwrap()[X87_OFFSET + 16..X87_OFFSET + 18]
        .copy_from_slice(&0x0b7f_u16.to_le_bytes());
    fixture.engine.abort_callback(KEY, callback.token).unwrap();
    assert_eq!(banks(&fixture), wanted);
}

#[test]
fn setup_changes_the_calling_thread_without_replacing_another_saved_bank() {
    let mut fixture = Fixture::new(0x9000_0000);
    fixture
        .engine
        .configure_synchronization(Default::default())
        .unwrap();
    fixture
        .engine
        .create_test_thread_at_gate(0x9010, 0x9020)
        .unwrap();
    assert_eq!(
        fixture.engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 2 }
    );
    let child = seed(&mut fixture, 0x027f, 0x0820);
    assert_eq!(
        fixture.engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 1 }
    );
    let parent = seed(&mut fixture, 0x1f40, 0xb8e5);
    create(&mut fixture, 0, 0x80, true);
    fixture.assert_return(7);
    let parent = expected(parent, 0x107f, 0x3865);
    assert_eq!(banks(&fixture), parent);
    assert_eq!(
        fixture.engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 2 }
    );
    assert_eq!(banks(&fixture), child);
    assert_eq!(
        fixture.engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 1 }
    );
    assert_eq!(banks(&fixture), parent);
}
