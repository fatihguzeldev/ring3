use std::time::Duration;

use ring3_core::execution::{Permissions, Process32, ProcessStop, Register32, StopReason};

use super::dinput_state_cases::{CODE, DEVICE, OUTPUT, STACK, call, process, words};

pub const API: u32 = 0x7000_05cc;
pub const COUNT: u32 = 0x3100_0f00;
pub const PROPERTY: u32 = 0x3100_0e00;

pub fn buffered(capacity: u32) -> Process32 {
    let mut p = process(true);
    set_buffer(&mut p, DEVICE, capacity);
    assert_eq!(call(&mut p, 0x7000_0584, &[DEVICE]), 0);
    p
}

pub fn set_buffer(p: &mut Process32, device: u32, capacity: u32) {
    words(p, PROPERTY, &[20, 16, 0, 0, capacity]);
    assert_eq!(call(p, 0x7000_0580, &[device, 1, PROPERTY]), 0);
}

pub fn records(p: &Process32, address: u32, count: usize) -> Vec<[u32; 4]> {
    let mut bytes = vec![0; count * 16];
    p.memory.read(u64::from(address), &mut bytes).unwrap();
    bytes
        .chunks_exact(16)
        .map(|record| {
            std::array::from_fn(|i| {
                u32::from_le_bytes(record[i * 4..i * 4 + 4].try_into().unwrap())
            })
        })
        .collect()
}

pub fn count(p: &Process32) -> u32 {
    let mut bytes = [0; 4];
    p.memory.read(u64::from(COUNT), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

pub fn read(p: &mut Process32, device: u32, output: u32, requested: u32, flags: u32) -> u32 {
    words(p, COUNT, &[requested]);
    call(p, API, &[device, 16, output, COUNT, flags])
}

fn append_read(code: &mut Vec<u8>, output: u32, flags: u32) {
    for value in [flags, COUNT, output, 16] {
        code.push(0x68);
        code.extend_from_slice(&value.to_le_bytes());
    }
    code.extend_from_slice(&[0x53, 0xff, 0x55, 40, 0xcc]);
}

fn phase(p: &mut Process32, budget: u64) -> (u64, u64) {
    let mut counts = (0, 0);
    loop {
        let result = p.run(budget);
        counts.0 += result.instructions;
        counts.1 += result.api_calls;
        assert!(counts.0 < 1000);
        if result.reason == ProcessStop::Stopped(StopReason::Breakpoint) {
            break;
        }
        assert_eq!(
            result.reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
    }
    assert_eq!(p.cpu.register(Register32::Eax), 0);
    assert_eq!(p.cpu.register(Register32::Esp), STACK);
    counts
}

pub fn keyboard_events_across_budgets() {
    let mut results = Vec::new();
    for budget in [1, 5, 100] {
        let mut p = buffered(16);
        let mut code = vec![0xbb];
        code.extend_from_slice(&DEVICE.to_le_bytes());
        code.extend_from_slice(&[0x8b, 0x2b]);
        for (index, flags) in [(0, 1), (1, 0), (2, 0)] {
            append_read(&mut code, OUTPUT + index * 64, flags);
        }
        p.memory
            .protect(0x0040_1000, 4096, Permissions::READ_WRITE)
            .unwrap();
        p.memory.write(u64::from(CODE), &code).unwrap();
        p.memory
            .protect(0x0040_1000, 4096, Permissions::READ_EXECUTE)
            .unwrap();
        p.cpu.eip = CODE;
        p.cpu.set_register(Register32::Esp, STACK);
        p.set_elapsed_time(Duration::from_millis(1234)).unwrap();
        let mut keys = [false; 256];
        keys[0x1e] = true;
        keys[0x30] = true;
        p.set_keyboard_state(keys).unwrap();
        p.set_keyboard_state(keys).unwrap();
        words(&mut p, COUNT, &[16]);
        let first = phase(&mut p, budget);
        assert_eq!(count(&p), 2);
        assert_eq!(
            records(&p, OUTPUT, 2),
            [[0x1e, 0x80, 1234, 1], [0x30, 0x80, 1234, 1]]
        );
        words(&mut p, COUNT, &[1]);
        let second = phase(&mut p, budget);
        assert_eq!(count(&p), 1);
        assert_eq!(records(&p, OUTPUT + 64, 1), [[0x1e, 0x80, 1234, 1]]);
        p.set_elapsed_time(Duration::from_secs(2)).unwrap();
        keys[0x1e] = false;
        keys[0x20] = true;
        p.set_keyboard_state(keys).unwrap();
        words(&mut p, COUNT, &[u32::MAX]);
        let third = phase(&mut p, budget);
        assert_eq!(count(&p), 3);
        let last = records(&p, OUTPUT + 128, 3);
        assert_eq!(
            last,
            [
                [0x30, 0x80, 1234, 1],
                [0x1e, 0, 2000, 2],
                [0x20, 0x80, 2000, 2]
            ]
        );
        assert_eq!(read(&mut p, DEVICE, 0x5000_0000, 16, 0), 0);
        assert_eq!(count(&p), 0);
        results.push((p.cpu, first, second, third, last, p.memory.mapped_pages()));
    }
    assert!(results.windows(2).all(|pair| pair[0] == pair[1]));
}
