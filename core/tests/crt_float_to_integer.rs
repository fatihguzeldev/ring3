#[path = "support/float_to_integer_executable.rs"]
mod float_to_integer_executable;
#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{
    LoadError, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const API: u32 = 0x7000_0168;
const STACK: u32 = 0x1000_ff00;

fn process() -> Process32 {
    Process32::load(
        &imported_executable::pe32(
            &[0xdd, 0x05, 0x80, 0x21, 0x40, 0, 0xcc],
            "MSVCRT.dll",
            &["_ftol"],
        ),
        33,
    )
    .unwrap()
}

fn push(p: &mut Process32, bits: u64) {
    p.memory.write(0x0040_2180, &bits.to_le_bytes()).unwrap();
    p.cpu.eip = 0x0040_1000;
    assert_eq!(p.run(1).instructions, 1);
}

fn prepare(p: &mut Process32) {
    p.memory
        .write(u64::from(STACK), &0x0040_1000_u32.to_le_bytes())
        .unwrap();
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu.set_register(Register32::Edx, 77);
    p.cpu.eflags = 0xced7;
}

fn integer(p: &Process32) -> u64 {
    (u64::from(p.cpu.register(Register32::Edx)) << 32) | u64::from(p.cpu.register(Register32::Eax))
}

#[test]
fn imported_float_conversion_returns_both_integer_halves_whole_or_stepwise() {
    for budget in [1, 40] {
        let mut p = Process32::load(&float_to_integer_executable::pe32(), 32).unwrap();
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
        assert_eq!(counts, (7, 2));
        assert_eq!(p.cpu.register(Register32::Esi), 1);
        assert_eq!(p.cpu.register(Register32::Edi), 1);
        assert_eq!(p.cpu.register(Register32::Eax), u32::MAX);
        assert_eq!(p.cpu.register(Register32::Edx), u32::MAX);
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
    }
}

#[test]
fn signed_fractions_and_integer_boundaries_truncate_and_pop_without_memory_writes() {
    for (input, expected) in [
        (0_u64, 0_u64),
        (0x8000_0000_0000_0000, 0),
        (0x3ffc_0000_0000_0000, 1),
        (0xbffc_0000_0000_0000, u64::MAX),
        (0x3fe8_0000_0000_0000, 0),
        (0xbfe8_0000_0000_0000, 0),
        (0x0010_0000_0000_0000, 0),
        (0x8010_0000_0000_0000, 0),
        (0x41df_ffff_ffc0_0000, 0x7fff_ffff),
        (0x41e0_0000_0000_0000, 0x8000_0000),
        (0xc1e0_0000_0000_0000, 0xffff_ffff_8000_0000),
        (0x41f0_0000_001c_0000, 0x0000_0001_0000_0001),
        (0xc1f0_0000_001c_0000, 0xffff_fffe_ffff_ffff),
        (0x43df_ffff_ffff_ffff, 0x7fff_ffff_ffff_fc00),
        (0xc3e0_0000_0000_0000, 0x8000_0000_0000_0000),
    ] {
        let mut p = process();
        prepare(&mut p);
        let mut empty = p.cpu;
        push(&mut p, input);
        p.memory.write(0x7000_2020, &123_u32.to_le_bytes()).unwrap();
        p.memory.write(0x7ffd_e034, &77_u32.to_le_bytes()).unwrap();
        for page in [0x7000_2000, 0x7ffd_e000] {
            p.memory.protect(page, 4096, Permissions::READ).unwrap();
        }
        prepare(&mut p);
        let before = p.cpu;
        assert_eq!(p.run(0).api_calls, 0);
        assert_eq!(p.cpu, before);
        let run = p.run(1);
        assert_eq!(
            run.reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
        assert_eq!((run.instructions, run.api_calls), (0, 1));
        assert_eq!(integer(&p), expected);
        empty.eip = 0x0040_1000;
        empty.set_register(Register32::Esp, STACK + 4);
        empty.set_register(Register32::Eax, p.cpu.register(Register32::Eax));
        empty.set_register(Register32::Edx, p.cpu.register(Register32::Edx));
        assert_eq!(p.cpu, empty);
        assert_eq!(p.last_error().unwrap(), 77);
        let mut error = [0; 4];
        p.memory.read(0x7000_2020, &mut error).unwrap();
        assert_eq!(error, 123_u32.to_le_bytes());
    }
}

#[test]
fn helper_truncation_ignores_pc_rc_and_preserves_deeper_stack_values() {
    for pc in [0, 0x200, 0x300] {
        for rc in [0, 0x400, 0x800, 0xc00] {
            let mut p = process();
            push(&mut p, 1.75_f64.to_bits());
            push(&mut p, (-2.75_f64).to_bits());
            let control = 0x7f | pc | rc;
            p.cpu.set_x87_control_word(control);
            for expected in [u64::MAX - 1, 1] {
                prepare(&mut p);
                assert_eq!(p.run(1).api_calls, 1);
                assert_eq!(integer(&p), expected);
                assert_eq!(p.cpu.x87_control_word(), control);
                assert_eq!(p.cpu.eflags, 0xced7);
            }
            prepare(&mut p);
            let before = p.cpu;
            assert_eq!(
                p.run(1).reason,
                ProcessStop::UnsupportedApi { address: API }
            );
            assert_eq!(p.cpu, before);
        }
    }
}

#[test]
fn invalid_ranges_masks_and_frames_preserve_the_unconsumed_input() {
    for input in [
        0x43e0_0000_0000_0000_u64,
        0xc3e0_0000_0000_0001,
        0x7fef_ffff_ffff_ffff,
    ] {
        let mut p = process();
        push(&mut p, input);
        prepare(&mut p);
        let before = p.cpu;
        let run = p.run(1);
        assert_eq!(run.reason, ProcessStop::UnsupportedApi { address: API });
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
    }
    for bit in 0..6 {
        let mut p = process();
        push(&mut p, 1.75_f64.to_bits());
        p.cpu.set_x87_control_word(0x027f & !(1 << bit));
        prepare(&mut p);
        let before = p.cpu;
        assert_eq!(
            p.run(1).reason,
            ProcessStop::UnsupportedApi { address: API }
        );
        assert_eq!(p.cpu, before);
        p.cpu.set_x87_control_word(0x027f);
        assert_eq!(p.run(1).api_calls, 1);
        assert_eq!(integer(&p), 1);
    }
    let mut p = process();
    push(&mut p, (-1.75_f64).to_bits());
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory
        .write(0xffff_fffc, &0x0040_1000_u32.to_le_bytes())
        .unwrap();
    for stack in [0x6000_0000, 0xffff_fffd] {
        prepare(&mut p);
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
    p.cpu.set_register(Register32::Esp, 0xffff_fffc);
    assert_eq!(p.run(1).api_calls, 1);
    assert_eq!(p.cpu.register(Register32::Esp), 0);
    assert_eq!(integer(&p), u64::MAX);
}

#[test]
fn aliases_and_wrong_module_remain_unresolved() {
    for (module, symbol) in [
        ("kernel32.dll", "_ftol"),
        ("MSVCRT.dll", "_FTOL"),
        ("MSVCRT.dll", "_ftol2"),
        ("MSVCRT.dll", "_ftol2_sse"),
    ] {
        assert!(matches!(
            Process32::load(&imported_executable::pe32(&[0xcc], module, &[symbol]), 32),
            Err(LoadError::UnresolvedImport { .. })
        ));
    }
}
