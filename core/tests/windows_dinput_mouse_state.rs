#[path = "support/dinput_acquire_cases.rs"]
#[allow(dead_code)]
mod dinput_acquire_cases;
#[path = "support/dinput_cooperative_cases.rs"]
#[allow(dead_code)]
mod dinput_cooperative_cases;
#[path = "support/dinput_device_cases.rs"]
#[allow(dead_code)]
mod dinput_device_cases;
#[path = "support/dinput_format_cases.rs"]
#[allow(dead_code)]
mod dinput_format_cases;
#[path = "support/dinput_mouse_acquire_cases.rs"]
#[allow(dead_code)]
mod dinput_mouse_acquire_cases;
#[path = "support/dinput_mouse_cases.rs"]
#[allow(dead_code)]
mod dinput_mouse_cases;
#[path = "support/dinput_mouse_format_cases.rs"]
#[allow(dead_code)]
mod dinput_mouse_format_cases;
#[path = "support/dinput_mouse_state_cases.rs"]
mod dinput_mouse_state_cases;
#[path = "support/dinput_state_cases.rs"]
#[allow(dead_code)]
mod dinput_state_cases;
#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/window_creation_executable.rs"]
mod window_creation_executable;

#[test]
fn imported_mouse_state_matches_across_budgets() {
    dinput_mouse_state_cases::mouse_state_across_budgets();
}

use dinput_acquire_cases::{DATA, WINDOW};
use dinput_mouse_format_cases::FORMAT;
use dinput_mouse_state_cases::{API, DEVICE, acquired, process, state};
use dinput_state_cases::{CODE, OUTPUT, STACK, call, prepare, words};
use ring3_core::execution::{
    MemoryError, MouseInput, MouseInputError, Permissions, Process32, ProcessStop, Register32,
    StopReason,
};

const ACQUIRE: u32 = 0x7000_05ac;
const UNACQUIRE: u32 = 0x7000_05b0;

fn sample(relative: [i32; 2], wheel_steps: i32, pressed: bool) -> MouseInput {
    MouseInput {
        relative,
        wheel_steps,
        buttons: [pressed; 8],
    }
}

fn read(p: &mut Process32) -> ([i32; 3], [u8; 8]) {
    assert_eq!(call(p, API, &[DEVICE, 20, OUTPUT]), 0);
    state(p, OUTPUT)
}

#[test]
fn host_sample_is_owned_inert_and_exited_admission_precedes_overflow() {
    let mut p = acquired();
    assert_eq!(read(&mut p), ([0; 3], [0; 8]));
    let cpu = p.cpu;
    let windows = p.window_snapshots();
    let pages = p.memory.mapped_pages();
    let mut input = sample([7, -8], 1, true);
    p.submit_mouse_input(input).unwrap();
    input.buttons.fill(false);
    assert_eq!(p.cpu, cpu);
    assert_eq!(p.window_snapshots(), windows);
    assert_eq!(p.memory.mapped_pages(), pages);
    assert_eq!(state(&p, OUTPUT), ([0; 3], [0; 8]));
    assert_eq!(read(&mut p), ([7, -8, 120], [0x80; 8]));
    p.submit_mouse_input(input).unwrap();
    assert_eq!(read(&mut p), ([7, -8, 120], [0; 8]));
    prepare(&mut p, 0x7000_0008, &[9]);
    assert_eq!(p.run(1).reason, ProcessStop::Exited(9));
    let cpu = p.cpu;
    assert_eq!(
        p.submit_mouse_input(sample([0; 2], i32::MAX, true)),
        Err(MouseInputError::Exited)
    );
    assert_eq!(p.cpu, cpu);
}

#[test]
fn all_axis_and_wheel_overflows_reject_the_whole_sample_then_recover() {
    let mut p = acquired();
    p.submit_mouse_input(sample([i32::MIN, i32::MAX], 0, true))
        .unwrap();
    let before = p.cpu;
    for input in [
        sample([-1, 0], 0, false),
        sample([0, 1], 0, false),
        sample([1, -1], i32::MAX, false),
        sample([1, -1], i32::MIN, false),
    ] {
        assert_eq!(
            p.submit_mouse_input(input),
            Err(MouseInputError::MotionOverflow)
        );
        assert_eq!(p.cpu, before);
        assert_eq!(state(&p, OUTPUT), ([0; 3], [0; 8]));
    }
    assert_eq!(read(&mut p), ([i32::MIN, i32::MAX, 0], [0x80; 8]));
    for direction in [-1, 1] {
        p.submit_mouse_input(sample([0; 2], direction * 17_895_697, true))
            .unwrap();
        assert_eq!(
            p.submit_mouse_input(sample([1, 2], direction, false)),
            Err(MouseInputError::MotionOverflow)
        );
        assert_eq!(read(&mut p), ([0, 0, direction * 2_147_483_640], [0x80; 8]));
        p.submit_mouse_input(sample([1, 2], direction, false))
            .unwrap();
        assert_eq!(read(&mut p), ([1, 2, direction * 120], [0; 8]));
    }
}

