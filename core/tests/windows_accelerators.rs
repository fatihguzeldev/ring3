#[path = "support/accelerator_executable.rs"]
mod accelerator_executable;
#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{
    GuestModule, PAGE_SIZE, Permissions, Process32, ProcessOptions, ProcessStop, Register32,
    StopReason,
};

const LOAD: u32 = 0x7000_029c;
const COPY: u32 = 0x7000_02a0;
const STACK: u32 = 0x1000_ff00;
const OUTPUT: u32 = 0x0040_2180;
const EXPECTED: [u8; 12] = [9, 0, 65, 0, 100, 0, 0, 0, 120, 0, 200, 0];

fn process() -> Process32 {
    Process32::load(&accelerator_executable::guest(), 32).unwrap()
}

fn prepare(process: &mut Process32, api: u32, args: &[u32]) {
    process.cpu.eip = api;
    process.cpu.set_register(Register32::Esp, STACK);
    process.cpu.set_register(Register32::Eax, 99);
    process.cpu.eflags = 0xced7;
    for (index, value) in std::iter::once(&0x0040_1000).chain(args).enumerate() {
        process
            .memory
            .write(u64::from(STACK) + index as u64 * 4, &value.to_le_bytes())
            .unwrap();
    }
}

fn call(process: &mut Process32, api: u32, args: &[u32]) -> u32 {
    prepare(process, api, args);
    let mut expected = process.cpu;
    let result = process.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    let value = process.cpu.register(Register32::Eax);
    expected.eip = 0x0040_1000;
    expected.set_register(Register32::Eax, value);
    expected.set_register(
        Register32::Esp,
        STACK + u32::try_from((args.len() + 1) * 4).unwrap(),
    );
    assert_eq!(process.cpu, expected);
    value
}

fn output(process: &Process32, size: usize) -> Vec<u8> {
    let mut bytes = vec![0; size];
    process.memory.read(u64::from(OUTPUT), &mut bytes).unwrap();
    bytes
}

#[test]
fn imported_load_count_copy_runs_whole_or_stepwise() {
    for budget in [1, 100] {
        let mut process = process();
        let (mut instructions, mut calls) = (0, 0);
        for _ in 0..100 {
            let run = process.run(budget);
            instructions += run.instructions;
            calls += run.api_calls;
            if run.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                break;
            }
        }
        assert_eq!((instructions, calls), (14, 3));
        assert_eq!(process.cpu.register(Register32::Eax), 2);
        assert_eq!(process.cpu.register(Register32::Ebx), 0x7700_0004);
        assert_eq!(process.cpu.register(Register32::Ecx), 2);
        assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
        assert_eq!(output(&process, 12), EXPECTED);
    }
}

#[test]
fn cached_tables_survive_source_changes_and_do_not_access_error_cells() {
    let mut process = process();
    let pages = process.memory.mapped_pages();
    process
        .memory
        .write(0x7ffd_e034, &77_u32.to_le_bytes())
        .unwrap();
    process
        .memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::NONE)
        .unwrap();
    process
        .memory
        .protect(0x7000_2000, PAGE_SIZE, Permissions::NONE)
        .unwrap();
    let handle = call(&mut process, LOAD, &[0, 1]);
    process
        .memory
        .protect(0x0040_2000, PAGE_SIZE, Permissions::NONE)
        .unwrap();
    assert_eq!(call(&mut process, LOAD, &[0, 1]), handle);
    assert_eq!(call(&mut process, COPY, &[handle, 0, u32::MAX]), 2);
    for count in [0, 0x8000_0000, u32::MAX] {
        assert_eq!(call(&mut process, COPY, &[handle, u32::MAX, count]), 0);
    }
    process
        .memory
        .protect(0x0040_2000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    process.memory.write(0x0040_2358, &[0xff; 16]).unwrap();
    assert_eq!(call(&mut process, COPY, &[handle, OUTPUT, 2]), 2);
    assert_eq!(output(&process, 12), EXPECTED);
    process
        .memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    assert_eq!(process.last_error().unwrap(), 77);
    assert_eq!(process.memory.mapped_pages(), pages);
}

