use super::delay_executable;

use ring3_core::execution::{
    LoadError, Process32, ProcessStop, Register32, StopReason, load_pe32, load_pe32_with_imports,
};

fn word(process: &Process32, address: u32) -> u32 {
    let mut bytes = [0; 4];
    process.memory.read(u64::from(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

#[test]
fn delay_helpers_own_resolution_and_cache_their_target_in_guest_memory() {
    for legacy in [true, false] {
        let bytes = delay_executable::pe32(legacy, false);
        let mut process = Process32::load(&bytes, 32).unwrap();
        assert_eq!(word(&process, 0x0040_2180), 0x0040_10c0);
        assert_eq!(word(&process, 0x0040_2188), 0);
        assert_eq!(word(&process, 0x0040_2190), 0);
        let mut mapped = [0; 64];
        process.memory.read(0x0040_2200, &mut mapped).unwrap();
        assert_eq!(mapped, bytes[1536..1600]);
        let mut instructions = 0;
        for _ in 0..50 {
            let result = process.run(1);
            instructions += result.instructions;
            if result.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                break;
            }
        }
        assert_eq!(instructions, 12);
        assert_eq!(process.cpu.register(Register32::Eax), 42);
        assert_eq!(process.cpu.register(Register32::Ebx), 42);
        assert_eq!(word(&process, 0x0040_2180), 0x0040_1080);
        assert_eq!(word(&process, 0x0040_2190), 1);
        assert_eq!(word(&process, 0x0040_2188), 0);
    }
}

#[test]
fn malformed_raw_delay_prefix_fails_and_low_level_loader_policies_stay_strict() {
    let bytes = delay_executable::pe32(false, false);
    for result in [
        load_pe32(&bytes, 32),
        load_pe32_with_imports(&bytes, 32, |_, _| None),
    ] {
        assert!(matches!(
            result,
            Err(LoadError::UnsupportedDirectory { index: 13 })
        ));
    }
    let mut bytes = bytes;
    bytes[0x164..0x168].copy_from_slice(&32_u32.to_le_bytes());
    assert!(matches!(
        Process32::load(&bytes, 32),
        Err(LoadError::DelayImports(_))
    ));
}

#[test]
fn unknown_helper_api_is_a_real_stop_and_no_resolution_is_faked() {
    let mut process = Process32::load_diagnostic(&delay_executable::pe32(true, true), 32).unwrap();
    assert!(
        matches!(process.run(100).reason, ProcessStop::UnresolvedImport { module, symbol, .. }
        if module == "KERNEL32.dll" && symbol == "MissingDelayApi")
    );
    assert_eq!(word(&process, 0x0040_2180), 0x0040_10c0);
    assert_eq!(word(&process, 0x0040_2190), 0);
}
