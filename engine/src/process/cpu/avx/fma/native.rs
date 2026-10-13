use crate::process::{EngineInstance, HostError};

#[test]
fn canonical_fused_helper_gate_is_effect_neutral_before_activation() {
    if crate::cpu::x86::capabilities::FMA_ENABLED {
        return;
    }
    let mut engine = EngineInstance::new(1, 0x464d41).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[140..142].copy_from_slice(&[0xeb, 0xfe]);
    engine.upload(0x1000, 2).unwrap();
    engine.arena_mut().unwrap()[140..148].copy_from_slice(&[0, 0x10, 0, 0, 2, 0, 0, 0]);
    engine.compile(1).unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.avx_execute(0x0100_1301, 1 | 2 << 3 | 3 << 6, 0, 0, 0, 2),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(engine.arena(), before);
}