#[test]
fn guest_focus_and_acquisition_control_motion_while_buttons_remain_current() {
    let mut p = process(true);
    p.submit_mouse_input(sample([7, 8], 1, true)).unwrap();
    assert_eq!(call(&mut p, ACQUIRE, &[DEVICE]), 0);
    assert_eq!(read(&mut p), ([0; 3], [0x80; 8]));
    p.submit_mouse_input(sample([7, 8], 1, false)).unwrap();
    assert_eq!(call(&mut p, ACQUIRE, &[DEVICE]), 1);
    assert_eq!(call(&mut p, 0x7000_0598, &[DEVICE, FORMAT]), 0x8007_00aa);
    assert_eq!(call(&mut p, 0x7000_059c, &[DEVICE, WINDOW, 5]), 0x8007_00aa);
    assert_eq!(read(&mut p), ([7, 8, 120], [0; 8]));
    p.submit_mouse_input(sample([1, 2], 1, true)).unwrap();
    assert_eq!(call(&mut p, 0x7000_0440, &[WINDOW, 0]), 1);
    p.submit_mouse_input(sample([i32::MAX, i32::MAX], 1, false))
        .unwrap();
    assert_eq!(call(&mut p, API, &[DEVICE, 20, 0x5000_0000]), 0x8007_001e);
    assert_eq!(call(&mut p, API, &[DEVICE, 19, 0]), 0x8007_0057);
    assert_eq!(call(&mut p, 0x7000_0440, &[WINDOW, 5]), 0);
    assert_eq!(call(&mut p, API, &[DEVICE, 20, OUTPUT]), 0x8007_001e);
    assert_eq!(call(&mut p, ACQUIRE, &[DEVICE]), 0);
    assert_eq!(read(&mut p), ([0; 3], [0; 8]));
    p.submit_mouse_input(sample([4, 5], -1, true)).unwrap();
    assert_eq!(call(&mut p, UNACQUIRE, &[DEVICE]), 0);
    assert_eq!(call(&mut p, API, &[DEVICE, 20, 0x5000_0000]), 0x8007_000c);
    assert_eq!(call(&mut p, ACQUIRE, &[DEVICE]), 0);
    assert_eq!(read(&mut p), ([0; 3], [0x80; 8]));
}

fn create_mouse(p: &mut Process32) {
    assert_eq!(call(p, 0x7000_0568, &[0x7001_7800, DATA + 64, DATA, 0]), 0);
    let device = DEVICE + 4;
    assert_eq!(call(p, 0x7000_0598, &[device, FORMAT]), 0);
    assert_eq!(call(p, 0x7000_059c, &[device, WINDOW, 5]), 0);
}

#[test]
fn exclusive_owner_transfer_does_not_move_old_motion_between_devices() {
    let mut p = acquired();
    create_mouse(&mut p);
    let second = DEVICE + 4;
    assert_eq!(call(&mut p, ACQUIRE, &[second]), 0x8007_0005);
    p.submit_mouse_input(sample([9, -9], 1, true)).unwrap();
    assert_eq!(call(&mut p, API, &[second, 20, OUTPUT]), 0x8007_000c);
    assert_eq!(call(&mut p, 0x7000_0594, &[DEVICE]), 0);
    assert_eq!(call(&mut p, ACQUIRE, &[second]), 0);
    assert_eq!(call(&mut p, API, &[second, 20, OUTPUT]), 0);
    assert_eq!(state(&p, OUTPUT), ([0; 3], [0x80; 8]));
    p.submit_mouse_input(sample([3, 4], -1, false)).unwrap();
    assert_eq!(call(&mut p, API, &[second, 20, OUTPUT]), 0);
    assert_eq!(state(&p, OUTPUT), ([3, 4, -120], [0; 8]));
}

fn refused(p: &mut Process32, expected: &ProcessStop) {
    let before = p.cpu;
    for _ in 0..2 {
        let result = p.run(1);
        assert_eq!(&result.reason, expected);
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
    }
}

