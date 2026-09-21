#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/locale_activity_executable.rs"]
mod locale_activity_executable;

use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

fn word(p: &Process32, address: u32) -> u32 {
    let mut bytes = [0; 4];
    p.memory.read(u64::from(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

#[test]
fn imported_data_cells_are_distinct_mutable_and_process_local() {
    let bytes = locale_activity_executable::pe32();
    let mut first = Process32::load(&bytes, 32).unwrap();
    let second = Process32::load(&bytes, 32).unwrap();
    let reader = word(&first, 0x0040_2060);
    let writer = word(&first, 0x0040_2064);
    assert_eq!((reader, writer), (0x7000_2024, 0x7000_2028));
    assert_eq!((word(&first, reader), word(&first, writer)), (0, 0));
    first
        .memory
        .write(u64::from(reader), &42_u32.to_le_bytes())
        .unwrap();
    first
        .memory
        .write(u64::from(writer), &7_u32.to_le_bytes())
        .unwrap();
    assert_eq!((word(&second, reader), word(&second, writer)), (0, 0));
    assert_eq!((word(&first, reader), word(&first, writer)), (42, 7));
    assert_eq!(word(&first, reader - 4), 0);
    assert_eq!(word(&first, writer + 4), 0);
    assert!(first.memory.fetch(u64::from(reader), &mut [0]).is_err());
}

#[test]
fn generic_counter_calls_share_the_directly_imported_cells() {
    let mut p = Process32::load(&locale_activity_executable::pe32(), 32).unwrap();
    let pages = p.memory.mapped_pages();
    for slot in [0x0040_2060, 0x0040_2064] {
        let address = word(&p, slot);
        for (api, expected) in [
            (0x7000_0204, 1),
            (0x7000_0204, 2),
            (0x7000_0208, 1),
            (0x7000_0208, 0),
        ] {
            p.cpu.eip = api;
            p.cpu.set_register(Register32::Esp, 0x1000_ff00);
            p.memory
                .write(0x1000_ff00, &0x0040_1000_u32.to_le_bytes())
                .unwrap();
            p.memory.write(0x1000_ff04, &address.to_le_bytes()).unwrap();
            assert_eq!(p.run(1).api_calls, 1);
            assert_eq!(p.cpu.register(Register32::Eax), expected);
            assert_eq!(word(&p, address), expected);
        }
    }
    assert_eq!(p.memory.mapped_pages(), pages);
}

#[test]
fn direct_data_guest_agrees_whole_and_single_step() {
    for budget in [1, 50] {
        let mut p = Process32::load(&locale_activity_executable::pe32(), 32).unwrap();
        let mut count = 0;
        loop {
            let result = p.run(budget);
            count += result.instructions;
            assert_eq!(result.api_calls, 0);
            if result.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                break;
            }
            assert!(count < 50);
        }
        assert_eq!(count, 9);
        assert_eq!(p.cpu.register(Register32::Eax), 0);
        assert_eq!(p.cpu.register(Register32::Edx), 0);
        assert_eq!(p.cpu.register(Register32::Esi), 42);
        assert_eq!(p.cpu.register(Register32::Edi), 7);
    }
}
