use ring3_core::execution::{Cpu32, GuestMemory, Permissions, Register32, StopReason};

#[test]
fn interception_stops_before_fetch_without_consuming_an_instruction() {
    let mut memory = GuestMemory::new(2);
    memory
        .map_zeroed(0x1000, 4096, Permissions::READ_WRITE)
        .unwrap();
    memory
        .write(0x1000, &[0xe8, 0xfb, 0x0f, 0, 0, 0xcc])
        .unwrap();
    memory
        .protect(0x1000, 4096, Permissions::READ_EXECUTE)
        .unwrap();
    memory
        .map_zeroed(0x3000, 4096, Permissions::READ_WRITE)
        .unwrap();
    let mut cpu = Cpu32::new(0x1000);
    cpu.set_register(Register32::Esp, 0x4000);
    let result = cpu.run_until(&mut memory, 10, |address| address == 0x2000);
    assert_eq!(result.reason, StopReason::Intercepted);
    assert_eq!(result.instructions, 1);
    assert_eq!(result.instruction_pointer, 0x2000);
    assert_eq!(cpu.eip, 0x2000);
    let before = cpu;
    assert_eq!(cpu.run_until(&mut memory, 10, |_| true).instructions, 0);
    assert_eq!(cpu, before);
    assert_eq!(
        cpu.run_until(&mut memory, 0, |_| true).reason,
        StopReason::InstructionLimit
    );
    cpu.eip = 0x1005;
    assert_eq!(cpu.run(&mut memory, 1).reason, StopReason::Breakpoint);
}
