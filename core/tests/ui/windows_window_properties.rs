use super::window_creation_executable;
use super::window_property_cases;

use ring3_core::execution::{Permissions, Process32, ProcessStop, Register32, StopReason};
use window_property_cases::{GET, INDEX, PARENT, REPLACEMENT, SET, WINDOW, call, created, prepare};

#[test]
fn actual_window_procedures_are_exchanged_without_changing_class_or_other_process() {
    window_property_cases::ownership();
}

#[test]
fn new_windows_of_the_same_class_keep_the_class_default() {
    let mut p = created();
    assert_eq!(
        call(&mut p, SET, &[WINDOW, INDEX, REPLACEMENT]),
        0x0040_1100
    );
    prepare(
        &mut p,
        0x7000_02a8,
        &[
            0,
            0x0040_2180,
            0x0040_2190,
            0x00ca_0000,
            10,
            20,
            130,
            90,
            0,
            0,
            0x0040_0000,
            0,
        ],
    );
    assert_eq!(
        p.run(200).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(p.cpu.register(Register32::Eax), WINDOW + 4);
    assert_eq!(call(&mut p, GET, &[WINDOW, INDEX]), REPLACEMENT);
    assert_eq!(call(&mut p, GET, &[WINDOW + 4, INDEX]), 0x0040_1100);
}

#[test]
fn window_styles_are_read_from_owned_windows_and_invalid_handles_report_error() {
    let mut p = created();
    call(&mut p, 0x7000_0000, &[77]);
    assert_eq!(
        call(&mut p, GET, &[WINDOW, (-16_i32).cast_unsigned()]),
        0x04ca_0000
    );
    assert_eq!(
        call(&mut p, GET, &[WINDOW, (-20_i32).cast_unsigned()]),
        0x100
    );
    assert_eq!(p.last_error().unwrap(), 77);
    for index in [(-16_i32).cast_unsigned(), (-20_i32).cast_unsigned()] {
        assert_eq!(call(&mut p, GET, &[0, index]), 0);
        assert_eq!(p.last_error().unwrap(), 1400);
    }
    assert_eq!(call(&mut p, GET, &[WINDOW, INDEX]), 0x0040_1100);
}

#[test]
fn unknown_and_rejected_window_handles_report_invalid_window() {
    let mut p = created();
    for window in [0, 2, 0xc000, 0x0040_0000, WINDOW + 4, u32::MAX] {
        for (api, args) in [
            (PARENT, vec![window]),
            (GET, vec![window, INDEX]),
            (SET, vec![window, INDEX, REPLACEMENT]),
        ] {
            assert_eq!(call(&mut p, api, &args), 0);
            assert_eq!(p.last_error().unwrap(), 1400);
        }
    }
    let bytes = window_creation_executable::pe32(&[0x31, 0xc0, 0xc2, 16, 0]);
    let mut rejected = Process32::load(&bytes, 32).unwrap();
    assert_eq!(
        rejected.run(100).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(call(&mut rejected, PARENT, &[WINDOW]), 0);
    assert_eq!(rejected.last_error().unwrap(), 1400);
    assert_eq!(call(&mut rejected, GET, &[WINDOW, INDEX]), 0);
    assert_eq!(rejected.last_error().unwrap(), 1400);
}

#[test]
fn successful_queries_and_replacement_do_not_access_error_fields() {
    let mut p = created();
    let pages = p.memory.mapped_pages();
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::NONE)
        .unwrap();
    p.memory
        .protect(0x7000_2000, 4096, Permissions::NONE)
        .unwrap();
    assert_eq!(call(&mut p, PARENT, &[WINDOW]), 0);
    assert_eq!(call(&mut p, PARENT, &[1]), 0);
    assert_eq!(call(&mut p, GET, &[WINDOW, INDEX]), 0x0040_1100);
    assert_eq!(
        call(&mut p, SET, &[WINDOW, INDEX, REPLACEMENT]),
        0x0040_1100
    );
    assert_eq!(p.memory.mapped_pages(), pages);
}

#[test]
fn error_write_faults_and_unsupported_profiles_leave_the_procedure_unchanged() {
    let mut p = created();
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ)
        .unwrap();
    for (api, args) in [
        (PARENT, vec![0]),
        (GET, vec![0, INDEX]),
        (GET, vec![0, (-16_i32).cast_unsigned()]),
        (SET, vec![0, INDEX, REPLACEMENT]),
    ] {
        let before = prepare(&mut p, api, &args);
        let run = p.run(1);
        assert!(matches!(
            run.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
    }
    for (api, args) in [
        (GET, vec![WINDOW, 0]),
        (SET, vec![WINDOW, 0, REPLACEMENT]),
        (GET, vec![1, INDEX]),
        (SET, vec![1, INDEX, REPLACEMENT]),
        (SET, vec![WINDOW, INDEX, 0]),
        (SET, vec![WINDOW, INDEX, 0x7000_0ff8]),
    ] {
        let before = prepare(&mut p, api, &args);
        let run = p.run(1);
        assert_eq!(run.reason, ProcessStop::UnsupportedApi { address: api });
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
        assert_eq!(call(&mut p, GET, &[WINDOW, INDEX]), 0x0040_1100);
    }
}

#[test]
fn full_replacement_frame_and_positive_budget_precede_mutation() {
    let mut p = created();
    let before = prepare(&mut p, SET, &[WINDOW, INDEX, REPLACEMENT]);
    assert_eq!(p.run(0).api_calls, 0);
    assert_eq!(p.cpu, before);
    assert_eq!(call(&mut p, GET, &[WINDOW, INDEX]), 0x0040_1100);
    p.cpu.eip = SET;
    p.cpu.set_register(Register32::Esp, 0x1000_fff4);
    let frame: Vec<_> = [0x0040_10f0, WINDOW, INDEX]
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect();
    p.memory.write(0x1000_fff4, &frame).unwrap();
    let before = p.cpu;
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    assert_eq!(call(&mut p, GET, &[WINDOW, INDEX]), 0x0040_1100);
}

#[test]
fn replacement_is_stored_without_pretending_an_invalid_target_can_execute() {
    let mut p = created();
    assert_eq!(
        call(&mut p, SET, &[WINDOW, INDEX, 0x6000_0000]),
        0x0040_1100
    );
    assert_eq!(call(&mut p, GET, &[WINDOW, INDEX]), 0x6000_0000);
    prepare(&mut p, 0x7000_02a4, &[0x6000_0000, WINDOW, 0x400, 0, 0]);
    let run = p.run(100);
    assert!(matches!(
        run.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    assert_eq!(p.cpu.eip, 0x6000_0000);
}
