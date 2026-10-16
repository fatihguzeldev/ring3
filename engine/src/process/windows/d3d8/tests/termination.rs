use super::support::runtime;
use crate::{
    abi::graphics::d3d8::packet,
    graphics::d3d8::resources::Rect,
    process::{EngineInstance, HostError},
};
#[test]
fn final_graphics_records_drain_before_removed_and_backend_is_reclaimed_only_after_ack() {
    let (mut runtime, memory, pointer) = runtime();
    runtime
        .device_mut(pointer)
        .unwrap()
        .clear(
            1,
            0xff123456,
            1.0f32.to_bits(),
            0,
            &[Rect {
                left: 0,
                top: 0,
                right: 16,
                bottom: 16,
            }],
        )
        .unwrap();
    let epoch = runtime.device(pointer).unwrap().queue().epoch();
    let mut engine = EngineInstance::new(8, 7).unwrap();
    engine.memory = Some(memory);
    engine.d3d8 = Some(runtime);
    let prepared = engine.prepare_media_process_exit().unwrap();
    engine.commit_media_process_exit(prepared);
    engine.exit_code = Some(0);
    let mut opcodes = Vec::new();
    let mut removed = 0;
    while let Some(id) = engine.d3d8_seal_batch(epoch).unwrap() {
        let (same, length) = engine.d3d8_batch_info(epoch).unwrap().unwrap();
        assert_eq!(same, id);
        let mut bytes = vec![0; length];
        engine.d3d8_copy_batch(epoch, 0, &mut bytes).unwrap();
        let (_, fragments) = packet(&bytes).unwrap();
        for fragment in fragments {
            opcodes.push(fragment.opcode);
            if fragment.opcode == 13 {
                removed += 1;
                assert_eq!(
                    u32::from_le_bytes(fragment.bytes[8..12].try_into().unwrap()),
                    3
                );
            }
        }
        assert!(engine.d3d8_device(0).unwrap().is_some());
        assert_eq!(engine.d3d8_seal_batch(epoch).unwrap(), Some(id));
        engine.d3d8_acknowledge_batch(id).unwrap();
    }
    assert!(opcodes.contains(&9));
    assert_eq!(removed, 1);
    assert_eq!(opcodes.last(), Some(&13));
    assert_eq!(engine.d3d8_device(0).unwrap(), None);
    assert!(matches!(engine.memory(), Err(HostError::ProcessExited)));
    assert!(
        engine
            .d3d8_acknowledge_batch(crate::graphics::BatchId { epoch, sequence: 1 })
            .is_err()
    );
}
