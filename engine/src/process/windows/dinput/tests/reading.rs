use super::support::*;
use crate::{
    input::{
        InputKind, TimedInputEvent,
        dinput::{DirectInputResult32, InterfaceKind32},
    },
    memory::{GuestAddress, PageRange, Permissions},
    process::windows::dinput::memory::words,
};
#[test]
fn inaccessible_count_output_preserves_data_and_consumer_cursor() {
    let (mut runtime, mut memory, pointer, mut source) = runtime(true);
    keyboard_format(&mut runtime, &mut memory, pointer);
    runtime
        .device_mut(pointer)
        .unwrap()
        .native
        .acquire(&source, 0)
        .unwrap();
    source
        .publish_physical(
            [TimedInputEvent {
                timestamp_ns: 2_000_000,
                kind: InputKind::Key {
                    code: 30,
                    pressed: true,
                },
            }]
            .into_iter(),
        )
        .unwrap();
    runtime
        .device_mut(pointer)
        .unwrap()
        .native
        .synchronize(&source, 0, 0);
    memory
        .write(GuestAddress(DATA + 512), &1u32.to_le_bytes())
        .unwrap();
    memory.write(GuestAddress(DATA + 768), &[0xaa; 20]).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x5000), 1).unwrap(),
            Permissions::READ,
        )
        .unwrap();
    let before = runtime.device(pointer).unwrap().native.buffered().unwrap();
    let version = memory.transaction_version();
    assert!(
        runtime
            .get_data(
                &mut memory,
                pointer,
                &[pointer.0, 20, DATA + 768, 0x5000, 0],
                InterfaceKind32::Device8A
            )
            .is_err()
    );
    assert_eq!(
        runtime.device(pointer).unwrap().native.buffered().unwrap(),
        before
    );
    assert_eq!(memory.transaction_version(), version);
    let mut bytes = [0; 20];
    memory.read(GuestAddress(DATA + 768), &mut bytes).unwrap();
    assert_eq!(bytes, [0xaa; 20]);
    assert_eq!(
        runtime
            .get_data(
                &mut memory,
                pointer,
                &[pointer.0, 20, DATA + 768, DATA + 512, 1],
                InterfaceKind32::Device8A
            )
            .unwrap(),
        0
    );
    assert_eq!(
        words::<5>(&memory, DATA + 768).unwrap()[..4],
        [3, 0x80, 2, 2]
    );
    assert_eq!(
        runtime.device(pointer).unwrap().native.buffered().unwrap(),
        1
    );
    runtime
        .get_data(
            &mut memory,
            pointer,
            &[pointer.0, 20, 0, DATA + 512, 0],
            InterfaceKind32::Device8A,
        )
        .unwrap();
    assert_eq!(
        runtime.device(pointer).unwrap().native.buffered().unwrap(),
        0
    );
}
#[test]
fn immediate_state_failure_does_not_consume_buffered_events() {
    let (mut runtime, mut memory, pointer, source) = runtime(true);
    keyboard_format(&mut runtime, &mut memory, pointer);
    runtime
        .device_mut(pointer)
        .unwrap()
        .native
        .acquire(&source, 0)
        .unwrap();
    assert_eq!(
        runtime.get_state(&mut memory, pointer, 7, DATA + 512),
        Err(DirectInputResult32::INVALID_PARAM)
    );
    assert_eq!(
        runtime
            .device(pointer)
            .unwrap()
            .native
            .state_size()
            .unwrap(),
        8
    );
}

#[test]
fn warm_immediate_and_buffered_reads_reuse_owned_transfer_storage() {
    let (mut runtime, mut memory, pointer, mut source) = runtime(true);
    keyboard_format(&mut runtime, &mut memory, pointer);
    runtime
        .device_mut(pointer)
        .unwrap()
        .native
        .acquire(&source, 0)
        .unwrap();
    source
        .publish_physical(
            [TimedInputEvent {
                timestamp_ns: 2_000_000,
                kind: InputKind::Key {
                    code: 30,
                    pressed: true,
                },
            }]
            .into_iter(),
        )
        .unwrap();
    runtime
        .device_mut(pointer)
        .unwrap()
        .native
        .synchronize(&source, 0, 0);
    memory
        .write(GuestAddress(DATA + 512), &1u32.to_le_bytes())
        .unwrap();
    runtime
        .get_data(
            &mut memory,
            pointer,
            &[pointer.0, 20, DATA + 768, DATA + 512, 1],
            InterfaceKind32::Device8A,
        )
        .unwrap();
    let allocation = runtime.device(pointer).unwrap().transfer.as_ptr();
    let capacity = runtime.device(pointer).unwrap().transfer.capacity();
    for _ in 0..16 {
        runtime
            .get_state(&mut memory, pointer, 8, DATA + 900)
            .unwrap();
        runtime
            .get_data(
                &mut memory,
                pointer,
                &[pointer.0, 20, DATA + 768, DATA + 512, 1],
                InterfaceKind32::Device8A,
            )
            .unwrap();
        assert_eq!(
            runtime.device(pointer).unwrap().transfer.as_ptr(),
            allocation
        );
        assert_eq!(
            runtime.device(pointer).unwrap().transfer.capacity(),
            capacity
        );
    }
}
