#[path = "support/dinput_acquire_cases.rs"]
#[allow(dead_code)]
mod dinput_acquire_cases;
#[path = "support/dinput_cooperative_cases.rs"]
#[allow(dead_code)]
mod dinput_cooperative_cases;
#[path = "support/dinput_format_cases.rs"]
#[allow(dead_code)]
mod dinput_format_cases;
#[path = "support/dinput_state_cases.rs"]
mod dinput_state_cases;
#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/window_creation_executable.rs"]
mod window_creation_executable;

#[test]
fn imported_keyboard_snapshots_match_across_budgets() {
    dinput_state_cases::keyboard_snapshots_across_budgets();
}

use dinput_acquire_cases::{DATA, WINDOW};
use dinput_format_cases::FORMAT;
use dinput_state_cases::{API, CODE, DEVICE, OUTPUT, STACK, call, prepare, process, words};
use ring3_core::execution::{
    Access, KeyboardInputError, MemoryError, Permissions, Process32, ProcessStop, Register32,
    StopReason,
};

const ACQUIRE: u32 = 0x7000_0584;
const UNACQUIRE: u32 = 0x7000_0588;

fn acquired() -> Process32 {
    let mut p = process(true);
    assert_eq!(call(&mut p, ACQUIRE, &[DEVICE]), 0);
    p
}

fn output(p: &Process32) -> [u8; 256] {
    let mut bytes = [0; 256];
    p.memory.read(u64::from(OUTPUT), &mut bytes).unwrap();
    bytes
}

fn refused(p: &mut Process32, reason: &ProcessStop) {
    let before = p.cpu;
    for _ in 0..2 {
        let result = p.run(1);
        assert_eq!(&result.reason, reason);
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
    }
}

#[test]
fn host_snapshot_is_owned_bounded_and_does_not_execute_or_modify_guest_state() {
    let mut p = acquired();
    assert_eq!(call(&mut p, API, &[DEVICE, 256, OUTPUT]), 0);
    assert_eq!(output(&p), [0; 256]);
    let cpu = p.cpu;
    let pages = p.memory.mapped_pages();
    let windows = p.window_snapshots();
    let mut keys = [true; 256];
    p.set_keyboard_state(keys).unwrap();
    keys.fill(false);
    assert_eq!(p.cpu, cpu);
    assert_eq!(p.memory.mapped_pages(), pages);
    assert_eq!(p.window_snapshots(), windows);
    assert_eq!(output(&p), [0; 256]);
    assert_eq!(call(&mut p, API, &[DEVICE, 256, OUTPUT]), 0);
    assert_eq!(output(&p), [0x80; 256]);
    p.set_keyboard_state(keys).unwrap();
    assert_eq!(call(&mut p, ACQUIRE, &[DEVICE]), 1);
    assert_eq!(call(&mut p, API, &[DEVICE, 256, OUTPUT]), 0);
    assert_eq!(output(&p), [0; 256]);
    prepare(&mut p, 0x7000_0008, &[42]);
    assert_eq!(p.run(1).reason, ProcessStop::Exited(42));
    let cpu = p.cpu;
    assert_eq!(
        p.set_keyboard_state([true; 256]),
        Err(KeyboardInputError::Exited)
    );
    assert_eq!(p.cpu, cpu);
    assert_eq!(output(&p), [0; 256]);
}

#[test]
fn scalar_errors_precede_acquisition_and_do_not_access_output_or_teb() {
    let mut p = process(false);
    words(&mut p, 0x7ffd_e034, &[77]);
    words(&mut p, 0x7000_2020, &[88]);
    p.memory
        .protect(0x7000_2000, 4096, Permissions::READ)
        .unwrap();
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::NONE)
        .unwrap();
    for size in [0, 255, 257, u32::MAX] {
        assert_eq!(call(&mut p, API, &[DEVICE, size, 0x5000_0000]), 0x8007_0057);
    }
    assert_eq!(call(&mut p, API, &[DEVICE, 256, 0]), 0x8007_0057);
    assert_eq!(call(&mut p, API, &[DEVICE, 256, 0x5000_0000]), 0x8007_000c);
    assert_eq!(call(&mut p, 0x7000_0578, &[DEVICE, FORMAT]), 0);
    assert_eq!(call(&mut p, 0x7000_057c, &[DEVICE, WINDOW, 0x16]), 0);
    assert_eq!(call(&mut p, ACQUIRE, &[DEVICE]), 0);
    p.set_keyboard_state([true; 256]).unwrap();
    assert_eq!(call(&mut p, API, &[DEVICE, 256, OUTPUT]), 0);
    assert_eq!(output(&p), [0x80; 256]);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(p.last_error().unwrap(), 77);
    let mut errno = [0; 4];
    p.memory.read(0x7000_2020, &mut errno).unwrap();
    assert_eq!(u32::from_le_bytes(errno), 88);
}

