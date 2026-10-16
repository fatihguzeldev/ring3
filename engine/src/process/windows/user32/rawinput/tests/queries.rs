use super::*;
use crate::windows::user32::{User32Api32, User32Error32};
#[test]
fn buffered_read_excludes_removed_message_and_failed_output_preserves_queue() {
    let (mut context, mut memory, call, window) = fixture();
    register(&mut context, &mut memory, 6, 0x30, window);
    key(&mut context, 0x1e);
    key(&mut context, 0x30);
    context
        .prepare_arguments(
            User32Api32::PeekMessageW,
            &[0x10600, window, 0xff, 0xff, 1],
            &mut memory,
            call,
        )
        .unwrap();
    let removed = dword(&memory, 0x1060c);
    memory
        .write(GuestAddress(0x10400), &64u32.to_le_bytes())
        .unwrap();
    let version = memory.transaction_version();
    assert!(
        context
            .raw_input_call(
                Api::GetRawInputBuffer,
                &[0xfffffff0, 0x10400, 16],
                &mut memory,
                7
            )
            .is_err()
    );
    assert_eq!(memory.transaction_version(), version);
    assert_eq!(context.raw_input.queued(7).count(), 1);
    assert_eq!(
        context.raw_input_call(
            Api::GetRawInputBuffer,
            &[0x10500, 0x10400, 16],
            &mut memory,
            7
        ),
        Ok(1)
    );
    assert_eq!(dword(&memory, 0x10516) & 0xffff, 66);
    assert_eq!(context.state.queue_status(7).0 & 0x400, 0);
    assert!(context.raw_input.packet(removed, 7).is_ok());
}
#[test]
fn registration_queries_device_caps_and_default_proc_have_literal_contracts() {
    let (mut context, mut memory, _, window) = fixture();
    register(&mut context, &mut memory, 6, 0x30, window);
    assert_eq!(
        context.raw_input_call(
            Api::GetRegisteredRawInputDevices,
            &[0, 0x10400, 12],
            &mut memory,
            7
        ),
        Err(User32Error32::Win32(122))
    );
    assert_eq!(dword(&memory, 0x10400), 1);
    assert_eq!(
        context.raw_input_call(Api::GetRawInputDeviceList, &[0, 0x10400, 8], &mut memory, 7),
        Ok(0)
    );
    assert_eq!(dword(&memory, 0x10400), 2);
    memory
        .write(GuestAddress(0x10400), &32u32.to_le_bytes())
        .unwrap();
    memory
        .write(GuestAddress(0x10500), &32u32.to_le_bytes())
        .unwrap();
    assert_eq!(
        context.raw_input_call(
            Api::GetRawInputDeviceInfoW,
            &[
                crate::windows::rawinput::KEYBOARD,
                0x2000000b,
                0x10500,
                0x10400
            ],
            &mut memory,
            7
        ),
        Ok(32)
    );
    assert_eq!(dword(&memory, 0x10504), 1);
    assert_eq!(dword(&memory, 0x1051c), 101);
    assert_eq!(
        context.raw_input_call(
            Api::DefRawInputProc,
            &[0xffffffff, 0xffffffff, 16],
            &mut memory,
            7
        ),
        Ok(0)
    );
    assert_eq!(
        context.raw_input_call(Api::DefRawInputProc, &[0, 0, 24], &mut memory, 7),
        Err(User32Error32::Win32(87))
    );
}
