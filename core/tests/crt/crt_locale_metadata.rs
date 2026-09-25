use super::locale_metadata_executable;

use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

fn word(p: &Process32, address: u32) -> u32 {
    let mut bytes = [0; 4];
    p.memory.read(u64::from(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

#[test]
fn imported_c_locale_data_has_correct_shape_isolation_and_permissions() {
    let bytes = locale_metadata_executable::pe32();
    let mut p = Process32::load(&bytes, 32).unwrap();
    let fresh = Process32::load(&bytes, 32).unwrap();
    let addresses = [0x7000_202c, 0x7000_2044, 0x7000_2048, 0x7000_204c];
    for (index, address) in addresses.into_iter().enumerate() {
        assert_eq!(
            word(&p, 0x0040_2060 + u32::try_from(index).unwrap() * 4),
            address
        );
    }
    for index in 0..9 {
        let address = addresses[0] + index * 4;
        let initial = u32::from(index == 8);
        assert_eq!(word(&p, address), initial);
        p.memory
            .write(u64::from(address), &(100 + index).to_le_bytes())
            .unwrap();
        assert_eq!(word(&fresh, address), initial);
        assert!(p.memory.fetch(u64::from(address), &mut [0]).is_err());
    }
    for index in 0..9 {
        assert_eq!(word(&p, addresses[0] + index * 4), 100 + index);
    }
    assert_eq!(word(&p, addresses[0] - 4), 0);
    assert_eq!(word(&p, addresses[3] + 4), 0);
    assert_eq!(word(&p, 0x7000_2000), 0x4000);
    assert_eq!(p.memory.mapped_pages(), fresh.memory.mapped_pages());
}

#[test]
fn multibyte_selection_preserves_c_locale_data() {
    let mut p = Process32::load(&locale_metadata_executable::pe32(), 32).unwrap();
    let mut before = [0; 36];
    p.memory.read(0x7000_202c, &mut before).unwrap();
    for codepage in [
        1252_u32,
        437,
        0,
        (-2_i32).cast_unsigned(),
        (-3_i32).cast_unsigned(),
    ] {
        p.cpu.eip = 0x7000_013c;
        p.cpu.set_register(Register32::Esp, 0x1000_ff00);
        p.memory
            .write(0x1000_ff00, &0x0040_1000_u32.to_le_bytes())
            .unwrap();
        p.memory
            .write(0x1000_ff04, &codepage.to_le_bytes())
            .unwrap();
        assert_eq!(p.run(1).api_calls, 1);
        assert_eq!(p.cpu.register(Register32::Eax), 0);
        let mut after = [0; 36];
        p.memory.read(0x7000_202c, &mut after).unwrap();
        assert_eq!(after, before);
    }
}

#[test]
fn imported_locale_guest_agrees_whole_and_single_step() {
    for budget in [1, 50] {
        let mut p = Process32::load(&locale_metadata_executable::pe32(), 32).unwrap();
        let mut instructions = 0;
        loop {
            let result = p.run(budget);
            instructions += result.instructions;
            assert_eq!(result.api_calls, 0);
            if result.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                break;
            }
            assert!(instructions < 50);
        }
        assert_eq!(instructions, 12);
        for (register, expected) in [
            (Register32::Eax, 0),
            (Register32::Ebx, 42),
            (Register32::Ecx, 0),
            (Register32::Edx, 0),
            (Register32::Ebp, 1),
        ] {
            assert_eq!(p.cpu.register(register), expected);
        }
        assert_eq!(word(&p, 0x7000_2040), 42);
        assert_eq!(word(&p, 0x7000_204c), 2);
    }
}
