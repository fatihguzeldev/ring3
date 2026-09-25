use super::space_classification_cases;

#[test]
fn imported_isspace_classifies_c_whitespace_across_budgets() {
    space_classification_cases::imported_calls_across_budgets();
}

#[test]
fn exhaustive_bytes_and_eof_preserve_cdecl_and_error_state() {
    space_classification_cases::all_bytes_and_eof_preserve_state();
}

use ring3_core::execution::{Permissions, ProcessStop, Register32, StopReason};
use space_classification_cases::{API, STACK, prepare, process, success, word};

#[test]
fn invalid_character_representations_are_atomic_and_repairable() {
    let mut p = process(11);
    for value in [
        256,
        0x120,
        0x8000_0000,
        0x7fff_ffff,
        0xffff_ff80,
        0xffff_fffe,
    ] {
        let before = prepare(&mut p, value);
        let run = p.run(1);
        assert_eq!(run.reason, ProcessStop::UnsupportedApi { address: API });
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
        assert_eq!(word(&p, STACK + 4), value);
        p.memory
            .write(u64::from(STACK + 4), &11_u32.to_le_bytes())
            .unwrap();
        success(&mut p, before, 1);
    }
}

#[test]
fn missing_protected_and_overflowing_frames_fault_without_partial_return() {
    let mut p = process(11);
    for stack in [0x1000_fffc, 0x6000_0000, 0xffff_fffc] {
        prepare(&mut p, 11);
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
    let before = prepare(&mut p, 11);
    p.memory
        .protect(0x1000_f000, 4096, Permissions::NONE)
        .unwrap();
    let run = p.run(1);
    assert!(matches!(
        run.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    p.memory
        .protect(0x1000_f000, 4096, Permissions::READ)
        .unwrap();
    success(&mut p, before, 1);
    assert_eq!(word(&p, STACK + 4), 11);
}

#[test]
fn multibyte_code_page_selection_keeps_c_locale_classification() {
    let mut p = process(11);
    for code_page in [0, 1252, 437] {
        let mut before = prepare(&mut p, code_page);
        p.cpu.eip = 0x7000_013c;
        before.eip = p.cpu.eip;
        success(&mut p, before, 0);
        for (value, result) in [
            (11, 1),
            (32, 1),
            (b't'.into(), 0),
            (0x85, 0),
            (0xa0, 0),
            (255, 0),
        ] {
            let before = prepare(&mut p, value);
            success(&mut p, before, result);
        }
    }
}
