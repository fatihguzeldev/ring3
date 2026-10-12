use ring3_engine::{abi::arena::TRANSFER_OFFSET, process::EngineInstance};
use std::{fs::OpenOptions, io::Write};

#[test]
fn existing_memory_push_store_result_module_is_preserved() {
    let mut engine = EngineInstance::new(1, 1).unwrap();
    let pc = 0x1000_u32;
    let program = [0xff, 0x33, 0xeb, 0];
    engine.map(pc, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + program.len()]
        .copy_from_slice(&program);
    engine.upload(pc, program.len() as u32).unwrap();
    engine.protect(pc, 1, 4).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[pc.to_le_bytes(), (program.len() as u32).to_le_bytes()].concat());
    engine.compile(1).unwrap();
    let bytes = engine.artifact_bytes().unwrap();
    assert!(bytes.starts_with(b"\0asm\x01\0\0\0"));
    if let Some(path) = std::env::var_os("RING3_STORE_RESULT_BRIDGE_OUTPUT") {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap()
            .write_all(bytes)
            .unwrap();
    }
}
