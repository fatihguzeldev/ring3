use ring3_core::execution::{
    GuestMemory, Permissions, Process32, ProcessStop, Register32, StopReason,
};

use super::{dinput_cooperative_cases, dinput_format_cases};

pub const DATA: u32 = dinput_cooperative_cases::DATA;
pub const WINDOW: u32 = dinput_cooperative_cases::WINDOW;
const ENTRY: u32 = 0x0040_1140;

fn push(code: &mut Vec<u8>, value: u32) {
    code.push(0x68);
    code.extend_from_slice(&value.to_le_bytes());
}

fn store(code: &mut Vec<u8>, address: u32) {
    code.push(0xa3);
    code.extend_from_slice(&address.to_le_bytes());
}

fn code(flags: u32) -> Vec<u8> {
    let mut code = Vec::new();
    for value in [0, DATA, 0x700, 0x0040_0000] {
        push(&mut code, value);
    }
    code.extend_from_slice(&[0xff, 0x15]);
    code.extend_from_slice(&0x0040_2290_u32.to_le_bytes());
    store(&mut code, DATA + 16);
    code.extend_from_slice(&[0x8b, 0x1d]);
    code.extend_from_slice(&DATA.to_le_bytes());
    code.extend_from_slice(&[0x8b, 0x2b]);
    for value in [0, DATA + 4, DATA + 64] {
        push(&mut code, value);
    }
    code.extend_from_slice(&[0x53, 0xff, 0x55, 12]);
    store(&mut code, DATA + 20);
    code.extend_from_slice(&[0x53, 0xff, 0x55, 8]);
    store(&mut code, DATA + 24);
    code.extend_from_slice(&[0x8b, 0x1d]);
    code.extend_from_slice(&(DATA + 4).to_le_bytes());
    code.extend_from_slice(&[0x8b, 0x2b]);
    push(&mut code, dinput_format_cases::FORMAT);
    code.extend_from_slice(&[0x53, 0xff, 0x55, 44]);
    store(&mut code, DATA + 28);
    push(&mut code, flags);
    push(&mut code, WINDOW);
    code.extend_from_slice(&[0x53, 0xff, 0x55, 52]);
    store(&mut code, DATA + 32);
    for (index, slot) in (0..).zip([7, 7, 8, 8, 7, 2]) {
        code.extend_from_slice(&[0x53, 0xff, 0x55, slot * 4]);
        store(&mut code, DATA + 36 + index * 4);
    }
    code.push(0xcc);
    code
}

pub fn process() -> Process32 {
    process_with_format(6, dinput_format_cases::standard)
}

pub fn process_with_format(flags: u32, setup: fn(&mut GuestMemory, bool)) -> Process32 {
    let mut p = dinput_cooperative_cases::process();
    let code = code(flags);
    assert!(code.len() <= 0xc0);
    p.memory
        .protect(0x0040_1000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(u64::from(ENTRY), &code).unwrap();
    p.memory.write(0x0040_1300, &[0xcc]).unwrap();
    p.memory
        .protect(0x0040_1000, 4096, Permissions::READ_EXECUTE)
        .unwrap();
    p.memory
        .map_zeroed(0x3000_0000, 12288, Permissions::READ_WRITE)
        .unwrap();
    setup(&mut p.memory, false);
    p.memory
        .protect(0x3000_0000, 12288, Permissions::READ)
        .unwrap();
    let stack = p.cpu.register(Register32::Esp);
    let frame: Vec<_> = [ENTRY, WINDOW, 5]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    p.memory.write(u64::from(stack - 12), &frame).unwrap();
    p.cpu.set_register(Register32::Esp, stack - 12);
    p.cpu.eip = 0x7000_0440;
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    assert_eq!(p.cpu.register(Register32::Esp), stack);
    assert!(
        p.window_snapshots()
            .iter()
            .any(|window| window.hwnd == WINDOW && window.active)
    );
    p
}

pub fn imported_keyboard_acquisition_across_budgets() {
    imported_acquisition_across_budgets(process);
}

pub fn imported_acquisition_across_budgets(setup: fn() -> Process32) {
    let mut results = Vec::new();
    for budget in [1, 7, 4096, 20000] {
        let mut p = setup();
        let windows = p.window_snapshots();
        let stack = p.cpu.register(Register32::Esp);
        let mut counts = (0, 0);
        loop {
            let run = p.run(budget);
            counts.0 += run.instructions;
            counts.1 += run.api_calls;
            assert!(counts.0 < 1000);
            if run.reason == ProcessStop::Stopped(StopReason::Breakpoint) {
                break;
            }
            assert_eq!(
                run.reason,
                ProcessStop::Stopped(StopReason::InstructionLimit)
            );
        }
        let mut bytes = [0; 44];
        p.memory.read(u64::from(DATA + 16), &mut bytes).unwrap();
        let outputs: Vec<_> = bytes
            .chunks_exact(4)
            .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
            .collect();
        assert_eq!(outputs, [0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0]);
        assert_eq!(p.cpu.register(Register32::Esp), stack);
        assert_eq!(p.window_snapshots(), windows);
        assert_eq!(counts.1, 11);
        results.push((p.cpu, counts, outputs));
    }
    assert!(results.windows(2).all(|pair| pair[0] == pair[1]));
}