#[test]
fn guest_focus_loss_requires_reacquire_and_host_release_does_not_change_acquisition() {
    let mut p = acquired();
    p.set_keyboard_state([true; 256]).unwrap();
    assert_eq!(call(&mut p, 0x7000_0440, &[WINDOW, 0]), 1);
    for _ in 0..2 {
        assert_eq!(call(&mut p, API, &[DEVICE, 256, 0x5000_0000]), 0x8007_001e);
    }
    assert_eq!(call(&mut p, API, &[DEVICE, 255, OUTPUT]), 0x8007_0057);
    assert_eq!(call(&mut p, API, &[DEVICE, 256, 0]), 0x8007_0057);
    assert_eq!(call(&mut p, 0x7000_0440, &[WINDOW, 5]), 0);
    assert_eq!(call(&mut p, API, &[DEVICE, 256, OUTPUT]), 0x8007_001e);
    assert_eq!(output(&p), [0; 256]);
    assert_eq!(call(&mut p, ACQUIRE, &[DEVICE]), 0);
    assert_eq!(call(&mut p, API, &[DEVICE, 256, OUTPUT]), 0);
    assert_eq!(output(&p), [0x80; 256]);
    p.set_keyboard_state([false; 256]).unwrap();
    assert_eq!(call(&mut p, ACQUIRE, &[DEVICE]), 1);
    assert_eq!(call(&mut p, API, &[DEVICE, 256, OUTPUT]), 0);
    assert_eq!(output(&p), [0; 256]);
    assert_eq!(call(&mut p, UNACQUIRE, &[DEVICE]), 0);
    assert_eq!(call(&mut p, 0x7000_0440, &[WINDOW, 0]), 1);
    assert_eq!(call(&mut p, 0x7000_0440, &[WINDOW, 5]), 0);
    assert_eq!(call(&mut p, API, &[DEVICE, 256, OUTPUT]), 0x8007_000c);
    assert_eq!(call(&mut p, ACQUIRE, &[DEVICE]), 0);
}

#[test]
fn multiple_devices_and_recreated_devices_share_current_physical_snapshot() {
    let mut p = acquired();
    let mut keys = [false; 256];
    keys[0] = true;
    keys[255] = true;
    p.set_keyboard_state(keys).unwrap();
    for device in [DEVICE + 4, DEVICE + 8] {
        assert_eq!(
            call(&mut p, 0x7000_0568, &[0x7001_7800, DATA + 64, DATA, 0]),
            0
        );
        assert_eq!(call(&mut p, 0x7000_0578, &[device, FORMAT]), 0);
        assert_eq!(call(&mut p, 0x7000_057c, &[device, WINDOW, 6]), 0);
        assert_eq!(call(&mut p, ACQUIRE, &[device]), 0);
        assert_eq!(call(&mut p, API, &[device, 256, OUTPUT]), 0);
        assert_eq!(output(&p), keys.map(|key| u8::from(key) << 7));
        if device == DEVICE + 4 {
            assert_eq!(call(&mut p, API, &[DEVICE, 256, OUTPUT]), 0);
            assert_eq!(output(&p), keys.map(|key| u8::from(key) << 7));
            assert_eq!(call(&mut p, 0x7000_0574, &[DEVICE]), 0);
            assert_eq!(call(&mut p, 0x7000_0574, &[device]), 0);
        }
    }
    for receiver in [
        0,
        DEVICE,
        DEVICE + 4,
        DEVICE + 1,
        DEVICE + 12,
        0x7001_7800,
        0x7001_7a00,
        u32::MAX,
    ] {
        prepare(&mut p, API, &[receiver, 0, 0]);
        refused(&mut p, &ProcessStop::UnsupportedApi { address: API });
    }
    assert_eq!(call(&mut p, 0x7000_0564, &[0x7001_7800]), 0);
    assert_eq!(call(&mut p, API, &[DEVICE + 8, 256, OUTPUT]), 0);
}

