#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/window_creation_executable.rs"]
mod window_creation_executable;

use ring3_core::execution::{Permissions, Process32, ProcessStop, Register32, StopReason};

const STACK: u32 = 0x1000_ef00;
const CREATE: u32 = 0x7000_02a8;
const HANDLE: u32 = 0x7500_0004;
const RETURN: u32 = 0x7000_0ff8;
const ARGS: [u32; 12] = [
    0,
    0x0040_2180,
    0x0040_2190,
    0x00ca_0000,
    10,
    20,
    130,
    90,
    0,
    0,
    0x0040_0000,
    0x1234,
];

fn put(p: &mut Process32, address: u32, values: &[u32]) {
    let bytes: Vec<_> = values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    p.memory.write(u64::from(address), &bytes).unwrap();
}

fn prepare(p: &mut Process32, api: u32, stack: u32, args: &[u32]) {
    p.cpu.eip = api;
    p.cpu.set_register(Register32::Esp, stack);
    let mut frame = vec![0x0040_10f0];
    frame.extend_from_slice(args);
    put(p, stack, &frame);
}

fn query(p: &mut Process32, api: u32, args: &[u32]) -> u32 {
    let cpu = p.cpu;
    prepare(p, api, 0x1000_c000, args);
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    let value = p.cpu.register(Register32::Eax);
    p.cpu = cpu;
    value
}

fn ready(bytes: &[u8]) -> Process32 {
    let mut p = Process32::load(bytes, 32).unwrap();
    assert_eq!(query(&mut p, 0x7000_0264, &[0x0040_21c0]), 0xc000);
    prepare(&mut p, CREATE, STACK, &ARGS);
    p
}

fn finish(p: &mut Process32) -> u32 {
    assert_eq!(
        p.run(1000).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(p.cpu.register(Register32::Esp), STACK + 52);
    p.cpu.register(Register32::Eax)
}

fn logged(reject: Option<(u32, u32)>) -> Vec<u8> {
    let mut procedure = vec![
        0x8b, 0x44, 0x24, 8, 0x8b, 0x0d, 0x80, 0x22, 0x40, 0, 0x89, 4, 0x8d, 0, 0x23, 0x40, 0,
        0x41, 0x89, 0x0d, 0x80, 0x22, 0x40, 0,
    ];
    if let Some((message, result)) = reject {
        procedure.push(0x3d);
        procedure.extend_from_slice(&message.to_le_bytes());
        procedure.extend_from_slice(&[0x75, 17, 0x6a, 77, 0xb8, 0, 0, 0, 0x70, 0xff, 0xd0, 0xb8]);
        procedure.extend_from_slice(&result.to_le_bytes());
        procedure.extend_from_slice(&[0xc2, 16, 0]);
    }
    for _ in 0..4 {
        procedure.extend_from_slice(&[0xff, 0x74, 0x24, 16]);
    }
    procedure.extend_from_slice(&[0xff, 0x15, 0x68, 0x20, 0x40, 0, 0xc2, 16, 0]);
    window_creation_executable::pe32(&procedure)
}

fn with_hook(code: &[u8]) -> Process32 {
    let mut bytes = logged(None);
    bytes[0x380..0x380 + code.len()].copy_from_slice(code);
    let mut p = ready(&bytes);
    assert_eq!(
        query(&mut p, 0x7000_024c, &[5, 0xdead_beef, 0, 1]),
        0x7400_0004
    );
    assert_eq!(
        query(&mut p, 0x7000_024c, &[5, 0x0040_1180, 0, 1]),
        0x7400_0008
    );
    p
}

fn words(p: &Process32, address: u32, count: usize) -> Vec<u32> {
    let mut bytes = vec![0; count * 4];
    p.memory.read(u64::from(address), &mut bytes).unwrap();
    bytes
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
        .collect()
}

#[test]
fn hidden_creation_delivers_ordered_guest_messages_and_returns_a_real_window() {
    let mut p = Process32::load(&window_creation_executable::guest(), 32).unwrap();
    let mut messages = Vec::new();
    let mut counts = (0, 0);
    loop {
        if p.cpu.eip == 0x7000_02ac {
            let frame = words(&p, p.cpu.register(Register32::Esp), 5);
            assert_eq!(frame[1], 0x7500_0004);
            assert_eq!(frame[3], 0);
            messages.push(frame[2]);
            match frame[2] {
                0x81 | 1 => assert_eq!(
                    words(&p, frame[4], 12),
                    [
                        0x1234,
                        0x0040_0000,
                        0,
                        0,
                        90,
                        130,
                        20,
                        10,
                        0x00ca_0000,
                        0x0040_2190,
                        0x0040_2180,
                        0
                    ]
                ),
                0x83 => assert_eq!(words(&p, frame[4], 4), [10, 20, 140, 110]),
                0x24 => assert_eq!(
                    words(&p, frame[4], 10),
                    [
                        0,
                        0,
                        646,
                        486,
                        (-3_i32).cast_unsigned(),
                        (-3_i32).cast_unsigned(),
                        112,
                        27,
                        652,
                        492
                    ]
                ),
                _ => panic!("unexpected creation message"),
            }
        }
        let run = p.run(1);
        counts.0 += run.instructions;
        counts.1 += run.api_calls;
        if run.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
            assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
            break;
        }
        assert!(counts.0 + counts.1 < 200);
    }
    assert_eq!(messages, [0x24, 0x81, 0x83, 1]);
    assert_eq!(p.cpu.register(Register32::Eax), 1);
    assert_eq!(p.cpu.register(Register32::Ebx), 0x7500_0004);
    assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
    let mut whole = Process32::load(&window_creation_executable::guest(), 32).unwrap();
    let run = whole.run(200);
    assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((run.instructions, run.api_calls), counts);
    assert_eq!(whole.cpu, p.cpu);
}