#[test]
fn output_span_faults_preserve_motion_for_a_successful_retry() {
    let mut p = acquired();
    p.submit_mouse_input(sample([7, -8], 1, true)).unwrap();
    p.memory.write(0x3100_0ff7, &[0x55; 9]).unwrap();
    prepare(&mut p, API, &[DEVICE, 20, 0x3100_0ff7]);
    refused(
        &mut p,
        &ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::Unmapped {
            address: 0x3100_1000,
        })),
    );
    let mut prefix = [0; 9];
    p.memory.read(0x3100_0ff7, &mut prefix).unwrap();
    assert_eq!(prefix, [0x55; 9]);
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    prepare(&mut p, API, &[DEVICE, 20, 0xffff_fff0]);
    refused(
        &mut p,
        &ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::AddressOverflow)),
    );
    p.memory
        .map_zeroed(0x3100_1000, 4096, Permissions::READ)
        .unwrap();
    prepare(&mut p, API, &[DEVICE, 20, 0x3100_0ff7]);
    refused(
        &mut p,
        &ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::PermissionDenied {
            address: 0x3100_1000,
            access: ring3_core::execution::Access::Write,
        })),
    );
    p.memory
        .protect(
            0x3100_1000,
            4096,
            Permissions {
                write: true,
                ..Permissions::NONE
            },
        )
        .unwrap();
    assert_eq!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!(p.cpu.register(Register32::Eax), 0);
    p.memory
        .protect(0x3100_1000, 4096, Permissions::READ)
        .unwrap();
    assert_eq!(state(&p, 0x3100_0ff7), ([7, -8, 120], [0x80; 8]));
    assert_eq!(read(&mut p), ([0; 3], [0x80; 8]));
}

#[test]
fn scalar_errors_and_success_need_no_teb_or_errno_writes() {
    let mut p = process(false);
    words(&mut p, 0x7ffd_e034, &[77]);
    words(&mut p, 0x7000_2020, &[88]);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::NONE)
        .unwrap();
    p.memory
        .protect(0x7000_2000, 4096, Permissions::READ)
        .unwrap();
    for size in [0, 16, 19, 21, u32::MAX] {
        assert_eq!(call(&mut p, API, &[DEVICE, size, 0x5000_0000]), 0x8007_0057);
    }
    assert_eq!(call(&mut p, API, &[DEVICE, 20, 0]), 0x8007_0057);
    assert_eq!(call(&mut p, API, &[DEVICE, 20, OUTPUT]), 0x8007_000c);
    assert_eq!(call(&mut p, 0x7000_0598, &[DEVICE, FORMAT]), 0);
    assert_eq!(call(&mut p, 0x7000_059c, &[DEVICE, WINDOW, 5]), 0);
    assert_eq!(call(&mut p, ACQUIRE, &[DEVICE]), 0);
    assert_eq!(read(&mut p), ([0; 3], [0; 8]));
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ)
        .unwrap();
    assert_eq!(p.last_error().unwrap(), 77);
    let mut errno = [0; 4];
    p.memory.read(0x7000_2020, &mut errno).unwrap();
    assert_eq!(u32::from_le_bytes(errno), 88);
}

#[test]
fn receiver_actor_and_stack_guards_preserve_pending_motion_and_aliases_consume_once() {
    let mut p = acquired();
    p.submit_mouse_input(sample([7, -8], 1, true)).unwrap();
    for receiver in [
        0,
        DEVICE + 1,
        DEVICE + 4,
        0x7001_7900,
        0x7001_7800,
        u32::MAX,
    ] {
        prepare(&mut p, API, &[receiver, 0, 0]);
        refused(&mut p, &ProcessStop::UnsupportedApi { address: API });
    }
    p.cpu.set_fs_base(0);
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, 0x5000_0000);
    refused(&mut p, &ProcessStop::UnsupportedApi { address: API });
    p.cpu.set_fs_base(0x7ffd_e000);
    let before = prepare(&mut p, API, &[DEVICE, 20, OUTPUT]);
    assert_eq!(p.run(0).api_calls, 0);
    assert_eq!(p.cpu, before);
    words(&mut p, 0x1000_fff4, &[CODE, DEVICE, 20]);
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
    words(&mut p, 0xffff_fff0, &[CODE, DEVICE, 20, OUTPUT]);
    p.cpu.set_register(Register32::Esp, 0xffff_fff0);
    refused(
        &mut p,
        &ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::AddressOverflow)),
    );
    assert_eq!(call(&mut p, API, &[DEVICE, 20, STACK]), 0);
    assert_eq!(p.cpu.eip, 7);
    assert_eq!(state(&p, STACK), ([7, -8, 120], [0x80; 8]));
    assert_eq!(call(&mut p, API, &[DEVICE, 20, 0x7ffd_e000]), 0);
    assert_eq!(state(&p, 0x7ffd_e000), ([0; 3], [0x80; 8]));
}
