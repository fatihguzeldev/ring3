#[path = "support/crt_sort_cases.rs"]
mod crt_sort_cases;
#[path = "support/imported_executable.rs"]
mod imported_executable;

#[test]
fn imported_sort_calls_guest_comparator_and_matches_across_budgets() {
    crt_sort_cases::imported_guest_sort_across_budgets();
}

use crt_sort_cases::{COMPARATOR, DATA};
use ring3_core::execution::{
    Access, Cpu32, MemoryError, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const SORT: u32 = 0x7000_01cc;
const STACK: u32 = 0x1000_ee00;
const STOP: u32 = 0x0040_1080;
const ARRAY: u32 = 0x3000_0000;
const BYTE_COMPARE: &[u8] = &[
    0x8b, 0x44, 0x24, 4, 0x8b, 0x54, 0x24, 8, 0x0f, 0xb6, 0x00, 0x0f, 0xb6, 0x12, 0x29, 0xd0, 0xc3,
];

fn write_words(p: &mut Process32, at: u32, values: &[u32]) {
    let bytes: Vec<_> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
    p.memory.write(u64::from(at), &bytes).unwrap();
}

fn code(p: &mut Process32, at: u32, bytes: &[u8]) {
    p.memory
        .protect(0x0040_1000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(u64::from(at), bytes).unwrap();
    p.memory
        .protect(0x0040_1000, 4096, Permissions::READ_EXECUTE)
        .unwrap();
}

fn process() -> Process32 {
    let mut p = crt_sort_cases::process();
    p.memory
        .map_zeroed(u64::from(ARRAY), 0x10000, Permissions::READ_WRITE)
        .unwrap();
    code(&mut p, COMPARATOR, BYTE_COMPARE);
    p
}

fn prepare(p: &mut Process32, args: [u32; 4]) -> Cpu32 {
    write_words(p, STACK, &[STOP, args[0], args[1], args[2], args[3]]);
    p.cpu.eip = SORT;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu
}

fn finish(p: &mut Process32, budget: u64) -> (u64, u64) {
    let mut counts = (0, 0);
    loop {
        let run = p.run(budget);
        counts.0 += run.instructions;
        counts.1 += run.api_calls;
        assert!(counts.0 + counts.1 < 2_000_000);
        if run.reason == ProcessStop::Stopped(StopReason::InstructionLimit) {
            continue;
        }
        assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
        return counts;
    }
}

fn bytes(p: &Process32, at: u32, length: usize) -> Vec<u8> {
    let mut data = vec![0; length];
    p.memory.read(u64::from(at), &mut data).unwrap();
    data
}

#[test]
fn generic_widths_orders_and_equal_keys_preserve_whole_records_and_boundaries() {
    for width in [1_usize, 5, 17, 1024] {
        for keys in [
            vec![],
            vec![7],
            vec![9, 1],
            vec![1, 2, 3, 4],
            vec![4, 3, 2, 1],
            vec![3, 1, 3, 0, 255, 1],
        ] {
            let mut p = process();
            let records: Vec<_> = keys
                .iter()
                .enumerate()
                .map(|(i, key)| {
                    let mut record = vec![u8::try_from(i).unwrap(); width];
                    record[0] = *key;
                    record
                })
                .collect();
            let original: Vec<_> = records.iter().flatten().copied().collect();
            p.memory.write(u64::from(ARRAY), &[0xa5; 8]).unwrap();
            p.memory.write(u64::from(ARRAY + 8), &original).unwrap();
            let end = ARRAY + 8 + u32::try_from(original.len()).unwrap();
            p.memory.write(u64::from(end), &[0x5a; 8]).unwrap();
            let pages = p.memory.mapped_pages();
            prepare(
                &mut p,
                [
                    ARRAY + 8,
                    u32::try_from(keys.len()).unwrap(),
                    u32::try_from(width).unwrap(),
                    COMPARATOR,
                ],
            );
            assert_eq!(finish(&mut p, 7).1, 1);
            assert_eq!(p.cpu.register(Register32::Esp), STACK + 4);
            assert_eq!(p.cpu.register(Register32::Eax), 0);
            assert_eq!(p.memory.mapped_pages(), pages);
            assert_eq!(bytes(&p, ARRAY, 8), [0xa5; 8]);
            assert_eq!(bytes(&p, end, 8), [0x5a; 8]);
            let sorted = bytes(&p, ARRAY + 8, original.len());
            let mut actual: Vec<_> = sorted.chunks_exact(width).map(<[u8]>::to_vec).collect();
            assert!(actual.windows(2).all(|pair| pair[0][0] <= pair[1][0]));
            let mut expected = records;
            actual.sort();
            expected.sort();
            assert_eq!(actual, expected);
        }
    }
}

#[test]
fn array_can_end_at_the_32_bit_boundary_without_a_wrapped_end_pointer() {
    let mut p = process();
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    let at = 0xffff_ffec;
    let input = [
        4, 11, 12, 13, 14, 1, 21, 22, 23, 24, 3, 31, 32, 33, 34, 2, 41, 42, 43, 44,
    ];
    p.memory.write(u64::from(at), &input).unwrap();
    prepare(&mut p, [at, 4, 5, COMPARATOR]);
    finish(&mut p, 1);
    assert_eq!(
        bytes(&p, at, 20),
        [
            1, 21, 22, 23, 24, 2, 41, 42, 43, 44, 3, 31, 32, 33, 34, 4, 11, 12, 13, 14
        ]
    );
}

fn refused(p: &mut Process32, expected: &ProcessStop) {
    let cpu = p.cpu;
    let data = bytes(p, ARRAY, 32);
    let workspace = bytes(p, STACK - 48, 68);
    for _ in 0..2 {
        let run = p.run(1);
        assert_eq!(&run.reason, expected);
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, cpu);
        assert_eq!(bytes(p, ARRAY, 32), data);
        assert_eq!(bytes(p, STACK - 48, 68), workspace);
    }
}

#[test]
fn scalar_span_and_stack_overlap_failures_preserve_entry_and_recover() {
    let mut p = process();
    p.memory.write(u64::from(ARRAY), &[3, 2, 1]).unwrap();
    for args in [
        [0, 1, 1, COMPARATOR],
        [ARRAY, 2, 0, COMPARATOR],
        [ARRAY, 2, 1, 0],
        [ARRAY, 16 * 1024 * 1024 + 1, 1, COMPARATOR],
        [STACK - 1, 2, 1, COMPARATOR],
        [STACK + 16, 2, 5, COMPARATOR],
    ] {
        prepare(&mut p, args);
        refused(&mut p, &ProcessStop::UnsupportedApi { address: SORT });
    }
    for args in [
        [ARRAY, u32::MAX, 2, COMPARATOR],
        [0xffff_fff8, 3, 5, COMPARATOR],
    ] {
        prepare(&mut p, args);
        refused(
            &mut p,
            &ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::AddressOverflow)),
        );
    }
    prepare(&mut p, [ARRAY, 3, 1, COMPARATOR]);
    finish(&mut p, 7);
    assert_eq!(bytes(&p, ARRAY, 3), [1, 2, 3]);
}