#[test]
fn created_title_geometry_and_class_lifetime_share_the_actual_window() {
    let mut p = ready(&logged(None));
    query(&mut p, 0x7000_0000, &[77]);
    assert_eq!(finish(&mut p), HANDLE);
    assert_eq!(
        query(&mut p, 0x7000_026c, &[0x0040_2180, 0x0040_2190]),
        HANDLE
    );
    assert_eq!(query(&mut p, 0x7000_02b0, &[HANDLE, 0x0040_2280]), 1);
    assert_eq!(words(&p, 0x0040_2280, 4), [10, 20, 140, 110]);
    assert_eq!(query(&mut p, 0x7000_02b4, &[HANDLE, 0x0040_2280]), 1);
    assert_eq!(words(&p, 0x0040_2280, 4), [0, 0, 124, 65]);
    assert_eq!(p.last_error().unwrap(), 77);
    assert_eq!(query(&mut p, 0x7000_0268, &[0x0040_2180, 0x0040_0000]), 0);
    assert_eq!(p.last_error().unwrap(), 1412);
    let mut other = Process32::load(&window_creation_executable::guest(), 32).unwrap();
    assert_eq!(query(&mut other, 0x7000_0270, &[HANDLE]), 0);
}

#[test]
fn nccreate_and_create_rejections_deliver_distinct_cleanup_and_release_the_class() {
    for (message, result, expected) in [
        (0x81, 0, vec![0x24, 0x81, 0x82]),
        (1, u32::MAX, vec![0x24, 0x81, 0x83, 1, 2, 0x82]),
    ] {
        let mut p = ready(&logged(Some((message, result))));
        assert_eq!(finish(&mut p), 0);
        assert_eq!(p.last_error().unwrap(), 77);
        assert_eq!(words(&p, 0x0040_2280, 1)[0] as usize, expected.len());
        assert_eq!(words(&p, 0x0040_2300, expected.len()), expected);
        assert_eq!(query(&mut p, 0x7000_0270, &[HANDLE]), 0);
        assert_eq!(query(&mut p, 0x7000_026c, &[0, 0]), 0);
        assert_eq!(query(&mut p, 0x7000_0268, &[0x0040_2180, 0x0040_0000]), 1);
    }
}

