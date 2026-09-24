#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

const STACK: u32 = 0x1000_ff00;
const CALLBACK: u32 = 0x0040_1020;

fn process(ordinal: bool) -> Process32 {
    let mut code = [0xcc; 48];
    code[32..40].copy_from_slice(&[0xb8, 1, 0, 0, 0, 0xc2, 16, 0]);
    let mut bytes = imported_executable::pe32(&code, "DSOUND.dll", &["DirectSoundEnumerateA"]);
    if ordinal {
        for offset in [1088, 1120] {
            bytes[offset..offset + 4].copy_from_slice(&0x8000_0002_u32.to_le_bytes());
        }
    }
    Process32::load(&bytes, 64).unwrap()
}

fn word(process: &Process32, address: u32) -> u32 {
    let mut bytes = [0; 4];
    process.memory.read(u64::from(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

#[test]
fn direct_sound_enumerates_a_primary_device_by_name_and_ordinal() {
    for ordinal in [false, true] {
        let mut process = process(ordinal);
        let pages_before_enumeration = process.memory.mapped_pages();
        let target = word(&process, 0x0040_2060);
        process.cpu.eip = target;
        process.cpu.set_register(Register32::Esp, STACK);
        for (index, value) in [0x0040_1000, CALLBACK, 0x1234_5678].iter().enumerate() {
            process
                .memory
                .write(u64::from(STACK) + index as u64 * 4, &value.to_le_bytes())
                .unwrap();
        }

        let first = process.run(1);
        assert_eq!(
            first.reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
        assert_eq!(process.cpu.eip, CALLBACK);
        assert_eq!(process.memory.mapped_pages(), pages_before_enumeration + 1);
        let callback_stack = process.cpu.register(Register32::Esp);
        assert_eq!(word(&process, callback_stack), 0x7000_0ff8);
        assert_eq!(word(&process, callback_stack + 4), 0);
        let description = word(&process, callback_stack + 8);
        let module = word(&process, callback_stack + 12);
        assert_eq!(word(&process, callback_stack + 16), 0x1234_5678);
        let mut name = [0; 21];
        process
            .memory
            .read(u64::from(description), &mut name)
            .unwrap();
        assert_eq!(&name, b"Primary Sound Driver\0");
        let mut module_name = [1];
        process
            .memory
            .read(u64::from(module), &mut module_name)
            .unwrap();
        assert_eq!(module_name, [0]);

        let resumed = process.run(20);
        assert_eq!(resumed.reason, ProcessStop::Stopped(StopReason::Breakpoint));
        assert_eq!(process.cpu.register(Register32::Eax), 0);
        assert_eq!(process.cpu.register(Register32::Esp), STACK + 12);
    }
}
