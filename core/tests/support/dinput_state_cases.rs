use ring3_core::execution::{Cpu32, Permissions, Process32, ProcessStop, Register32, StopReason};

use super::{dinput_acquire_cases, dinput_format_cases};

pub const DEVICE: u32 = 0x7001_7900;
pub const API: u32 = 0x7000_05c8;
pub const OUTPUT: u32 = 0x3100_0009;
pub const STACK: u32 = 0x1000_ef00;
pub const CODE: u32 = 0x0040_1300;

pub fn words(p: &mut Process32, address: u32, values: &[u32]) {
    let bytes: Vec<_> = values.iter().flat_map(|word| word.to_le_bytes()).collect();
    p.memory.write(u64::from(address), &bytes).unwrap();
}

pub fn prepare(p: &mut Process32, api: u32, args: &[u32]) -> Cpu32 {
    p.cpu.eip = api;
    p.cpu.set_register(Register32::Esp, STACK);
    words(p, STACK, &[CODE]);
    words(p, STACK + 4, args);
    p.cpu
}

pub fn call(p: &mut Process32, api: u32, args: &[u32]) -> u32 {
    prepare(p, api, args);
    let result = p.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    assert_eq!(
        p.cpu.register(Register32::Esp),
        STACK + 4 * u32::try_from(args.len() + 1).unwrap()
    );
    p.cpu.register(Register32::Eax)
}

pub fn process(configured: bool) -> Process32 {
    let mut p = dinput_acquire_cases::process();
    let data = dinput_acquire_cases::DATA;
    assert_eq!(call(&mut p, 0x7000_0558, &[1, 0x700, data, 0]), 0);
    assert_eq!(
        call(&mut p, 0x7000_0568, &[0x7001_7800, data + 64, data, 0]),
        0
    );
    if configured {
        assert_eq!(
            call(&mut p, 0x7000_0578, &[DEVICE, dinput_format_cases::FORMAT]),
            0
        );
        assert_eq!(
            call(
                &mut p,
                0x7000_057c,
                &[DEVICE, dinput_acquire_cases::WINDOW, 6]
            ),
            0
        );
    }
    p.memory
        .map_zeroed(0x3100_0000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p
}

fn run_to_break(p: &mut Process32, budget: u64) -> (u64, u64) {
    let mut counts = (0, 0);
    loop {
        let result = p.run(budget);
        counts.0 += result.instructions;
        counts.1 += result.api_calls;
        assert!(counts.0 < 1000);
        if result.reason == ProcessStop::Stopped(StopReason::Breakpoint) {
            return counts;
        }
        assert_eq!(
            result.reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
    }
}

fn append_read(code: &mut Vec<u8>, output: u32) {
    for value in [output, 256] {
        code.push(0x68);
        code.extend_from_slice(&value.to_le_bytes());
    }
    code.extend_from_slice(&[0x53, 0xff, 0x55, 36]);
}

pub fn keyboard_snapshots_across_budgets() {
    let mut results = Vec::new();
    for budget in [1, 5, 100] {
        let mut p = process(true);
        let mut slot = [0; 4];
        p.memory.read(0x7001_7224, &mut slot).unwrap();
        assert_eq!(u32::from_le_bytes(slot), API);
        let mut code = vec![0xbb];
        code.extend_from_slice(&DEVICE.to_le_bytes());
        code.extend_from_slice(&[0x8b, 0x2b, 0x53, 0xff, 0x55, 28]);
        append_read(&mut code, OUTPUT);
        append_read(&mut code, OUTPUT + 256);
        code.push(0xcc);
        append_read(&mut code, OUTPUT + 512);
        code.extend_from_slice(&[0x53, 0xff, 0x55, 8, 0xcc]);
        p.memory
            .protect(0x0040_1000, 4096, Permissions::READ_WRITE)
            .unwrap();
        p.memory.write(u64::from(CODE), &code).unwrap();
        p.memory
            .protect(0x0040_1000, 4096, Permissions::READ_EXECUTE)
            .unwrap();
        p.cpu.eip = CODE;
        p.cpu.set_register(Register32::Esp, STACK);
        let mut keys = [false; 256];
        for key in [0x01, 0x1e, 0x9c, 0xdb, 0xff] {
            keys[key] = true;
        }
        let before = p.cpu;
        p.set_keyboard_state(keys).unwrap();
        assert_eq!(p.cpu, before);
        let first = run_to_break(&mut p, budget);
        keys[0x1e] = false;
        keys[0x20] = true;
        p.set_keyboard_state(keys).unwrap();
        let second = run_to_break(&mut p, budget);
        let mut output = [0; 768];
        p.memory.read(u64::from(OUTPUT), &mut output).unwrap();
        for key in 0..256 {
            let initial = u8::from([0x01, 0x1e, 0x9c, 0xdb, 0xff].contains(&key)) << 7;
            assert_eq!(output[key], initial);
            assert_eq!(output[key + 256], initial);
            assert_eq!(output[key + 512], u8::from(keys[key]) << 7);
        }
        assert_eq!(p.cpu.register(Register32::Esp), STACK);
        assert_eq!(first.1 + second.1, 5);
        results.push((p.cpu, first, second, output, p.memory.mapped_pages()));
    }
    assert!(results.windows(2).all(|pair| pair[0] == pair[1]));
}