#[test]
fn missing_resources_and_unsupported_inputs_do_not_publish_handles() {
    let mut process = process();
    for (module, id, error) in [(123, 1, 87), (0x7000_0800, 1, 1812), (0, 2, 1814)] {
        assert_eq!(call(&mut process, LOAD, &[module, id]), 0);
        assert_eq!(process.last_error().unwrap(), error);
    }
    for (api, args) in [(LOAD, &[0, OUTPUT][..]), (COPY, &[0x7700_0004, 0, 0][..])] {
        prepare(&mut process, api, args);
        let before = process.cpu;
        assert_eq!(
            process.run(1).reason,
            ProcessStop::UnsupportedApi { address: api }
        );
        assert_eq!(process.cpu, before);
    }
    process
        .memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    prepare(&mut process, LOAD, &[0, 2]);
    let before = process.cpu;
    assert!(matches!(
        process.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(process.cpu, before);
    assert_eq!(call(&mut process, LOAD, &[0, 1]), 0x7700_0004);
}

#[test]
fn malformed_entries_and_unreadable_sources_leave_identity_available() {
    for entries in [
        vec![],
        vec![[0, 65, 1, 0]],
        vec![[0x180, 65, 1, 0]],
        vec![[0x80, 128, 1, 0]],
        vec![[0x81, 256, 1, 0]],
        vec![[0x80, 65, 1, 0], [0x80, 66, 2, 0]],
        vec![[0, 65, 1, 0], [0xa0, 66, 2, 0]],
    ] {
        let mut process = Process32::load(&accelerator_executable::pe32(&entries), 32).unwrap();
        prepare(&mut process, LOAD, &[0, 1]);
        let before = process.cpu;
        assert_eq!(
            process.run(1).reason,
            ProcessStop::UnsupportedApi { address: LOAD }
        );
        assert_eq!(process.cpu, before);
        if !entries.is_empty() {
            for index in 0..entries.len() {
                let flags: u16 = if index + 1 == entries.len() { 0x81 } else { 1 };
                process
                    .memory
                    .write(0x0040_2358 + index as u64 * 8, &flags.to_le_bytes())
                    .unwrap();
                process
                    .memory
                    .write(0x0040_235a + index as u64 * 8, &65_u16.to_le_bytes())
                    .unwrap();
            }
            assert_eq!(call(&mut process, LOAD, &[0, 1]), 0x7700_0004);
        }
    }
    let mut process = process();
    process
        .memory
        .protect(0x0040_2000, PAGE_SIZE, Permissions::NONE)
        .unwrap();
    prepare(&mut process, LOAD, &[0, 1]);
    let before = process.cpu;
    assert!(matches!(
        process.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(process.cpu, before);
    process
        .memory
        .protect(0x0040_2000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(call(&mut process, LOAD, &[0, 1]), 0x7700_0004);
}

#[test]
fn copy_preflights_the_entire_span_and_incomplete_frames_do_not_run() {
    let mut process = process();
    let handle = call(&mut process, LOAD, &[0, 1]);
    for destination in [0x1000_fffa, u32::MAX - 3] {
        process.memory.write(0x1000_fffa, &[0x55; 6]).unwrap();
        prepare(&mut process, COPY, &[handle, destination, 2]);
        let before = process.cpu;
        assert!(matches!(
            process.run(1).reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!(process.cpu, before);
        let mut bytes = [0; 6];
        process.memory.read(0x1000_fffa, &mut bytes).unwrap();
        assert_eq!(bytes, [0x55; 6]);
    }
    for api in [LOAD, COPY] {
        prepare(&mut process, api, &[0, 1, 0]);
        process.cpu.set_register(Register32::Esp, 0x1000_fffc);
        let before = process.cpu;
        assert_eq!(
            process.run(0).reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
        assert_eq!(process.cpu, before);
        assert!(matches!(
            process.run(1).reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!(process.cpu, before);
    }
}

#[test]
fn relocated_provider_tables_keep_module_identity_separate_from_main() {
    let mut dll = accelerator_executable::pe32(&[[0x81, 66, 999, 0]]);
    for (offset, value) in [
        (0xb4, 0x1000_0000_u32),
        (0xa8, 0),
        (0x100, 0),
        (0x104, 0),
        (0x120, 0x2f00),
        (0x124, 12),
        (0x1300, 0),
        (0x1304, 12),
    ] {
        dll[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    dll[0x96..0x98].copy_from_slice(&0x2102_u16.to_le_bytes());
    let mut process = Process32::load_with_options(
        &accelerator_executable::guest(),
        40,
        ProcessOptions {
            modules: &[GuestModule {
                name: "extra.dll",
                bytes: &dll,
            }],
            ..ProcessOptions::default()
        },
    )
    .unwrap();
    dll.fill(0);
    let main = call(&mut process, LOAD, &[0, 1]);
    let other = call(&mut process, LOAD, &[0x3000_0000, 1]);
    assert_eq!((main, other), (0x7700_0004, 0x7700_0008));
    assert_eq!(call(&mut process, COPY, &[other, OUTPUT, 2]), 1);
    assert_eq!(output(&process, 6), [1, 0, 66, 0, 0xe7, 3]);
    assert_eq!(call(&mut process, COPY, &[main, OUTPUT, 2]), 2);
    assert_eq!(output(&process, 12), EXPECTED);
}

#[test]
fn repeated_loads_and_size_queries_refer_to_the_same_table() {
    let mut process = process();
    let handle = call(&mut process, LOAD, &[0, 1]);
    assert_eq!(handle, 0x7700_0004);
    assert_eq!(call(&mut process, LOAD, &[0x0040_0000, 1]), handle);
    assert_eq!(call(&mut process, COPY, &[handle, 0, u32::MAX]), 2);
    process
        .memory
        .write(u64::from(OUTPUT), &[0x55; 13])
        .unwrap();
    assert_eq!(call(&mut process, COPY, &[handle, OUTPUT, 1]), 1);
    assert_eq!(output(&process, 7), [&EXPECTED[..6], &[0x55]].concat());
    assert_eq!(call(&mut process, COPY, &[handle, OUTPUT, 100]), 2);
    assert_eq!(output(&process, 13), [&EXPECTED[..], &[0x55]].concat());
    prepare(&mut process, COPY, &[handle, STACK, 2]);
    let result = process.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!(process.cpu.eip, 0x0041_0009);
    assert_eq!(process.cpu.register(Register32::Esp), STACK + 16);
    assert_eq!(process.cpu.register(Register32::Eax), 2);
}
