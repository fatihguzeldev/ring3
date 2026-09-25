use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

pub const DATA: u32 = 0x0040_2180;
pub const COMPARATOR: u32 = 0x0040_1100;
pub const COUNTER: u32 = DATA + 128;

fn push(code: &mut Vec<u8>, value: u32) {
    code.push(0x68);
    code.extend_from_slice(&value.to_le_bytes());
}

pub fn process() -> Process32 {
    let mut code = Vec::new();
    for value in [COMPARATOR, 5, 6, DATA] {
        push(&mut code, value);
    }
    code.extend_from_slice(&[0xff, 0x15, 0x60, 0x20, 0x40, 0, 0x83, 0xc4, 16, 0xcc]);
    code.resize(0x100, 0xcc);
    // independently authored signed three-way comparison of packed records.
    code.extend_from_slice(&[
        0x8b, 0x44, 0x24, 4, 0x8b, 0x54, 0x24, 8, 0x8b, 0x48, 1, 0x8b, 0x52, 1, 0x39, 0xd1, 0xb8,
        0, 0, 0, 0, 0x0f, 0x9f, 0xc0, 0x0f, 0x9c, 0xc2, 0x0f, 0xb6, 0xd2, 0x29, 0xd0, 0xff, 0x05,
    ]);
    code.extend_from_slice(&COUNTER.to_le_bytes());
    code.push(0xc3);
    let mut p = Process32::load_diagnostic(
        &super::imported_executable::pe32(&code, "MSVCRT.dll", &["qsort"]),
        96,
    )
    .unwrap();
    for (index, value) in [-3_i32, 7, 0, -3, i32::MIN, i32::MAX]
        .into_iter()
        .enumerate()
    {
        let mut record = [0; 5];
        record[0] = u8::try_from(index).unwrap();
        record[1..].copy_from_slice(&value.to_le_bytes());
        p.memory
            .write(u64::from(DATA) + index as u64 * 5, &record)
            .unwrap();
    }
    p.memory.write(u64::from(COUNTER), &[0; 4]).unwrap();
    p
}

pub fn imported_guest_sort_across_budgets() {
    let mut outcomes = Vec::new();
    for budget in [1, 7, 4096, 20000] {
        let mut p = process();
        let before_stack = p.cpu.register(Register32::Esp);
        for register in [
            Register32::Ebx,
            Register32::Esi,
            Register32::Edi,
            Register32::Ebp,
        ] {
            p.cpu.set_register(register, 0x1234_5678);
        }
        let pages = p.memory.mapped_pages();
        let mut counts = (0, 0);
        loop {
            let run = p.run(budget);
            counts.0 += run.instructions;
            counts.1 += run.api_calls;
            if run.reason == ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert!(counts.0 + counts.1 < 200_000);
                continue;
            }
            assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
            break;
        }
        assert_eq!(p.cpu.register(Register32::Esp), before_stack);
        for register in [
            Register32::Ebx,
            Register32::Esi,
            Register32::Edi,
            Register32::Ebp,
        ] {
            assert_eq!(p.cpu.register(register), 0x1234_5678);
        }
        assert_eq!(p.memory.mapped_pages(), pages);
        let mut records = [0; 30];
        p.memory.read(u64::from(DATA), &mut records).unwrap();
        let values: Vec<_> = records
            .chunks_exact(5)
            .map(|r| i32::from_le_bytes(r[1..].try_into().unwrap()))
            .collect();
        assert_eq!(values, [i32::MIN, -3, -3, 0, 7, i32::MAX]);
        for record in records.chunks_exact(5) {
            let original = [-3_i32, 7, 0, -3, i32::MIN, i32::MAX][usize::from(record[0])];
            assert_eq!(&record[1..], &original.to_le_bytes());
        }
        let mut tags: Vec<_> = records.chunks_exact(5).map(|r| r[0]).collect();
        tags.sort_unstable();
        assert_eq!(tags, [0, 1, 2, 3, 4, 5]);
        let mut counter = [0; 4];
        p.memory.read(u64::from(COUNTER), &mut counter).unwrap();
        assert!(u32::from_le_bytes(counter) > 0);
        outcomes.push((records, counter, counts));
    }
    assert!(outcomes.windows(2).all(|pair| pair[0] == pair[1]));
}