#[test]
fn array_and_comparator_permissions_are_checked_before_any_sorting() {
    let mut p = process();
    p.memory.write(u64::from(ARRAY), &[3, 2, 1]).unwrap();
    p.memory
        .protect(u64::from(ARRAY), 4096, Permissions::READ)
        .unwrap();
    prepare(&mut p, [ARRAY, 3, 1, COMPARATOR]);
    refused(
        &mut p,
        &ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::PermissionDenied {
            address: u64::from(ARRAY),
            access: Access::Write,
        })),
    );
    p.memory
        .protect(u64::from(ARRAY), 4096, Permissions::READ_WRITE)
        .unwrap();
    prepare(&mut p, [ARRAY, 3, 1, DATA]);
    refused(
        &mut p,
        &ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::PermissionDenied {
            address: u64::from(DATA),
            access: Access::Execute,
        })),
    );
    prepare(&mut p, [ARRAY, 3, 1, COMPARATOR]);
    finish(&mut p, 4096);
    assert_eq!(bytes(&p, ARRAY, 3), [1, 2, 3]);
}

fn push(code: &mut Vec<u8>, value: u32) {
    code.push(0x68);
    code.extend_from_slice(&value.to_le_bytes());
}

#[test]
fn comparator_can_call_a_provider_and_sort_a_separate_array_recursively() {
    let mut p = process();
    let flag = DATA + 160;
    let nested = ARRAY + 1024;
    let second = COMPARATOR + 128;
    let mut comparator = vec![0x80, 0x3d];
    comparator.extend_from_slice(&flag.to_le_bytes());
    comparator.extend_from_slice(&[0, 0x75, 0, 0xc6, 0x05]);
    comparator.extend_from_slice(&flag.to_le_bytes());
    comparator.push(1);
    for value in [second, 1, 3, nested] {
        push(&mut comparator, value);
    }
    comparator.extend_from_slice(&[0xb8]);
    comparator.extend_from_slice(&SORT.to_le_bytes());
    comparator.extend_from_slice(&[0xff, 0xd0, 0x83, 0xc4, 16]);
    comparator[8] = u8::try_from(comparator.len() - 9).unwrap();
    comparator.extend_from_slice(&[0xb8, 0x24, 0x01, 0, 0x70, 0xff, 0xd0]);
    comparator.extend_from_slice(BYTE_COMPARE);
    code(&mut p, COMPARATOR, &comparator);
    code(&mut p, second, BYTE_COMPARE);
    p.memory.write(u64::from(flag), &[0]).unwrap();
    p.memory.write(u64::from(ARRAY), &[3, 2, 1]).unwrap();
    p.memory.write(u64::from(nested), &[4, 9, 2]).unwrap();
    prepare(&mut p, [ARRAY, 3, 1, COMPARATOR]);
    let counts = finish(&mut p, 1);
    assert!(counts.1 > 2);
    assert_eq!(bytes(&p, ARRAY, 3), [1, 2, 3]);
    assert_eq!(bytes(&p, nested, 3), [2, 4, 9]);
    assert_eq!(bytes(&p, flag, 1), [1]);
    assert_eq!(p.cpu.register(Register32::Esp), STACK + 4);
}

