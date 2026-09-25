use ring3_core::execution::{Cpu32, Permissions, Process32, ProcessStop, Register32, StopReason};

pub const API: u32 = 0x7000_01e0;
pub const STACK: u32 = 0x1000_ff00;

pub fn process(value: u32) -> Process32 {
    let mut code = vec![0x68];
    code.extend(value.to_le_bytes());
    code.extend([0xff, 0x15, 0x60, 0x20, 0x40, 0, 0x83, 0xc4, 4, 0xcc]);
    Process32::load(
        &super::imported_executable::pe32(&code, "mSvCrT.dll", &["isspace"]),
        32,
    )
    .unwrap()
}

pub fn word(p: &Process32, address: u32) -> u32 {
    let mut bytes = [0; 4];
    p.memory.read(u64::from(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

pub fn prepare(p: &mut Process32, value: u32) -> Cpu32 {
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu.eflags = 0xced7;
    p.memory
        .write(u64::from(STACK), &0x0040_1000_u32.to_le_bytes())
        .unwrap();
    p.memory
        .write(u64::from(STACK + 4), &value.to_le_bytes())
        .unwrap();
    p.cpu
}

pub fn success(p: &mut Process32, mut expected: Cpu32, result: u32) {
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    expected.eip = 0x0040_1000;
    expected.set_register(Register32::Esp, expected.register(Register32::Esp) + 4);
    expected.set_register(Register32::Eax, result);
    assert_eq!(p.cpu, expected);
}

fn expected(value: u32) -> u32 {
    u32::from([9, 10, 11, 12, 13, 32].contains(&value))
}

pub fn imported_calls_across_budgets() {
    for value in [9, 10, 11, 12, 13, 32, 0, 116, 0x85, 0xa0, 255, u32::MAX] {
        for budget in [1, 3, 30] {
            let mut p = process(value);
            let mut counts = (0, 0);
            loop {
                let run = p.run(budget);
                counts.0 += run.instructions;
                counts.1 += run.api_calls;
                if run.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                    assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                    break;
                }
                assert!(counts.0 + counts.1 < 10);
            }
            assert_eq!(counts, (4, 1));
            assert_eq!(p.cpu.register(Register32::Eax), expected(value));
            assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
            assert_eq!(word(&p, 0x0040_2060), API);
        }
    }
}

pub fn all_bytes_and_eof_preserve_state() {
    let mut p = process(11);
    let pages = p.memory.mapped_pages();
    p.cpu.set_x87_control_word(0x0c7f);
    for register in [
        Register32::Ebx,
        Register32::Ecx,
        Register32::Edx,
        Register32::Esi,
        Register32::Edi,
        Register32::Ebp,
    ] {
        p.cpu.set_register(register, 0x1234_5678);
    }
    p.memory.write(0x7000_2020, &88_u32.to_le_bytes()).unwrap();
    p.memory.write(0x7ffd_e034, &77_u32.to_le_bytes()).unwrap();
    for page in [0x7000_2000, 0x7ffd_e000] {
        p.memory.protect(page, 4096, Permissions::NONE).unwrap();
    }
    for value in (0..=255).chain([u32::MAX]) {
        let before = prepare(&mut p, value);
        assert_eq!(p.run(0).api_calls, 0);
        assert_eq!(p.cpu, before);
        success(&mut p, before, expected(value));
        assert_eq!(word(&p, STACK + 4), value);
    }
    assert_eq!(p.memory.mapped_pages(), pages);
    for page in [0x7000_2000, 0x7ffd_e000] {
        p.memory.protect(page, 4096, Permissions::READ).unwrap();
    }
    assert_eq!(word(&p, 0x7000_2020), 88);
    assert_eq!(p.last_error().unwrap(), 77);
}