#[test]
fn cbt_observes_a_valid_window_and_vetoes_without_window_messages() {
    let hook = [
        0x6a, 77, 0xb8, 0, 0, 0, 0x70, 0xff, 0xd0, 0xb8, 1, 0, 0, 0, 0xc2, 12, 0,
    ];
    let mut p = with_hook(&hook);
    assert_eq!(p.run(1).api_calls, 1);
    assert_eq!(p.cpu.eip, 0x0040_1180);
    let frame = words(&p, p.cpu.register(Register32::Esp), 4);
    assert_eq!(frame, [RETURN, 3, HANDLE, STACK - 112 + 48]);
    assert_eq!(words(&p, frame[3], 2), [STACK - 112, 0]);
    assert_eq!(query(&mut p, 0x7000_0270, &[HANDLE]), 1);
    assert_eq!(query(&mut p, 0x7000_02b0, &[HANDLE, 0x0040_2290]), 1);
    assert_eq!(words(&p, 0x0040_2290, 4), [0; 4]);
    assert_eq!(query(&mut p, 0x7000_0268, &[0x0040_2180, 0x0040_0000]), 0);
    assert_eq!(p.last_error().unwrap(), 1412);
    assert_eq!(finish(&mut p), 0);
    assert_eq!(p.last_error().unwrap(), 77);
    assert_eq!(words(&p, 0x0040_2280, 1), [0]);
    assert_eq!(query(&mut p, 0x7000_0270, &[HANDLE]), 0);
    assert_eq!(query(&mut p, 0x7000_0268, &[0x0040_2180, 0x0040_0000]), 1);
}

#[test]
fn newest_cbt_can_change_geometry_without_automatic_hook_chaining() {
    let mut hook = vec![0x8b, 0x44, 0x24, 12, 0x8b, 0];
    for (offset, value) in [(16, 180_i32), (20, 220), (24, -10), (28, 20)] {
        hook.extend_from_slice(&[0xc7, 0x40, offset]);
        hook.extend_from_slice(&value.to_le_bytes());
    }
    hook.extend_from_slice(&[0x31, 0xc0, 0xc2, 12, 0]);
    let mut p = with_hook(&hook);
    assert_eq!(finish(&mut p), HANDLE);
    assert_eq!(query(&mut p, 0x7000_02b0, &[HANDLE, 0x0040_2290]), 1);
    assert_eq!(
        words(&p, 0x0040_2290, 4),
        [20, (-10_i32).cast_unsigned(), 240, 170]
    );
    assert_eq!(query(&mut p, 0x7000_02b4, &[HANDLE, 0x0040_2290]), 1);
    assert_eq!(words(&p, 0x0040_2290, 4), [0, 0, 214, 155]);
}

#[test]
fn unsupported_cbt_mutation_stops_before_next_phase_and_can_be_repaired() {
    let hook = [
        0x8b, 0x44, 0x24, 12, 0x8b, 0, 0xc7, 0x40, 32, 0, 0, 0xca, 0x10, 0x31, 0xc0, 0xc2, 12, 0,
    ];
    let mut p = with_hook(&hook);
    assert_eq!(
        p.run(100).reason,
        ProcessStop::UnsupportedApi { address: RETURN }
    );
    let before = p.cpu;
    assert_eq!(p.run(0).api_calls, 0);
    assert_eq!(p.cpu, before);
    assert_eq!(
        p.run(1).reason,
        ProcessStop::UnsupportedApi { address: RETURN }
    );
    assert_eq!(p.cpu, before);
    assert_eq!(words(&p, 0x0040_2280, 1), [0]);
    assert_eq!(query(&mut p, 0x7000_0270, &[HANDLE]), 1);
    put(&mut p, STACK - 112 + 32, &[ARGS[3]]);
    assert_eq!(finish(&mut p), HANDLE);
    assert_eq!(words(&p, 0x0040_2280, 1), [4]);
}