#[test]
fn comparator_fault_breakpoint_and_partial_byte_swap_resume_on_the_same_stack() {
    let mut p = process();
    let original: Vec<_> = [3, 2, 1]
        .into_iter()
        .flat_map(|value| [value; 64])
        .collect();
    p.memory.write(u64::from(ARRAY), &original).unwrap();
    let before = prepare(&mut p, [ARRAY, 3, 64, COMPARATOR]);
    assert_eq!(p.run(0).api_calls, 0);
    assert_eq!(p.cpu, before);
    for _ in 0..200 {
        if p.cpu.eip == COMPARATOR {
            break;
        }
        assert_eq!(
            p.run(1).reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
    }
    assert_eq!(p.cpu.eip, COMPARATOR);
    assert_eq!(bytes(&p, ARRAY, original.len()), original);
    p.memory
        .protect(0x0040_1000, 4096, Permissions::READ)
        .unwrap();
    let stopped = p.cpu;
    assert_eq!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::PermissionDenied {
            address: u64::from(COMPARATOR),
            access: Access::Execute
        }))
    );
    assert_eq!(p.cpu, stopped);
    let mut breakpoint = vec![0xcc];
    breakpoint.extend_from_slice(BYTE_COMPARE);
    code(&mut p, COMPARATOR, &breakpoint);
    assert_eq!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    let mut resumed = stopped;
    resumed.eip += 1;
    assert_eq!(p.cpu, resumed);
    breakpoint[0] = 0x90;
    code(&mut p, COMPARATOR, &breakpoint);
    let mut partial = None;
    for _ in 0..500 {
        assert_eq!(
            p.run(1).reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
        let current = bytes(&p, ARRAY, original.len());
        if current
            .chunks_exact(64)
            .any(|record| record.iter().any(|byte| *byte != record[0]))
        {
            partial = Some((p.cpu, current));
            break;
        }
    }
    let (cpu, partial) = partial.expect("byte swap must be interruptible");
    p.run(0);
    assert_eq!(p.cpu, cpu);
    assert_eq!(bytes(&p, ARRAY, original.len()), partial);
    finish(&mut p, 7);
    let expected: Vec<_> = [1, 2, 3]
        .into_iter()
        .flat_map(|value| [value; 64])
        .collect();
    assert_eq!(bytes(&p, ARRAY, expected.len()), expected);
}

