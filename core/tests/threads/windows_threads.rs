use super::suspended_thread_executable;

use ring3_core::execution::{
    Cpu32, LoadError, MemoryError, PAGE_SIZE, Permissions, Process32, ProcessStop, Register32,
    StopReason,
};

const CREATE: u32 = 0x7000_0548;
const STACK: u32 = 0x1000_ff00;
const OUTPUT: u32 = 0x0040_2200;

fn load(pages: u32) -> Process32 {
    Process32::load(&suspended_thread_executable::pe32(), pages).unwrap()
}

fn read(p: &Process32, address: u32) -> u32 {
    let mut bytes = [0; 4];
    p.memory.read(u64::from(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

fn prepare(p: &mut Process32, api: u32, args: &[u32]) -> Cpu32 {
    p.cpu.eip = api;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu.eflags = 0xced7;
    for (index, value) in std::iter::once(&0x0040_1000_u32).chain(args).enumerate() {
        p.memory
            .write(u64::from(STACK) + index as u64 * 4, &value.to_le_bytes())
            .unwrap();
    }
    p.cpu
}

fn call(p: &mut Process32, api: u32, args: &[u32]) -> u32 {
    let mut expected = prepare(p, api, args);
    let result = p.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    let value = p.cpu.register(Register32::Eax);
    expected.eip = 0x0040_1000;
    expected.set_register(Register32::Eax, value);
    expected.set_register(
        Register32::Esp,
        STACK
            + if api == CREATE {
                4
            } else {
                u32::try_from(args.len() + 1).unwrap() * 4
            },
    );
    assert_eq!(p.cpu, expected);
    value
}

fn args() -> [u32; 6] {
    [0, 0, 0x0040_1000, 0x1234_5678, 4, OUTPUT]
}

#[test]
fn thread_handles_are_nonsignaled_and_closing_preserves_live_storage() {
    let mut p = load(80);
    let first = call(&mut p, CREATE, &args());
    assert_eq!(read(&p, OUTPUT), 2);
    assert_eq!(call(&mut p, 0x7000_0258, &[first]), 0);
    assert_eq!(call(&mut p, 0x7000_0254, &[first, 1]), 1);
    assert_eq!(call(&mut p, 0x7000_0258, &[first]), 1);
    assert_eq!(p.last_error().unwrap(), 0);
    assert_eq!(call(&mut p, 0x7000_0214, &[first, 0]), 258);
    for api in [0x7000_0218, 0x7000_0540, 0x7000_0544] {
        assert_eq!(call(&mut p, api, &[first]), 0);
        assert_eq!(p.last_error().unwrap(), 6);
    }
    let pages = p.memory.mapped_pages();
    for timeout in [1, u32::MAX] {
        let before = prepare(&mut p, 0x7000_0214, &[first, timeout]);
        assert_eq!(
            p.run(1).reason,
            ProcessStop::UnsupportedApi {
                address: 0x7000_0214
            }
        );
        assert_eq!(p.cpu, before);
    }
    assert_eq!(call(&mut p, 0x7000_021c, &[first]), 1);
    assert_eq!(p.memory.mapped_pages(), pages);
    assert_eq!(read(&p, 0x1101_0024), 2);
    assert_eq!(call(&mut p, 0x7000_0214, &[first, 0]), u32::MAX);
    let mut second_args = args();
    second_args[5] = 0;
    let second = call(&mut p, CREATE, &second_args);
    assert_ne!(first, second);
    assert_eq!(read(&p, OUTPUT), 2);
    assert_eq!(read(&p, 0x1102_1024), 3);
    assert_eq!(p.last_error().unwrap(), 6);
}

#[test]
fn unsupported_creation_profiles_preserve_cpu_memory_and_identity() {
    for (index, value) in [(0, 1), (1, 4096), (4, 0), (4, 1), (4, 0x10004)] {
        let mut p = load(64);
        let pages = p.memory.mapped_pages();
        let mut values = args();
        values[index] = value;
        let before = prepare(&mut p, CREATE, &values);
        assert_eq!(
            p.run(1).reason,
            ProcessStop::UnsupportedApi { address: CREATE }
        );
        assert_eq!(p.cpu, before);
        assert_eq!(p.memory.mapped_pages(), pages);
        assert_eq!(read(&p, OUTPUT), 0);
        assert_eq!(call(&mut p, CREATE, &args()), 0x7200_0004);
        assert_eq!(read(&p, OUTPUT), 2);
    }
}

#[test]
fn invalid_entry_or_output_cannot_publish_a_thread() {
    for (index, value) in [
        (2, 0),
        (2, OUTPUT),
        (5, 0x0040_1000),
        (5, 0x0040_2ffe),
        (5, u32::MAX - 1),
        (5, 0x1100_0000),
    ] {
        let mut p = load(64);
        let pages = p.memory.mapped_pages();
        let mut values = args();
        values[index] = value;
        let before = prepare(&mut p, CREATE, &values);
        assert!(matches!(
            p.run(1).reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!(p.cpu, before);
        assert_eq!(p.memory.mapped_pages(), pages);
        assert_eq!(read(&p, OUTPUT), 0);
        assert_eq!(call(&mut p, CREATE, &args()), 0x7200_0004);
        assert_eq!(read(&p, OUTPUT), 2);
    }
}

#[test]
fn failed_mapping_preserves_output_and_budget_without_consuming_a_handle() {
    let mut p = load(41);
    let pages = p.memory.mapped_pages();
    let before = prepare(&mut p, CREATE, &args());
    assert_eq!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::PageLimitExceeded))
    );
    assert_eq!(p.cpu, before);
    assert_eq!(p.memory.mapped_pages(), pages);
    assert_eq!(read(&p, OUTPUT), 0);
    assert_eq!(call(&mut p, 0x7000_0210, &[0, 0, 0]), 0x7200_0004);

    let mut collision = load(64);
    collision
        .memory
        .map_zeroed(0x1100_8000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    let pages = collision.memory.mapped_pages();
    let before = prepare(&mut collision, CREATE, &args());
    assert!(matches!(
        collision.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::AlreadyMapped { .. }))
    ));
    assert_eq!(collision.cpu, before);
    assert_eq!(collision.memory.mapped_pages(), pages);
    assert_eq!(read(&collision, OUTPUT), 0);
    assert!(collision.memory.read(0x1100_0000, &mut [0]).is_err());
}

#[test]
fn closed_suspended_threads_still_count_toward_the_context_limit() {
    let mut p = load(600);
    for id in 2..34 {
        let handle = call(&mut p, CREATE, &args());
        assert_eq!(read(&p, OUTPUT), id);
        assert_eq!(call(&mut p, 0x7000_021c, &[handle]), 1);
    }
    let pages = p.memory.mapped_pages();
    let before = prepare(&mut p, CREATE, &args());
    assert_eq!(
        p.run(1).reason,
        ProcessStop::UnsupportedApi { address: CREATE }
    );
    assert_eq!(p.cpu, before);
    assert_eq!(p.memory.mapped_pages(), pages);
    assert_eq!(read(&p, OUTPUT), 33);
}

#[test]
fn creation_obeys_shared_handle_budget_and_return_trap_stops_explicitly() {
    let mut p = load(64);
    let first = call(&mut p, 0x7000_0210, &[0, 0, 0]);
    for _ in 1..4096 {
        call(&mut p, 0x7000_0210, &[0, 0, 0]);
    }
    let pages = p.memory.mapped_pages();
    let before = prepare(&mut p, CREATE, &args());
    assert_eq!(
        p.run(1).reason,
        ProcessStop::UnsupportedApi { address: CREATE }
    );
    assert_eq!(p.cpu, before);
    assert_eq!(p.memory.mapped_pages(), pages);
    assert_eq!(read(&p, OUTPUT), 0);
    call(&mut p, 0x7000_021c, &[first]);
    assert_eq!(call(&mut p, CREATE, &args()), 0x7200_4004);
    p.cpu.eip = 0x7000_054c;
    assert_eq!(
        p.run(1).reason,
        ProcessStop::UnsupportedApi {
            address: 0x7000_054c
        }
    );
}

#[test]
fn loader_reserves_the_child_arena_without_mapping_it_eagerly() {
    let mut bytes = suspended_thread_executable::pe32();
    bytes[0xb4..0xb8].copy_from_slice(&0x1100_0000_u32.to_le_bytes());
    assert!(matches!(
        Process32::load(&bytes, 64),
        Err(LoadError::Memory(MemoryError::AlreadyMapped { .. }))
    ));
}

#[test]
fn incomplete_call_frames_do_not_allocate_and_output_can_alias_the_return_slot() {
    let mut p = load(64);
    let pages = p.memory.mapped_pages();
    p.cpu.eip = CREATE;
    p.cpu.set_register(Register32::Esp, 0x1000_fff0);
    let before = p.cpu;
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    assert_eq!(p.memory.mapped_pages(), pages);
    let mut values = args();
    values[5] = STACK;
    let mut expected = prepare(&mut p, CREATE, &values);
    let result = p.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    expected.eip = 2;
    expected.set_register(Register32::Eax, 0x7200_0004);
    expected.set_register(Register32::Esp, STACK + 4);
    assert_eq!(p.cpu, expected);
    assert_eq!(read(&p, STACK), 2);
}

#[test]
fn suspended_creation_runs_whole_and_single_step_without_executing_the_child() {
    for budget in [1, 100] {
        let mut p = Process32::load(&suspended_thread_executable::pe32(), 64).unwrap();
        let original_pages = p.memory.mapped_pages();
        let mut instructions = 0;
        let mut calls = 0;
        loop {
            let result = p.run(budget);
            instructions += result.instructions;
            calls += result.api_calls;
            if result.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                break;
            }
            assert!(instructions + calls < 100);
        }
        assert_eq!((instructions, calls), (9, 1));
        assert_eq!(p.cpu.register(Register32::Eax), 0x7200_0004);
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
        assert_eq!(p.cpu.fs_base(), 0x7ffd_e000);
        assert_eq!(p.memory.mapped_pages(), original_pages + 17);
        for (address, expected) in [
            (0x0040_2200, 2),
            (0x1101_0000, u32::MAX),
            (0x1101_0004, 0x1101_0000),
            (0x1101_0008, 0x1100_0000),
            (0x1101_0018, 0x1101_0000),
            (0x1101_0024, 2),
            (0x1101_0034, 0),
            (0x1100_fff8, 0x7000_054c),
            (0x1100_fffc, 0x1234_5678),
        ] {
            let mut bytes = [0; 4];
            p.memory.read(address, &mut bytes).unwrap();
            assert_eq!(u32::from_le_bytes(bytes), expected);
        }
    }
}