#[test]
fn entire_output_is_checked_before_any_copy_and_faults_keep_snapshot() {
    let mut p = acquired();
    p.set_keyboard_state([true; 256]).unwrap();
    p.memory.write(u64::from(OUTPUT), &[0x55; 256]).unwrap();
    p.memory
        .protect(0x3100_0000, 4096, Permissions::READ)
        .unwrap();
    prepare(&mut p, API, &[DEVICE, 256, OUTPUT]);
    refused(
        &mut p,
        &ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::PermissionDenied {
            address: u64::from(OUTPUT),
            access: Access::Write,
        })),
    );
    assert_eq!(output(&p), [0x55; 256]);
    p.memory
        .protect(0x3100_0000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    for (address, error) in [
        (
            0x3100_0f81,
            MemoryError::Unmapped {
                address: 0x3100_1000,
            },
        ),
        (0xffff_ff81, MemoryError::AddressOverflow),
    ] {
        p.memory.write(u64::from(address), &[0x55; 127]).unwrap();
        prepare(&mut p, API, &[DEVICE, 256, address]);
        refused(
            &mut p,
            &ProcessStop::Stopped(StopReason::MemoryFault(error)),
        );
        let mut retained = [0; 127];
        p.memory.read(u64::from(address), &mut retained).unwrap();
        assert_eq!(retained, [0x55; 127]);
    }
    p.memory
        .map_zeroed(0x3100_1000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(call(&mut p, API, &[DEVICE, 256, 0x3100_0f81]), 0);
    let mut copied = [0; 256];
    p.memory.read(0x3100_0f81, &mut copied).unwrap();
    assert_eq!(copied, [0x80; 256]);
    p.memory
        .protect(
            0x3100_0000,
            4096,
            Permissions {
                write: true,
                ..Permissions::NONE
            },
        )
        .unwrap();
    assert_eq!(call(&mut p, API, &[DEVICE, 256, OUTPUT]), 0);
    p.memory
        .protect(0x3100_0000, 4096, Permissions::READ)
        .unwrap();
    assert_eq!(output(&p), [0x80; 256]);
}

#[test]
fn output_aliases_use_captured_arguments_and_reread_the_return_slot() {
    let mut p = acquired();
    let mut keys = [false; 256];
    keys[3] = true;
    p.set_keyboard_state(keys).unwrap();
    assert_eq!(call(&mut p, API, &[DEVICE, 256, STACK]), 0);
    assert_eq!(p.cpu.eip, 0x8000_0000);
    assert_eq!(call(&mut p, API, &[DEVICE, 256, 0x7ffd_e000]), 0);
    assert_eq!(p.last_error().unwrap(), 0);
    assert_eq!(call(&mut p, API, &[DEVICE, 256, OUTPUT]), 0);
    assert_eq!(output(&p), keys.map(|key| u8::from(key) << 7));
}

#[test]
fn actor_and_complete_stack_validation_precede_output_mutation() {
    let mut p = acquired();
    p.set_keyboard_state([true; 256]).unwrap();
    for fs in [0, 0x1101_0000, 0x1234_0000] {
        p.cpu.set_fs_base(fs);
        p.cpu.eip = API;
        p.cpu.set_register(Register32::Esp, 0x5000_0000);
        refused(&mut p, &ProcessStop::UnsupportedApi { address: API });
    }
    p.cpu.set_fs_base(0x7ffd_e000);
    let before = prepare(&mut p, API, &[DEVICE, 256, OUTPUT]);
    assert_eq!(p.run(0).api_calls, 0);
    assert_eq!(p.cpu, before);
    words(&mut p, 0x1000_fff4, &[CODE, DEVICE, 256]);
    p.cpu.set_register(Register32::Esp, 0x1000_fff4);
    refused(
        &mut p,
        &ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::Unmapped {
            address: 0x1001_0000,
        })),
    );
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    words(&mut p, 0xffff_fff0, &[CODE, DEVICE, 256, OUTPUT]);
    p.cpu.set_register(Register32::Esp, 0xffff_fff0);
    refused(
        &mut p,
        &ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::AddressOverflow)),
    );
    assert_eq!(output(&p), [0; 256]);
    assert_eq!(call(&mut p, API, &[DEVICE, 256, OUTPUT]), 0);
    assert_eq!(output(&p), [0x80; 256]);
}
