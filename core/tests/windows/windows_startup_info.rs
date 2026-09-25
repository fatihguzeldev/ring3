use super::startup_info_executable;

use ring3_core::execution::{Cpu32, Permissions, Process32, ProcessStop, Register32, StopReason};

const API: u32 = 0x7000_0248;
const STACK: u32 = 0x1000_ff00;

fn process() -> Process32 {
    Process32::load(&startup_info_executable::pe32(), 64).unwrap()
}

fn record() -> [u8; 68] {
    let mut bytes = [0; 68];
    bytes[0] = 68;
    bytes
}

fn read(p: &Process32, address: u32, count: usize) -> Vec<u8> {
    let mut bytes = vec![0; count];
    p.memory.read(u64::from(address), &mut bytes).unwrap();
    bytes
}

fn prepare(p: &mut Process32, output: u32) -> Cpu32 {
    p.cpu.eip = API;
    p.cpu.eflags = 0xced7;
    p.cpu.set_register(Register32::Eax, 0x1234_5678);
    p.cpu.set_register(Register32::Esp, STACK);
    p.memory
        .write(u64::from(STACK), &0x0040_1000_u32.to_le_bytes())
        .unwrap();
    p.memory
        .write(u64::from(STACK + 4), &output.to_le_bytes())
        .unwrap();
    p.cpu
}

fn success(p: &mut Process32, mut expected: Cpu32, target: u32) {
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    expected.eip = target;
    expected.set_register(Register32::Esp, STACK + 8);
    assert_eq!(p.cpu, expected);
}

#[test]
fn imported_startup_information_runs_whole_or_stepwise() {
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
        assert_eq!(counts, (4, 1));
        assert_eq!(p.cpu.register(Register32::Eax), 0x1234_5678);
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
        assert_eq!(read(&p, 0x0040_2180, 68), record());
    }
}

#[test]
fn complete_record_replaces_poison_and_preserves_guards_error_cells_and_cpu() {
    use ring3_core::execution::ProcessOptions;
    let mut p = Process32::load_with_options(
        &startup_info_executable::pe32(),
        64,
        ProcessOptions {
            image_path: b"D:\\demo.exe",
            command_line: b"demo /quiet",
            environment: &[b"X=Y"],
            ..ProcessOptions::default()
        },
    )
    .unwrap();
    p.memory.write(0x7000_2020, &88_u32.to_le_bytes()).unwrap();
    p.memory.write(0x7ffd_e034, &77_u32.to_le_bytes()).unwrap();
    for page in [0x7000_2000, 0x7ffd_e000] {
        p.memory.protect(page, 4096, Permissions::NONE).unwrap();
    }
    let pages = p.memory.mapped_pages();
    for poison in [0_u8, 0x55, 0xff] {
        p.memory.write(0x0040_2180, &[poison; 70]).unwrap();
        let before = prepare(&mut p, 0x0040_2181);
        assert_eq!(p.run(0).api_calls, 0);
        assert_eq!(p.cpu, before);
        assert_eq!(read(&p, 0x0040_2180, 70), [poison; 70]);
        success(&mut p, before, 0x0040_1000);
        assert_eq!(read(&p, 0x0040_2181, 68), record());
        assert_eq!(read(&p, 0x0040_2180, 1), [poison]);
        assert_eq!(read(&p, 0x0040_21c5, 1), [poison]);
    }
    assert_eq!(p.memory.mapped_pages(), pages);
    for page in [0x7000_2000, 0x7ffd_e000] {
        p.memory.protect(page, 4096, Permissions::READ).unwrap();
    }
    assert_eq!(read(&p, 0x7000_2020, 4), 88_u32.to_le_bytes());
    assert_eq!(p.last_error().unwrap(), 77);
}

#[test]
fn write_only_cross_page_and_last_guest_byte_outputs_need_no_previous_contents() {
    for output in [0_u32, 0x3000_0fdf, u32::MAX - 67] {
        let mut p = process();
        let base = u64::from(output & !0xfff);
        let length = if output == 0x3000_0fdf { 8192 } else { 4096 };
        p.memory
            .map_zeroed(
                base,
                length,
                Permissions {
                    write: true,
                    ..Permissions::NONE
                },
            )
            .unwrap();
        let before = prepare(&mut p, output);
        success(&mut p, before, 0x0040_1000);
        p.memory.protect(base, length, Permissions::READ).unwrap();
        assert_eq!(read(&p, output, 68), record());
    }
}

#[test]
fn destination_faults_and_bad_call_frames_preserve_memory_and_allow_retry() {
    let mut p = process();
    p.memory.write(0x0040_2fe0, &[0x55; 32]).unwrap();
    for output in [0, 0x6000_0000, 0x0040_2fe0, u32::MAX - 66] {
        let before = prepare(&mut p, output);
        let run = p.run(1);
        assert!(matches!(
            run.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
        assert_eq!(read(&p, 0x0040_2fe0, 32), [0x55; 32]);
    }
    for permissions in [Permissions::READ, Permissions::NONE] {
        let before = prepare(&mut p, 0x0040_2180);
        p.memory.protect(0x0040_2000, 4096, permissions).unwrap();
        assert!(matches!(
            p.run(1).reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!(p.cpu, before);
    }
    p.memory
        .protect(0x0040_2000, 4096, Permissions::READ_WRITE)
        .unwrap();
    let before = prepare(&mut p, 0x0040_2fe0);
    p.memory
        .map_zeroed(0x0040_3000, 4096, Permissions::READ_WRITE)
        .unwrap();
    success(&mut p, before, 0x0040_1000);
    assert_eq!(read(&p, 0x0040_2fe0, 68), record());
    for stack in [0x1000_fffc, 0x6000_0000, 0xffff_fffc] {
        prepare(&mut p, 0x0040_2180);
        p.memory.write(0x0040_2180, &[0x55; 68]).unwrap();
        p.cpu.set_register(Register32::Esp, stack);
        let before = p.cpu;
        assert!(matches!(
            p.run(1).reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!(p.cpu, before);
        assert_eq!(read(&p, 0x0040_2180, 68), [0x55; 68]);
    }
}

#[test]
fn output_aliases_use_captured_argument_and_written_return_and_error_cells() {
    for (output, target) in [(STACK, 68), (STACK - 4, 0), (STACK + 4, 0x0040_1000)] {
        let mut p = process();
        let before = prepare(&mut p, output);
        success(&mut p, before, target);
        assert_eq!(read(&p, output, 68), record());
    }
    for output in [0x7000_2020, 0x7ffd_e034] {
        let mut p = process();
        let before = prepare(&mut p, output);
        success(&mut p, before, 0x0040_1000);
        assert_eq!(read(&p, output, 68), record());
    }
}
