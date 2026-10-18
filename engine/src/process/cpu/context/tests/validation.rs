use super::*;
use crate::process::EngineInstance;

#[test]
fn validated_snapshots_preserve_every_opaque_vector_and_float_payload_bit() {
    let mut engine = EngineInstance::new(1, 71).unwrap();
    for (start, length) in [
        (X87_OFFSET + 40, 80),
        (SIMD_OFFSET + 32, 128),
        (YMM_OFFSET + 16, 128),
    ] {
        for offset in start..start + length {
            for bit in 0..8 {
                engine.arena_mut().unwrap()[offset] ^= 1 << bit;
                let captured = ExtendedCpuState::capture(engine.arena()).unwrap();
                assert!(captured.matches(engine.arena()));
                let mut restored = engine.arena().to_vec();
                restored[offset] ^= 1 << bit;
                captured.restore(&mut restored);
                assert_eq!(restored, engine.arena());
                engine.arena_mut().unwrap()[offset] ^= 1 << bit;
            }
        }
    }
}

#[test]
fn every_capture_revalidates_header_reserved_and_control_fields_after_a_valid_snapshot() {
    let mut engine = EngineInstance::new(1, 71).unwrap();
    let original = engine.arena().to_vec();
    for (offset, bit) in [
        (X87_OFFSET, 1),
        (X87_OFFSET + 6, 2),
        (X87_OFFSET + 23, 8),
        (X87_OFFSET + 36, 1),
        (X87_OFFSET + 127, 1),
        (SIMD_OFFSET + 8, 1),
        (SIMD_OFFSET + 18, 1),
        (SIMD_OFFSET + 24, 1),
        (YMM_OFFSET + 4, 1),
        (YMM_OFFSET + 12, 1),
    ] {
        ExtendedCpuState::capture(engine.arena()).unwrap();
        engine.arena_mut().unwrap()[offset] ^= bit;
        let malformed = engine.arena().to_vec();
        assert_eq!(
            ExtendedCpuState::capture(engine.arena()),
            Err(HostError::Call(CallError::InvalidStop))
        );
        assert_eq!(engine.arena(), malformed);
        engine.arena_mut().unwrap().copy_from_slice(&original);
    }
}