#[test]
fn setup_scratch_fault_and_zero_budget_publish_nothing_and_preserve_the_frame() {
    let mut p = ready(&logged(None));
    let stack = 0x1000_e080;
    prepare(&mut p, CREATE, stack, &ARGS);
    let before = p.cpu;
    let frame = words(&p, stack - 132, 46);
    p.memory
        .protect(0x1000_d000, 4096, Permissions::READ)
        .unwrap();
    assert_eq!(p.run(0).api_calls, 0);
    assert_eq!(p.cpu, before);
    let run = p.run(1);
    assert!(matches!(
        run.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    assert_eq!(words(&p, stack - 132, 46), frame);
    assert_eq!(query(&mut p, 0x7000_0270, &[HANDLE]), 0);
    p.memory
        .protect(0x1000_d000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(
        p.run(1000).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(p.cpu.register(Register32::Eax), HANDLE);
}

#[test]
fn complete_thirteen_word_input_is_read_before_any_creation_effect() {
    let mut p = ready(&logged(None));
    p.cpu.eip = CREATE;
    p.cpu.set_register(Register32::Esp, 0x1000_ffd0);
    let before = p.cpu;
    let scratch = words(&p, 0x1000_ffd0 - 132, 33);
    let run = p.run(1);
    assert!(matches!(
        run.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    assert_eq!(words(&p, 0x1000_ffd0 - 132, 33), scratch);
    assert_eq!(query(&mut p, 0x7000_0270, &[HANDLE]), 0);
}

#[test]
fn saved_return_fault_retains_completed_callbacks_and_window_until_repaired() {
    let mut p = ready(&logged(None));
    for _ in 0..500 {
        if p.cpu.eip == RETURN && words(&p, 0x0040_2280, 1) == [4] {
            break;
        }
        assert_eq!(
            p.run(1).reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
    }
    assert_eq!(p.cpu.eip, RETURN);
    let before = p.cpu;
    p.memory
        .protect(0x1000_e000, 4096, Permissions::NONE)
        .unwrap();
    let run = p.run(1);
    assert!(matches!(
        run.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    assert_eq!(query(&mut p, 0x7000_0270, &[HANDLE]), 1);
    p.memory
        .protect(0x1000_e000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(finish(&mut p), HANDLE);
    assert_eq!(words(&p, 0x0040_2280, 1), [4]);
}

#[test]
fn unsupported_profiles_stop_without_publishing_a_window() {
    for (index, value) in [
        (0, 1),
        (3, 0x10ca_0000),
        (4, 0x8000_0000),
        (6, u32::MAX),
        (8, 1),
        (9, 1),
    ] {
        let mut p = ready(&logged(None));
        let mut args = ARGS;
        args[index] = value;
        prepare(&mut p, CREATE, STACK, &args);
        let before = p.cpu;
        let scratch = words(&p, STACK - 132, 33);
        let run = p.run(1);
        assert_eq!(run.reason, ProcessStop::UnsupportedApi { address: CREATE });
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
        assert_eq!(words(&p, STACK - 132, 33), scratch);
        assert_eq!(query(&mut p, 0x7000_0270, &[HANDLE]), 0);
    }
}

#[test]
fn minmax_limits_change_size_without_reapplying_the_creation_buffer() {
    let mut procedure = vec![0x83, 0x7c, 0x24, 8, 0x24, 0x75, 39, 0x8b, 0x44, 0x24, 16];
    for (offset, value) in [(24, 200_u32), (28, 100), (32, 210), (36, 110), (220, 900)] {
        procedure.extend_from_slice(&[0xc7, 0x40, offset]);
        procedure.extend_from_slice(&value.to_le_bytes());
    }
    procedure.extend(window_creation_executable::default_procedure());
    let mut p = ready(&window_creation_executable::pe32(&procedure));
    for _ in 0..100 {
        if p.cpu.eip == 0x7000_02ac && words(&p, p.cpu.register(Register32::Esp) + 8, 1) == [0x81] {
            break;
        }
        assert_eq!(
            p.run(1).reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
    }
    assert_eq!(p.cpu.eip, 0x7000_02ac);
    assert_eq!(words(&p, STACK - 112 + 20, 1), [900]);
    assert_eq!(query(&mut p, 0x7000_02b4, &[HANDLE, 0x0040_2290]), 1);
    assert_eq!(words(&p, 0x0040_2290, 4), [0, 0, 200, 100]);
    assert_eq!(finish(&mut p), HANDLE);
    assert_eq!(query(&mut p, 0x7000_02b0, &[HANDLE, 0x0040_2290]), 1);
    assert_eq!(words(&p, 0x0040_2290, 4), [10, 20, 210, 120]);
    assert_eq!(query(&mut p, 0x7000_02b4, &[HANDLE, 0x0040_2290]), 1);
    assert_eq!(words(&p, 0x0040_2290, 4), [0, 0, 194, 75]);
}

#[test]
fn nccalc_callback_owns_the_client_rectangle_without_a_second_default_adjustment() {
    let mut procedure = vec![
        0x81, 0x7c, 0x24, 8, 0x83, 0, 0, 0, 0x75, 37, 0x8b, 0x44, 0x24, 16,
    ];
    for (offset, value) in [(0, 1_u32), (4, 2), (8, 101), (12, 52)] {
        procedure.extend_from_slice(&[0xc7, 0x40, offset]);
        procedure.extend_from_slice(&value.to_le_bytes());
    }
    procedure.extend_from_slice(&[0x31, 0xc0, 0xc2, 16, 0]);
    procedure.extend(window_creation_executable::default_procedure());
    let mut p = ready(&window_creation_executable::pe32(&procedure));
    assert_eq!(finish(&mut p), HANDLE);
    assert_eq!(query(&mut p, 0x7000_02b4, &[HANDLE, 0x0040_2290]), 1);
    assert_eq!(words(&p, 0x0040_2290, 4), [0, 0, 100, 50]);
}

#[test]
fn nested_creation_shares_the_callback_depth_limit_before_publication() {
    let mut procedure = Vec::new();
    for value in ARGS.iter().rev() {
        procedure.push(0x68);
        procedure.extend_from_slice(&value.to_le_bytes());
    }
    procedure.push(0xb8);
    procedure.extend_from_slice(&CREATE.to_le_bytes());
    procedure.extend_from_slice(&[0xff, 0xd0]);
    procedure.extend(window_creation_executable::default_procedure());
    let mut p = ready(&window_creation_executable::pe32(&procedure));
    assert_eq!(
        p.run(2000).reason,
        ProcessStop::UnsupportedApi { address: CREATE }
    );
    assert_eq!(query(&mut p, 0x7000_0270, &[HANDLE + 63 * 4]), 1);
    assert_eq!(query(&mut p, 0x7000_0270, &[HANDLE + 64 * 4]), 0);
    let before = p.cpu;
    let scratch = words(&p, p.cpu.register(Register32::Esp) - 132, 33);
    assert_eq!(
        p.run(1).reason,
        ProcessStop::UnsupportedApi { address: CREATE }
    );
    assert_eq!(p.cpu, before);
    assert_eq!(
        words(&p, p.cpu.register(Register32::Esp) - 132, 33),
        scratch
    );
}

#[test]
fn window_capacity_is_bounded_and_error_write_fault_does_not_complete_the_call() {
    let mut p = ready(&window_creation_executable::guest());
    for index in 0..4096 {
        prepare(&mut p, CREATE, STACK, &ARGS);
        assert_eq!(finish(&mut p), HANDLE + index * 4);
    }
    assert_eq!(query(&mut p, 0x7000_026c, &[0, 0]), HANDLE + 4095 * 4);
    prepare(&mut p, CREATE, STACK, &ARGS);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ)
        .unwrap();
    let before = p.cpu;
    let scratch = words(&p, STACK - 132, 33);
    let run = p.run(1);
    assert!(matches!(
        run.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    assert_eq!(words(&p, STACK - 132, 33), scratch);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(finish(&mut p), 0);
    assert_eq!(p.last_error().unwrap(), 8);
    assert_eq!(query(&mut p, 0x7000_0270, &[HANDLE + 4096 * 4]), 0);
}

#[test]
fn successful_creation_does_not_touch_error_fields_or_allocate_guest_pages() {
    let mut p = ready(&window_creation_executable::guest());
    let pages = p.memory.mapped_pages();
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::NONE)
        .unwrap();
    p.memory
        .protect(0x7000_2000, 4096, Permissions::NONE)
        .unwrap();
    assert_eq!(finish(&mut p), HANDLE);
    assert_eq!(p.memory.mapped_pages(), pages);
}

#[test]
fn rejected_handles_are_not_reused_and_missing_classes_fail_normally() {
    let mut p = ready(&logged(Some((0x81, 0))));
    assert_eq!(finish(&mut p), 0);
    prepare(&mut p, CREATE, STACK, &ARGS);
    assert_eq!(finish(&mut p), 0);
    assert_eq!(words(&p, STACK - 128, 1), [HANDLE + 4]);
    assert_eq!(query(&mut p, 0x7000_0268, &[0x0040_2180, 0x0040_0000]), 1);
    prepare(&mut p, CREATE, STACK, &ARGS);
    assert_eq!(finish(&mut p), 0);
    assert_eq!(p.last_error().unwrap(), 1407);
}
