use super::arguments_executable;

use ring3_core::execution::{
    PAGE_SIZE, Permissions, Process32, ProcessOptions, ProcessStop, Register32, StopReason,
};

fn word(process: &Process32, address: u32) -> u32 {
    let mut bytes = [0; 4];
    process.memory.read(u64::from(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

#[test]
fn main_arguments_return_real_guest_arrays_and_caller_cleans_the_stack() {
    let bytes = arguments_executable::pe32(0, 1);
    let mut process = Process32::load_with_options(
        &bytes,
        32,
        ProcessOptions {
            command_line: b"app one \"two words\"",
            environment: &[b"A=B"],
            ..ProcessOptions::default()
        },
    )
    .unwrap();
    assert_eq!(process.crt_new_mode(), 0);
    let stack = process.cpu.register(Register32::Esp);
    let result = process.run(8);
    assert_eq!((result.instructions, result.api_calls), (7, 1));
    assert_eq!(process.cpu.register(Register32::Esp), stack - 20);
    assert_eq!(process.cpu.register(Register32::Eax), 0);
    assert_eq!(process.crt_new_mode(), 1);
    assert_eq!(word(&process, 0x0040_2304), 3);
    let argv = word(&process, 0x0040_2308);
    let env = word(&process, 0x0040_230c);
    assert_eq!(argv, word(&process, 0x7000_2014));
    assert_eq!(env, word(&process, 0x7000_2018));
    assert_eq!(word(&process, argv + 12), 0);
    assert_eq!(word(&process, env + 4), 0);
    assert_eq!(word(&process, 0x7000_201c), 0);
    assert_eq!(
        process.run(2).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(process.cpu.register(Register32::Esp), stack);
}

#[test]
fn faults_and_unsupported_startup_options_have_no_output_or_state_side_effects() {
    for (wildcard, mode) in [(1, 0), (0, 2)] {
        let mut process = Process32::load(&arguments_executable::pe32(wildcard, mode), 32).unwrap();
        assert_eq!(process.run(7).instructions, 7);
        let before = process.cpu;
        assert!(matches!(
            process.run(1).reason,
            ProcessStop::UnsupportedApi { .. }
        ));
        assert_eq!(process.cpu, before);
        assert_eq!(process.crt_new_mode(), 0);
        assert_eq!(word(&process, 0x0040_2304), 0);
    }
    for argument in [0_u32, 1, 2, 4] {
        let mut process = Process32::load(&arguments_executable::pe32(0, 1), 32).unwrap();
        process.run(7);
        let stack = process.cpu.register(Register32::Esp);
        process
            .memory
            .write(
                u64::from(stack + (argument + 1) * 4),
                &0xffff_fffe_u32.to_le_bytes(),
            )
            .unwrap();
        let before = process.cpu;
        assert!(matches!(
            process.run(1).reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!(process.cpu, before);
        assert_eq!(process.crt_new_mode(), 0);
        assert_eq!(word(&process, 0x0040_2304), 0);
        assert_eq!(word(&process, 0x0040_2308), 0);
    }
    let mut process = Process32::load(&arguments_executable::pe32(0, 1), 32).unwrap();
    process.run(7);
    process
        .memory
        .protect(0x0040_2000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    let before = process.cpu;
    assert!(matches!(
        process.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(process.cpu, before);
}

#[test]
fn aliased_outputs_use_a_source_snapshot_and_reread_the_return_address() {
    let mut process = Process32::load(&arguments_executable::pe32(0, 1), 32).unwrap();
    process.run(7);
    let stack = process.cpu.register(Register32::Esp);
    let argv = word(&process, 0x7000_2014);
    let env = word(&process, 0x7000_2018);
    for (argument, destination) in [(0_u32, 0x7000_2014_u32), (1, 0x0040_2300), (2, stack)] {
        process
            .memory
            .write(
                u64::from(stack + (argument + 1) * 4),
                &destination.to_le_bytes(),
            )
            .unwrap();
    }
    assert_eq!(process.run(1).api_calls, 1);
    assert_eq!(word(&process, 0x7000_2014), 1);
    assert_eq!(word(&process, 0x0040_2300), argv);
    assert_eq!(process.crt_new_mode(), 1);
    assert_eq!(process.cpu.eip, env);
}
