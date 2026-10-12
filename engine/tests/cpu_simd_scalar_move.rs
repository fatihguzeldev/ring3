use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    cpu::x86::{decode::decode_one, ir::Operation, simd::SimdKind},
    memory::{Access, GuestAddress},
    process::EngineInstance,
};

#[test]
fn scalar_moves_keep_exact_width_and_compile_without_operand_access() {
    for prefix in [0xf3, 0xf2] {
        let width = if prefix == 0xf3 { 4 } else { 8 };
        for (opcode, operand, memory) in [
            (0x10, 0xc1, false),
            (0x11, 0xc1, false),
            (0x10, 0xc0, false),
            (0x10, 0x00, true),
            (0x11, 0x00, true),
        ] {
            for (resident, entries) in [(false, false), (false, true), (true, false), (true, true)]
            {
                let mut engine = EngineInstance::new(1, 7).unwrap();
                engine.map(0x2000, 1, 7).unwrap();
                let bytes = [prefix, 0x0f, opcode, operand, 0xeb, 0xfe];
                engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
                    .copy_from_slice(&bytes);
                engine.upload(0x2000, bytes.len() as u32).unwrap();
                engine.protect(0x2000, 1, 4).unwrap();
                let before = engine.arena().to_vec();
                let decoded = decode_one(engine.memory().unwrap(), GuestAddress(0x2000)).unwrap();
                assert_eq!(decoded.length(), 4);
                assert_eq!(decoded.next_pc(), GuestAddress(0x2004));
                let Operation::Simd(operation) = decoded.operation() else {
                    panic!("expected a typed scalar move");
                };
                assert_eq!(operation.kind, SimdKind::MoveScalar);
                assert_eq!(operation.lane_bytes, width);
                assert_eq!(operation.vector_bytes, 16);
                assert_eq!(
                    operation.memory_bytes,
                    if memory { u16::from(width) } else { 0 }
                );
                assert_eq!(engine.arena(), before);
                let request = if entries {
                    vec![0x2000]
                } else {
                    vec![0x2000, 6]
                };
                for (index, value) in request.into_iter().enumerate() {
                    let at = TRANSFER_OFFSET + index * 4;
                    engine.arena_mut().unwrap()[at..at + 4]
                        .copy_from_slice(&u32::to_le_bytes(value));
                }
                let before = engine.arena().to_vec();
                let id = match (resident, entries) {
                    (false, false) => u64::from(engine.compile(1).unwrap()),
                    (false, true) => u64::from(engine.compile_entries(1, 0).unwrap()),
                    (true, false) => engine.compile_resident(1).unwrap().get(),
                    (true, true) => engine.compile_resident_entries(1, 0).unwrap().get(),
                };
                let bytes = if resident {
                    engine.resident_bytes(id).unwrap()
                } else {
                    engine.artifact_bytes().unwrap()
                };
                assert_eq!(&bytes[..8], b"\0asm\x01\0\0\0");
                assert!(
                    !bytes
                        .windows(b"simd_execute".len())
                        .any(|name| name == b"simd_execute"),
                    "scalar-only modules must have no semantic execution helper import"
                );
                assert_eq!(engine.arena(), before);
                let memory = engine.memory().unwrap();
                assert_eq!(memory.mapped_pages(), 1);
                for address in [0, 0x2000, 0x5000] {
                    assert!(memory.resolve(GuestAddress(address), Access::Read).is_err());
                }
            }
        }
    }
}
