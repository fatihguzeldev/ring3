use super::imported_executable;

use ring3_core::execution::{
    Cpu32, PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const API: u32 = 0x7000_0118;
const STACK: u32 = 0x1000_ff00;
const RETURN: u32 = 0x0040_1000;
const HANDLER: u32 = 0x1234_5678;
const OLD_EBP: u32 = 0xabcd_1234;

fn process() -> Process32 {
    Process32::load(
        &imported_executable::pe32(&[0xcc], "MSVCRT.dll", &["_EH_prolog"]),
        32,
    )
    .unwrap()
}

fn prepare(process: &mut Process32, stack: u32, target: u32) -> Cpu32 {
    process.cpu.eip = API;
    process.cpu.set_register(Register32::Esp, stack);
    process.cpu.set_register(Register32::Ebp, OLD_EBP);
    process.cpu.set_register(Register32::Eax, HANDLER);
    process.cpu.eflags = 0xced7;
    process
        .memory
        .write(u64::from(stack), &target.to_le_bytes())
        .unwrap();
    process.cpu
}

fn words(process: &Process32, address: u32) -> [u32; 5] {
    let mut bytes = [0; 20];
    process.memory.read(u64::from(address), &mut bytes).unwrap();
    std::array::from_fn(|i| u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap()))
}

#[test]
fn helper_builds_full_frame_and_returns_with_custom_register_abi() {
    for (stack, target) in [(STACK, RETURN), (0xffff_fffc, 0xdead_beef)] {
        let mut process = process();
        if stack != STACK {
            process
                .memory
                .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
                .unwrap();
        }
        process.cpu.set_fs_base(0x0040_2180);
        process
            .memory
            .write(0x0040_2180, &0x8765_4321_u32.to_le_bytes())
            .unwrap();
        let before = prepare(&mut process, stack, target);
        let result = process.run(1);
        assert_eq!(
            result.reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
        assert_eq!((result.instructions, result.api_calls), (0, 1));
        let mut expected = before;
        expected.eip = target;
        expected.set_register(Register32::Eax, target);
        expected.set_register(Register32::Ebp, stack);
        expected.set_register(Register32::Esp, stack - 12);
        assert_eq!(process.cpu, expected);
        assert_eq!(
            words(&process, stack - 16),
            [target, 0x8765_4321, HANDLER, u32::MAX, OLD_EBP]
        );
        let mut chain = [0; 4];
        process.memory.read(0x0040_2180, &mut chain).unwrap();
        assert_eq!(u32::from_le_bytes(chain), stack - 12);
        if target != RETURN {
            assert!(matches!(
                process.run(1).reason,
                ProcessStop::Stopped(StopReason::MemoryFault(_))
            ));
        }
    }
}

#[test]
fn stack_and_fs_failures_preserve_cpu_frame_and_chain() {
    for mode in 0..4 {
        let mut process = process();
        let stack = match mode {
            0 => {
                process
                    .memory
                    .map_zeroed(0, PAGE_SIZE, Permissions::READ_WRITE)
                    .unwrap();
                8
            }
            1 => {
                process
                    .memory
                    .map_zeroed(0x0fff_f000, PAGE_SIZE, Permissions::READ_WRITE)
                    .unwrap();
                0x1000_0004
            }
            _ => STACK,
        };
        let before = prepare(&mut process, stack, RETURN);
        let frame_before = if stack >= 16 {
            Some(words(&process, stack - 16))
        } else {
            None
        };
        match mode {
            1 => process
                .memory
                .protect(0x1000_0000, PAGE_SIZE, Permissions::READ)
                .unwrap(),
            2 => process
                .memory
                .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ)
                .unwrap(),
            3 => {
                process.cpu.set_register(Register32::Esp, 0xffff_fffe);
            }
            _ => {}
        }
        let before = if mode == 3 { process.cpu } else { before };
        let result = process.run(1);
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
        if let Some(frame) = frame_before {
            assert_eq!(words(&process, stack - 16), frame);
        }
        let mut chain = [0; 4];
        process.memory.read(0x7ffd_e000, &mut chain).unwrap();
        assert_eq!(chain, [0xff; 4]);
    }
}

#[test]
fn fs_slot_overlapping_any_frame_byte_is_explicitly_unsupported() {
    for offset in [-3_i32, 0, 4, 16, 19] {
        let mut process = process();
        process
            .cpu
            .set_fs_base((STACK - 16).wrapping_add_signed(offset));
        let before = prepare(&mut process, STACK, RETURN);
        let frame = words(&process, STACK - 16);
        let result = process.run(1);
        assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: API });
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
        assert_eq!(words(&process, STACK - 16), frame);
    }
}