#[test]
fn zero_singleton_and_regular_sorts_do_not_access_teb_or_allocate_pages() {
    let mut p = process();
    write_words(&mut p, 0x7ffd_e034, &[77]);
    write_words(&mut p, 0x7ffd_e040, &[88]);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::NONE)
        .unwrap();
    let pages = p.memory.mapped_pages();
    for args in [
        [0, 0, 1, 1],
        [0x5000_0000, 1, 8, 1],
        [ARRAY, 3, 1, COMPARATOR],
    ] {
        p.memory.write(u64::from(ARRAY), &[3, 2, 1]).unwrap();
        prepare(&mut p, args);
        finish(&mut p, 7);
        assert_eq!(p.cpu.register(Register32::Eax), 0);
        assert_eq!(p.memory.mapped_pages(), pages);
    }
    assert_eq!(bytes(&p, ARRAY, 3), [1, 2, 3]);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(p.last_error().unwrap(), 77);
    assert_eq!(bytes(&p, 0x7ffd_e040, 4), 88_u32.to_le_bytes());
}

#[test]
fn incomplete_frames_and_missing_helper_workspace_fail_before_transfer_and_recover() {
    let mut p = process();
    p.memory.write(u64::from(ARRAY), &[3, 2, 1]).unwrap();
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    prepare(&mut p, [ARRAY, 3, 1, COMPARATOR]);
    write_words(&mut p, 0xffff_fff0, &[STOP, ARRAY, 3, 1]);
    p.cpu.set_register(Register32::Esp, 0xffff_fff0);
    refused(
        &mut p,
        &ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::AddressOverflow)),
    );
    p.memory
        .map_zeroed(0, 4096, Permissions::READ_WRITE)
        .unwrap();
    write_words(&mut p, 0x10, &[STOP, ARRAY, 3, 1, COMPARATOR]);
    p.cpu.set_register(Register32::Esp, 0x10);
    refused(
        &mut p,
        &ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::AddressOverflow)),
    );
    p.memory
        .map_zeroed(0x4000_0000, 4096, Permissions::READ_WRITE)
        .unwrap();
    write_words(&mut p, 0x4000_0010, &[STOP, ARRAY, 3, 1, COMPARATOR]);
    p.cpu.set_register(Register32::Esp, 0x4000_0010);
    refused(
        &mut p,
        &ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::Unmapped {
            address: 0x3fff_ffe0,
        })),
    );
    p.memory
        .map_zeroed(0x3fff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    finish(&mut p, 7);
    assert_eq!(bytes(&p, ARRAY, 3), [1, 2, 3]);
    assert_eq!(p.cpu.register(Register32::Esp), 0x4000_0014);
}

#[test]
fn scheduled_child_executes_sort_with_its_own_cdecl_stack() {
    let mut p = crt_sort_cases::process();
    prepare(&mut p, [0, 0, 0, 0]);
    write_words(&mut p, STACK, &[STOP, 0, 0, 0x0040_1000, 0, 4, 0]);
    p.cpu.eip = 0x7000_0548;
    assert_eq!(p.run(1).api_calls, 1);
    let child = p.cpu.register(Register32::Eax);
    assert_ne!(child, 0);
    write_words(&mut p, STACK, &[STOP, child]);
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.eip = 0x7000_0550;
    assert_eq!(p.run(1).api_calls, 1);
    let run = p.run(20000);
    assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!(p.cpu.fs_base(), 0x1101_0000);
    assert_eq!(run.api_calls, 1);
    let records = bytes(&p, DATA, 30);
    let values: Vec<_> = records
        .chunks_exact(5)
        .map(|r| i32::from_le_bytes(r[1..].try_into().unwrap()))
        .collect();
    assert_eq!(values, [i32::MIN, -3, -3, 0, 7, i32::MAX]);
}

#[test]
fn missing_array_tail_can_be_mapped_and_retried_without_a_partial_sort() {
    let mut p = process();
    let at = ARRAY + 0xfffe;
    p.memory.write(u64::from(at), &[3, 2]).unwrap();
    prepare(&mut p, [at, 3, 1, COMPARATOR]);
    refused(
        &mut p,
        &ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::Unmapped {
            address: u64::from(ARRAY + 0x10000),
        })),
    );
    assert_eq!(bytes(&p, at, 2), [3, 2]);
    p.memory
        .map_zeroed(u64::from(ARRAY + 0x10000), 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(u64::from(ARRAY + 0x10000), &[1]).unwrap();
    finish(&mut p, 1);
    assert_eq!(bytes(&p, at, 3), [1, 2, 3]);
}
