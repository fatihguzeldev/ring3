#[path = "support/argument_pointer_executable.rs"]
mod argument_pointer_executable;
#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{
    Cpu32, Permissions, Process32, ProcessOptions, ProcessStop, Register32, StopReason,
};

const COUNT: u32 = 0x7000_2010;
const VECTOR: u32 = 0x7000_2014;
const COUNT_API: u32 = 0x7000_0180;
const VECTOR_API: u32 = 0x7000_0184;
const STACK: u32 = 0x1000_ff00;

fn load() -> Process32 {
    Process32::load(&argument_pointer_executable::pe32(), 32).unwrap()
}
fn word(p: &Process32, address: u32) -> u32 {
    let mut bytes = [0; 4];
    p.memory.read(u64::from(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}
fn write(p: &mut Process32, address: u32, value: u32) {
    p.memory
        .write(u64::from(address), &value.to_le_bytes())
        .unwrap();
}
fn prepare(p: &mut Process32, api: u32) -> Cpu32 {
    p.cpu.eip = api;
    p.cpu.eflags = 0xced7;
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu.set_register(Register32::Esp, STACK);
    write(p, STACK, 0x0040_1000);
    p.cpu
}
fn call(p: &mut Process32, api: u32, value: u32) {
    let mut cpu = prepare(p, api);
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    cpu.eip = 0x0040_1000;
    cpu.set_register(Register32::Eax, value);
    cpu.set_register(Register32::Esp, STACK + 4);
    assert_eq!(p.cpu, cpu);
}

#[test]
fn imported_pointer_functions_return_the_data_import_cells_whole_or_stepwise() {
    for budget in [1, 100] {
        let mut p = Process32::load_with_options(
            &argument_pointer_executable::pe32(),
            32,
            ProcessOptions {
                command_line: b"app one \"two words\"",
                ..ProcessOptions::default()
            },
        )
        .unwrap();
        let mut counts = (0, 0);
        loop {
            let run = p.run(budget);
            counts.0 += run.instructions;
            counts.1 += run.api_calls;
            if run.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                break;
            }
            assert!(counts.0 + counts.1 < 100);
        }
        assert_eq!(counts, (4, 2));
        assert_eq!(p.cpu.register(Register32::Ebx), word(&p, 0x0040_2068));
        assert_eq!(p.cpu.register(Register32::Eax), word(&p, 0x0040_206c));
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
        assert_eq!(word(&p, COUNT), 3);
        let argv = word(&p, VECTOR);
        assert_eq!(word(&p, argv + 12), 0);
        let mut argument = [0; 10];
        p.memory
            .read(u64::from(word(&p, argv + 8)), &mut argument)
            .unwrap();
        assert_eq!(&argument, b"two words\0");
    }
}

#[test]
fn guest_mutations_are_shared_with_getmainargs_and_isolated_between_processes() {
    let mut p = load();
    let other = load();
    write(&mut p, COUNT, u32::MAX);
    write(&mut p, VECTOR, 0x0040_2280);
    for _ in 0..2 {
        call(&mut p, COUNT_API, COUNT);
        call(&mut p, VECTOR_API, VECTOR);
    }
    for (i, value) in [0x0040_2300, 0x0040_2304, 0x0040_2308, 0, 0]
        .iter()
        .enumerate()
    {
        write(&mut p, STACK + 4 + u32::try_from(i).unwrap() * 4, *value);
    }
    call(&mut p, 0x7000_0110, 0);
    assert_eq!(word(&p, 0x0040_2300), u32::MAX);
    assert_eq!(word(&p, 0x0040_2304), 0x0040_2280);
    assert_eq!(word(&other, COUNT), 1);
    assert_ne!(word(&other, VECTOR), 0x0040_2280);
}

#[test]
fn taking_an_address_does_not_read_the_cell_or_touch_error_state() {
    let mut p = load();
    let pages = p.memory.mapped_pages();
    write(&mut p, 0x7ffd_e034, 77);
    write(&mut p, 0x7000_2020, 88);
    p.memory
        .protect(0x7000_2000, 4096, Permissions::NONE)
        .unwrap();
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::NONE)
        .unwrap();
    call(&mut p, COUNT_API, COUNT);
    call(&mut p, VECTOR_API, VECTOR);
    assert_eq!(p.memory.mapped_pages(), pages);
    p.memory
        .protect(0x7000_2000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(p.last_error().unwrap(), 77);
    assert_eq!(word(&p, 0x7000_2020), 88);
}

#[test]
fn zero_budgets_and_saved_return_faults_do_not_complete_pointer_calls() {
    let mut p = load();
    for api in [COUNT_API, VECTOR_API] {
        let cpu = prepare(&mut p, api);
        let run = p.run(0);
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, cpu);
        for stack in [0x1001_0000, 0xffff_fffd] {
            p.cpu.set_register(Register32::Esp, stack);
            let cpu = p.cpu;
            assert!(matches!(
                p.run(1).reason,
                ProcessStop::Stopped(StopReason::MemoryFault(_))
            ));
            assert_eq!(p.cpu, cpu);
        }
    }
}
