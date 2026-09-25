use super::imported_executable;

use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

const INITIALIZE: u32 = 0x7000_0474;
const UNINITIALIZE: u32 = 0x7000_0478;
const STACK: u32 = 0x1000_ef00;

fn process() -> Process32 {
    Process32::load(
        &imported_executable::pe32(&[0xcc], "ole32.dll", &["CoInitialize", "CoUninitialize"]),
        32,
    )
    .unwrap()
}

fn call(process: &mut Process32, api: u32, args: &[u32]) -> u32 {
    process.cpu.eip = api;
    process.cpu.set_register(Register32::Esp, STACK);
    let frame: Vec<_> = std::iter::once(0x0040_1000_u32)
        .chain(args.iter().copied())
        .flat_map(u32::to_le_bytes)
        .collect();
    process.memory.write(u64::from(STACK), &frame).unwrap();
    let result = process.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    assert_eq!(
        process.cpu.register(Register32::Esp),
        STACK + 4 + u32::try_from(args.len() * 4).unwrap()
    );
    process.cpu.register(Register32::Eax)
}

#[test]
fn successful_sta_initializations_balance_with_uninitializations() {
    let mut process = process();
    assert_eq!(call(&mut process, INITIALIZE, &[0]), 0);
    assert_eq!(call(&mut process, INITIALIZE, &[0]), 1);
    process.cpu.set_register(Register32::Eax, 0x1234_5678);
    assert_eq!(call(&mut process, UNINITIALIZE, &[]), 0x1234_5678);
    assert_eq!(call(&mut process, INITIALIZE, &[0]), 1);
    call(&mut process, UNINITIALIZE, &[]);
    call(&mut process, UNINITIALIZE, &[]);
    assert_eq!(call(&mut process, INITIALIZE, &[0]), 0);
    assert_eq!(call(&mut process, INITIALIZE, &[0]), 1);
    let mut fresh = self::process();
    assert_eq!(call(&mut fresh, INITIALIZE, &[0]), 0);
}

#[test]
fn reserved_pointer_error_and_unbalanced_uninit_do_not_initialize() {
    let mut process = process();
    assert_eq!(call(&mut process, INITIALIZE, &[0x1234]), 0x8007_0057);
    process.cpu.set_register(Register32::Eax, 0xace0);
    assert_eq!(call(&mut process, UNINITIALIZE, &[]), 0xace0);
    assert_eq!(call(&mut process, INITIALIZE, &[0]), 0);
}
