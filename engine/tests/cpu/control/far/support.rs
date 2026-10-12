use ring3_engine::{
    cpu::x86::{
        decode::{DecodeError, decode_one},
        ir::Operation,
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::EngineInstance,
};

pub fn decode(bytes: &[u8]) -> Result<Operation, DecodeError> {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    let decoded = decode_one(&memory, GuestAddress(0x1000))?;
    assert_eq!(usize::from(decoded.length()), bytes.len());
    assert!(memory.is_code_current(decoded.code_snapshot()));
    Ok(*decoded.operation())
}

pub fn bound(bytes: &[u8]) {
    closed(bytes);
    for (resident, entries) in [(false, false), (false, true), (true, false), (true, true)] {
        let mut engine = EngineInstance::new(4, 0x464152).unwrap();
        engine.map(0x1000, 1, 7).unwrap();
        engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
        engine.upload(0x1000, bytes.len() as u32).unwrap();
        engine.protect(0x1000, 1, 4).unwrap();
        engine.arena_mut().unwrap()[140..144].copy_from_slice(&0x1000_u32.to_le_bytes());
        if !entries {
            engine.arena_mut().unwrap()[144..148]
                .copy_from_slice(&(bytes.len() as u32).to_le_bytes());
        }
        let before = engine.arena()[..140].to_vec();
        match (resident, entries) {
            (false, false) => engine.compile(1).map(|_| ()),
            (false, true) => engine.compile_entries(1, 0).map(|_| ()),
            (true, false) => engine.compile_resident(1).map(|_| ()),
            (true, true) => engine.compile_resident_entries(1, 0).map(|_| ()),
        }
        .unwrap();
        assert_eq!(&engine.arena()[..140], before);
        assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
    }
}

fn closed(bytes: &[u8]) {
    use ring3_engine::cpu::dbt::{
        BlockSpec, CompileError, CompileLimits, InstructionError, prepare_entry_region,
        prepare_region,
    };
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    let before = memory
        .snapshot_code(GuestAddress(0x1000), bytes.len())
        .unwrap();
    let before_pages = memory.mapped_pages();
    let expected = Some(CompileError::Instruction {
        pc: GuestAddress(0x1000),
        cause: InstructionError::BackendUnsupported,
    });
    assert_eq!(
        prepare_region(
            &memory,
            &[BlockSpec {
                entry: GuestAddress(0x1000),
                byte_length: bytes.len() as u32
            }],
            CompileLimits::default()
        )
        .err(),
        expected
    );
    assert_eq!(
        prepare_entry_region(&memory, &[GuestAddress(0x1000)], CompileLimits::default()).err(),
        expected
    );
    assert!(memory.is_code_current(&before));
    let mut after = vec![0; bytes.len()];
    memory.read(GuestAddress(0x1000), &mut after).unwrap();
    assert_eq!(after, bytes);
    assert_eq!(memory.mapped_pages(), before_pages);
}
