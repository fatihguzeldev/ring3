use super::imported_executable;

use ring3_core::execution::{
    Cpu32, PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const API: u32 = 0x7000_0128;
const BEGIN: u32 = 0x0040_2180;
const END: u32 = BEGIN + 4;
const STACK: u32 = 0x1000_ff00;

fn process(limit: u32) -> Process32 {
    Process32::load(
        &imported_executable::pe32(&[0xcc], "MSVCRT.dll", &["__dllonexit", "malloc", "free"]),
        limit,
    )
    .unwrap()
}

fn write(process: &mut Process32, address: u32, value: u32) {
    process
        .memory
        .write(u64::from(address), &value.to_le_bytes())
        .unwrap();
}

fn read(process: &Process32, address: u32) -> u32 {
    let mut bytes = [0; 4];
    process.memory.read(u64::from(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

fn prepare(process: &mut Process32, api: u32, args: &[u32]) -> Cpu32 {
    process.cpu.eip = api;
    process.cpu.set_register(Register32::Esp, STACK);
    process.cpu.set_register(Register32::Eax, 99);
    process.cpu.eflags = 0xced7;
    for (index, value) in std::iter::once(&0x0040_1000).chain(args).enumerate() {
        write(process, STACK + u32::try_from(index).unwrap() * 4, *value);
    }
    process.cpu
}

fn call(process: &mut Process32, api: u32, args: &[u32]) -> u32 {
    let before = prepare(process, api, args);
    let result = process.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    let value = process.cpu.register(Register32::Eax);
    let mut expected = before;
    expected.eip = 0x0040_1000;
    expected.set_register(Register32::Esp, STACK + 4);
    expected.set_register(Register32::Eax, value);
    assert_eq!(process.cpu, expected);
    value
}

fn table(process: &mut Process32, used: u32) -> u32 {
    let pointer = call(process, 0x7000_011c, &[4096]);
    assert_ne!(pointer, 0);
    write(process, BEGIN, pointer);
    write(process, END, pointer + used);
    pointer
}

#[test]
fn appends_opaque_functions_and_grows_a_real_freeable_guest_table() {
    let mut process = process(28);
    let pages = process.memory.mapped_pages();
    let old = table(&mut process, 0);
    write(&mut process, 0x7000_2020, 123);
    write(&mut process, 0x7ffd_e034, 77);
    for (index, function) in [0x0040_1000, 0, 0xffff_ffff].into_iter().enumerate() {
        assert_eq!(call(&mut process, API, &[function, BEGIN, END]), function);
        assert_eq!(
            read(&process, old + u32::try_from(index).unwrap() * 4),
            function
        );
    }
    for index in 3..1024 {
        write(&mut process, old + index * 4, index);
    }
    write(&mut process, END, old + 4096);
    assert_eq!(call(&mut process, API, &[42, BEGIN, END]), 42);
    let new = read(&process, BEGIN);
    assert_ne!(new, old);
    assert_eq!(read(&process, END), new + 4100);
    assert_eq!(read(&process, new), 0x0040_1000);
    assert_eq!(read(&process, new + 4), 0);
    assert_eq!(read(&process, new + 8), u32::MAX);
    for index in 3..1024 {
        assert_eq!(read(&process, new + index * 4), index);
    }
    assert_eq!(read(&process, new + 4096), 42);
    assert!(process.memory.read(u64::from(old), &mut [0]).is_err());
    assert_eq!(process.memory.mapped_pages(), pages + 2);
    assert_eq!(read(&process, 0x7000_2020), 123);
    assert_eq!(process.last_error().unwrap(), 77);
    assert_eq!(call(&mut process, 0x7000_0120, &[new]), 99);
    assert_eq!(process.memory.mapped_pages(), pages);
}

#[test]
fn malformed_ownership_ranges_and_overlaps_are_atomic() {
    let mut process = process(28);
    let pointer = table(&mut process, 0);
    let pages = process.memory.mapped_pages();
    for (begin, end, begin_cell, end_cell) in [
        (pointer + 4, pointer + 4, BEGIN, END),
        (pointer, pointer - 4, BEGIN, END),
        (pointer, pointer + 1, BEGIN, END),
        (pointer, pointer + 4100, BEGIN, END),
        (pointer, u32::MAX, BEGIN, END),
        (pointer, pointer, BEGIN, BEGIN),
        (pointer, pointer, BEGIN, BEGIN + 2),
        (pointer, pointer, pointer, END),
        (pointer, pointer, BEGIN, pointer + 16),
        (0x0040_2180, 0x0040_2180, BEGIN, END),
    ] {
        write(&mut process, end_cell, end);
        write(&mut process, begin_cell, begin);
        let before = prepare(&mut process, API, &[42, begin_cell, end_cell]);
        let first = read(&process, pointer);
        assert_eq!(
            process.run(1).reason,
            ProcessStop::UnsupportedApi { address: API }
        );
        assert_eq!(process.cpu, before);
        assert_eq!(process.memory.mapped_pages(), pages);
        assert_eq!(read(&process, pointer), first);
    }
}

#[test]
fn output_source_and_errno_faults_preserve_table_and_capacity() {
    for (limit, output_fault, source_fault, errno_fault) in [
        (28, true, false, false),
        (28, false, true, false),
        (26, false, false, true),
        (26, false, false, false),
    ] {
        let mut process = process(limit);
        let pointer = table(&mut process, 4096);
        write(&mut process, pointer, 123);
        write(&mut process, 0x7ffd_e034, 77);
        let pages = process.memory.mapped_pages();
        if output_fault {
            process
                .memory
                .protect(0x0040_2000, PAGE_SIZE, Permissions::READ)
                .unwrap();
        }
        if source_fault {
            process
                .memory
                .protect(u64::from(pointer), PAGE_SIZE, Permissions::NONE)
                .unwrap();
        }
        if errno_fault {
            process
                .memory
                .protect(0x7000_2000, PAGE_SIZE, Permissions::NONE)
                .unwrap();
        }
        let before = prepare(&mut process, API, &[42, BEGIN, END]);
        let result = process.run(1);
        if output_fault || source_fault || errno_fault {
            assert!(matches!(
                result.reason,
                ProcessStop::Stopped(StopReason::MemoryFault(_))
            ));
            assert_eq!(process.cpu, before);
            assert_eq!(result.api_calls, 0);
        } else {
            assert_eq!(
                result.reason,
                ProcessStop::Stopped(StopReason::InstructionLimit)
            );
            assert_eq!(process.cpu.register(Register32::Eax), 0);
            assert_eq!(read(&process, 0x7000_2020), 12);
        }
        assert_eq!(process.memory.mapped_pages(), pages);
        assert_eq!(read(&process, BEGIN), pointer);
        assert_eq!(read(&process, END), pointer + 4096);
        process
            .memory
            .protect(u64::from(pointer), PAGE_SIZE, Permissions::READ_WRITE)
            .unwrap();
        assert_eq!(read(&process, pointer), 123);
        assert_eq!(process.last_error().unwrap(), 77);
    }
}

#[test]
fn null_tables_frame_validation_and_captured_return_alias_are_explicit() {
    let mut process = process(28);
    assert_eq!(call(&mut process, API, &[42, 0, u32::MAX]), 0);
    assert_eq!(call(&mut process, API, &[42, BEGIN, END]), 0);
    let pointer = table(&mut process, 0);
    prepare(&mut process, API, &[42, BEGIN, END]);
    process.cpu.set_register(Register32::Esp, 0x1000_fff4);
    let before = process.cpu;
    assert!(matches!(
        process.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(process.cpu, before);
    let before = prepare(&mut process, API, &[42, BEGIN, END]);
    process.cpu.set_register(Register32::Esp, pointer);
    for (i, value) in [0x0040_1000, 42, BEGIN, END].into_iter().enumerate() {
        write(&mut process, pointer + u32::try_from(i).unwrap() * 4, value);
    }
    assert_eq!(
        process.run(1).reason,
        ProcessStop::UnsupportedApi { address: API }
    );
    process.cpu = before;
    prepare(&mut process, API, &[42, STACK, END]);
    write(&mut process, STACK, pointer);
    let result = process.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!(process.cpu.eip, pointer);
    assert_eq!(process.cpu.register(Register32::Eax), 42);
    assert_eq!(read(&process, END), pointer + 4);
    assert_eq!(read(&process, pointer), 42);
}

#[test]
fn error_cells_follow_normal_guest_output_aliases() {
    let mut full = process(26);
    let pointer = table(&mut full, 4096);
    write(&mut full, 0x7000_2020, pointer);
    write(&mut full, pointer, 123);
    assert_eq!(call(&mut full, API, &[42, 0x7000_2020, END]), 0);
    assert_eq!(read(&full, 0x7000_2020), 12);
    assert_eq!(read(&full, END), pointer + 4096);
    assert_eq!(read(&full, pointer), 123);
    assert_eq!(full.memory.mapped_pages(), 26);
    assert_eq!(call(&mut full, 0x7000_0120, &[pointer]), 99);

    let mut spare = process(26);
    let pointer = table(&mut spare, 0);
    write(&mut spare, 0x7ffd_e034, pointer);
    assert_eq!(call(&mut spare, API, &[42, BEGIN, 0x7ffd_e034]), 42);
    assert_eq!(spare.last_error().unwrap(), pointer + 4);
    assert_eq!(read(&spare, pointer), 42);
    spare
        .memory
        .protect(u64::from(pointer), PAGE_SIZE, Permissions::READ)
        .unwrap();
    let before = prepare(&mut spare, API, &[43, BEGIN, 0x7ffd_e034]);
    assert!(matches!(
        spare.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(spare.cpu, before);
    assert_eq!(spare.last_error().unwrap(), pointer + 4);
    assert_eq!(read(&spare, pointer), 42);
}
