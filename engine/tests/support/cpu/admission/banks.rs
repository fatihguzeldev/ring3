use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    cpu::{dbt::CompileLimits, x86::decode::decode_one},
    memory::GuestAddress,
    process::EngineInstance,
};

// dense synthetic banks may exceed the unchanged wasm ceiling before the instruction cap.
// keep their whole-unit refusal separately; every original operation is admitted here.
pub fn assert_bounded_memory_bank(bytes: &[u8], root: u32, key: u64, resident: bool, entries: bool) {
    assert!(bytes.ends_with(&[0xeb, 0]));
    let mut source = EngineInstance::new(1, key).unwrap();
    source.map(root & !0xfff, 1, 7).unwrap();
    source.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()].copy_from_slice(bytes);
    source.upload(root, bytes.len() as u32).unwrap();
    let mut offsets = Vec::new();
    let mut offset = 0;
    while offset < bytes.len() - 2 {
        let decoded = decode_one(source.memory().unwrap(), GuestAddress(root + offset as u32)).unwrap();
        let length = usize::from(decoded.length());
        assert!(length > 0 && offset + length <= bytes.len() - 2);
        offsets.push((offset, length));
        offset += length;
    }
    assert_eq!(offset, bytes.len() - 2);
    let mut covered = 0;
    for group in offsets.chunks(8) {
        let start = group[0].0;
        let &(last, length) = group.last().unwrap();
        let end = last + length;
        let mut chunk = bytes[start..end].to_vec();
        chunk.extend_from_slice(&bytes[bytes.len() - 2..]);
        let pc = root + start as u32;
        let mut engine = EngineInstance::new(1, key).unwrap();
        engine.map(root & !0xfff, 1, 7).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + chunk.len()].copy_from_slice(&chunk);
        engine.upload(pc, chunk.len() as u32).unwrap();
        engine.protect(root & !0xfff, 1, 4).unwrap();
        let snapshot = engine.memory().unwrap().snapshot_code(GuestAddress(pc), chunk.len()).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
            .copy_from_slice(&[pc.to_le_bytes(), (chunk.len() as u32).to_le_bytes()].concat());
        let arena = engine.arena().to_vec();
        let id = match (resident, entries) {
            (false, false) => u64::from(engine.compile(1).unwrap()),
            (false, true) => u64::from(engine.compile_entries(1, 0).unwrap()),
            (true, false) => engine.compile_resident(1).unwrap().get(),
            (true, true) => engine.compile_resident_entries(1, 0).unwrap().get(),
        };
        let module = if resident {
            engine.guard_resident(key, id).unwrap();
            for &(offset, length) in group {
                assert_eq!(engine.lookup_resident(root + offset as u32).unwrap().get(), id);
                if length > 1 { assert!(engine.lookup_resident(root + offset as u32 + 1).is_err()); }
            }
            if end == bytes.len() - 2 { assert_eq!(engine.lookup_resident(root + end as u32).unwrap().get(), id); }
            engine.resident_bytes(id).unwrap()
        } else {
            engine.guard(key, id as u32).unwrap();
            engine.artifact_bytes().unwrap()
        };
        assert_eq!(&module[..8], b"\0asm\x01\0\0\0");
        assert!(module.len() <= CompileLimits::default().wasm_bytes);
        assert_eq!(engine.arena(), arena);
        assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
        assert!(engine.memory().unwrap().is_code_current(&snapshot));
        covered += end - start;
    }
    assert_eq!(covered, bytes.len() - 2, "every original operation byte was admitted");
}
