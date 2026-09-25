use super::lowercase_executable;

use ring3_core::execution::{Cpu32, Permissions, Process32, ProcessStop, Register32, StopReason};

const API: u32 = 0x7000_017c;
const STACK: u32 = 0x1000_ff00;

fn process() -> Process32 {
    Process32::load(&lowercase_executable::pe32(), 32).unwrap()
}

fn prepare(p: &mut Process32, character: u32) -> Cpu32 {
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu.eflags = 0xced7;
    p.memory
        .write(u64::from(STACK), &0x0040_1000_u32.to_le_bytes())
        .unwrap();
    p.memory
        .write(u64::from(STACK + 4), &character.to_le_bytes())
        .unwrap();
    p.cpu
}

fn success(p: &mut Process32, mut expected: Cpu32, result: u32) {
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

#[test]
fn imported_lowercase_runs_whole_or_stepwise() {
    for budget in [1, 40] {
        let mut p = process();
        let mut counts = (0, 0);
        loop {
            let run = p.run(budget);
            counts.0 += run.instructions;
            counts.1 += run.api_calls;
            if run.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                break;
            }
            assert!(counts.0 + counts.1 < 40);
        }
        assert_eq!(counts, (8, 2));
        assert_eq!(p.cpu.register(Register32::Esi), u32::from(b't'));
        assert_eq!(p.cpu.register(Register32::Eax), u32::MAX);
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
    }
}

#[test]
fn all_bytes_and_eof_preserve_c_locale_error_state_and_cdecl_arguments() {
    let mut p = process();
    let pages = p.memory.mapped_pages();
    p.memory.write(0x7000_2020, &88_u32.to_le_bytes()).unwrap();
    p.memory.write(0x7ffd_e034, &77_u32.to_le_bytes()).unwrap();
    for page in [0x7000_2000, 0x7ffd_e000] {
        p.memory.protect(page, 4096, Permissions::NONE).unwrap();
    }
    for character in (0..=255).chain([u32::MAX]) {
        let expected = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ"
            .iter()
            .zip(b"abcdefghijklmnopqrstuvwxyz")
            .find_map(|(a, b)| (u32::from(*a) == character).then_some(u32::from(*b)))
            .unwrap_or(character);
        let before = prepare(&mut p, character);
        assert_eq!(p.run(0).api_calls, 0);
        assert_eq!(p.cpu, before);
        success(&mut p, before, expected);
        let mut bytes = [0; 4];
        p.memory.read(u64::from(STACK + 4), &mut bytes).unwrap();
        assert_eq!(bytes, character.to_le_bytes());
    }
    assert_eq!(p.memory.mapped_pages(), pages);
    for page in [0x7000_2000, 0x7ffd_e000] {
        p.memory.protect(page, 4096, Permissions::READ).unwrap();
    }
    let mut bytes = [0; 4];
    p.memory.read(0x7000_2020, &mut bytes).unwrap();
    assert_eq!(bytes, 88_u32.to_le_bytes());
    assert_eq!(p.last_error().unwrap(), 77);
}

#[test]
fn invalid_characters_and_argument_frame_faults_leave_state_unchanged() {
    let mut p = process();
    for character in [
        256,
        0x141,
        0x8000_0000,
        0x7fff_ffff,
        0xffff_ff80,
        0xffff_fffe,
    ] {
        let before = prepare(&mut p, character);
        let run = p.run(1);
        assert_eq!(run.reason, ProcessStop::UnsupportedApi { address: API });
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
    }
    for stack in [0x1000_fffc, 0x6000_0000, 0xffff_fffc] {
        prepare(&mut p, 0x54);
        p.cpu.set_register(Register32::Esp, stack);
        let before = p.cpu;
        let run = p.run(1);
        assert!(matches!(
            run.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
    }
    let before = prepare(&mut p, 0x54);
    p.memory
        .protect(0x1000_f000, 4096, Permissions::NONE)
        .unwrap();
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    p.memory
        .protect(0x1000_f000, 4096, Permissions::READ)
        .unwrap();
    success(&mut p, before, 0x74);
}

#[test]
fn changing_multibyte_code_page_does_not_change_character_mapping() {
    let mut p = process();
    for code_page in [0, 1252] {
        let mut before = prepare(&mut p, code_page);
        p.cpu.eip = 0x7000_013c;
        before.eip = p.cpu.eip;
        success(&mut p, before, 0);
        for (character, expected) in [(0x54, 0x74), (0xc4, 0xc4), (0xff, 0xff)] {
            let before = prepare(&mut p, character);
            success(&mut p, before, expected);
        }
    }
}
