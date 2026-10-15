use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    cpu::dbt::{CompileError, CompileLimits},
    memory::GuestAddress,
    process::{EngineInstance, HostError},
};

pub fn assert_bounded_emission(bytes: &[u8], key: u64) {
    for entries in [false, true] {
        let mut engine = EngineInstance::new(1, key).unwrap();
        engine.map(0x1000, 1, 7).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
            .copy_from_slice(bytes);
        engine.upload(0x1000, bytes.len() as u32).unwrap();
        engine.protect(0x1000, 1, 4).unwrap();
        let descriptor = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
        descriptor[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
        descriptor[4..].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
        let arena = engine.arena().to_vec();
        let code = engine
            .memory()
            .unwrap()
            .snapshot_code(GuestAddress(0x1000), bytes.len())
            .unwrap();
        if entries {
            assert_eq!(engine.compile_entries(1, 0), Ok(1));
            engine.guard(key, 1).unwrap();
            assert!(engine.artifact_bytes().unwrap().len() <= CompileLimits::default().wasm_bytes);
        } else {
            assert_eq!(
                engine.compile(1),
                Err(HostError::Compile(CompileError::WasmLimit))
            );
            assert_eq!(engine.generation(), 0);
            assert_eq!(engine.artifact_bytes(), Err(HostError::InvalidArtifact));
        }
        assert_eq!(engine.arena(), arena);
        assert!(engine.memory().unwrap().is_code_current(&code));
        assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
        let mut actual = vec![0; bytes.len()];
        engine
            .memory()
            .unwrap()
            .fetch(GuestAddress(0x1000), &mut actual)
            .unwrap();
        assert_eq!(actual, bytes);
    }
}
