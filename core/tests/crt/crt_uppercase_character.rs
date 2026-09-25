use super::imported_executable;

use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

const API: u32 = 0x7000_01c4;
const STACK: u32 = 0x1000_ff00;

fn process() -> Process32 {
    let code = [
        0x6a, 0x61, 0xff, 0x15, 0x60, 0x20, 0x40, 0, 0x83, 0xc4, 4, 0xcc,
    ];
    Process32::load(
        &imported_executable::pe32(&code, "MSVCRT.dll", &["toupper"]),
        32,
    )
    .unwrap()
}

fn call(process: &mut Process32, value: u32) -> ring3_core::execution::ProcessResult {
    process.cpu.eip = API;
    process.cpu.set_register(Register32::Esp, STACK);
    process
        .memory
        .write(u64::from(STACK), &0x0040_1000_u32.to_le_bytes())
        .unwrap();
    process
        .memory
        .write(u64::from(STACK + 4), &value.to_le_bytes())
        .unwrap();
    process.run(1)
}

#[test]
fn imported_toupper_uses_c_locale_and_preserves_eof() {
    let mut process = process();
    let result = process.run(20);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!(result.api_calls, 1);
    assert_eq!(process.cpu.register(Register32::Eax), u32::from(b'A'));
    for value in 0..=255_u32 {
        let result = call(&mut process, value);
        assert_eq!(
            result.reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
        assert_eq!(result.api_calls, 1);
        assert_eq!(
            process.cpu.register(Register32::Eax),
            u32::from(u8::try_from(value).unwrap().to_ascii_uppercase())
        );
    }
    let result = call(&mut process, u32::MAX);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!(process.cpu.register(Register32::Eax), u32::MAX);
    let result = call(&mut process, 256);
    assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: API });
}
