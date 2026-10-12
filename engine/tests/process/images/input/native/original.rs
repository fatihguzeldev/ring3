use ring3_engine::{
    abi::{
        arena::{ARENA_SIZE, X87_OFFSET},
        x86::decode_x87,
    },
    loader::{ImageMetadata32, LoadError},
    memory::GuestAddress,
    process::{EngineInstance, HostError},
};

const ORIGINAL: &[u8] = include_bytes!("../../relocation/fixtures/p2-pe32-relocatable/relocations.exe");
const KEY: u64 = 0xa349_cdef_1234_5678;
const PREFERRED: u32 = 0x0040_0000;

fn engine() -> EngineInstance {
    let mut engine = EngineInstance::new(5, KEY).unwrap();
    let arena = engine.arena_mut().unwrap();
    assert_eq!(arena.len(), ARENA_SIZE);
    assert_eq!(ARENA_SIZE, 4612);
    arena[X87_OFFSET + 16..X87_OFFSET + 18].copy_from_slice(&0x027f_u16.to_le_bytes());
    arena[X87_OFFSET + 18..X87_OFFSET + 20].copy_from_slice(&0x81a5_u16.to_le_bytes());
    for (index, byte) in arena[X87_OFFSET + 40..X87_OFFSET + 120]
        .iter_mut()
        .enumerate()
    {
        *byte = (index as u8).wrapping_mul(29).wrapping_add(17);
    }
    decode_x87(&arena[X87_OFFSET..X87_OFFSET + ring3_engine::abi::x86::X87_SIZE]).unwrap();
    engine
}

fn append_owned_chunks(engine: &mut EngineInstance) {
    assert_eq!(ORIGINAL.len(), 6144);
    engine.begin_image_input(ORIGINAL.len() as u32).unwrap();
    for (index, bytes) in ORIGINAL.chunks(4096).enumerate() {
        let mut copied = bytes.to_vec();
        engine
            .append_image_input((index * 4096) as u32, &copied)
            .unwrap();
        copied.fill(0xee);
    }
}

fn check_original(engine: &EngineInstance, base: u32) {
    let memory = engine.memory().unwrap();
    assert_eq!(memory.capacity_pages(), 5);
    assert_eq!(memory.mapped_pages(), 5);
    let mut entry = [0; 3];
    memory
        .read(GuestAddress(base + 0x1000), &mut entry)
        .unwrap();
    assert_eq!(entry, [0x31, 0xc0, 0xc3]);
    for rva in [0x2008, 0x200c, 0x3010] {
        let mut word = [0; 4];
        memory.read(GuestAddress(base + rva), &mut word).unwrap();
        assert_eq!(u32::from_le_bytes(word), base + 0x2000);
    }
    let mut relocations = [0; 24];
    memory
        .read(GuestAddress(base + 0x4000), &mut relocations)
        .unwrap();
    assert_eq!(relocations, ORIGINAL[5632..5656]);
}

#[test]
fn staged_original_owns_both_chunks_and_matches_direct_publication_at_three_bases() {
    for base in [PREFERRED, 0x0050_0000, 0x0030_0000] {
        let mut staged = engine();
        let arena = staged.arena().to_vec();
        append_owned_chunks(&mut staged);
        assert_eq!(staged.arena(), arena);
        let image = staged.load_pe32_input_at(base).unwrap();
        assert_eq!(
            image,
            ImageMetadata32 {
                image_base: base,
                image_size: 0x5000,
                entry_point: base + 0x1000,
                mapped_pages: 5,
            }
        );
        assert_eq!(staged.arena(), arena);
        check_original(&staged, base);
        let mut direct = engine();
        assert_eq!(direct.load_pe32_at(ORIGINAL, base).unwrap(), image);
        check_original(&direct, base);
        assert_eq!(
            staged.load_pe32_input_at(base),
            Err(HostError::InvalidRequest)
        );
        assert_eq!(
            staged.append_image_input(6144, b"x"),
            Err(HostError::InvalidRequest)
        );
        assert_eq!(staged.begin_image_input(1), Err(HostError::InvalidRequest));
        assert_eq!(staged.arena(), arena);
    }
}

#[test]
fn incomplete_then_bad_base_refusals_retain_original_for_retry_without_reupload() {
    let mut engine = engine();
    let arena = engine.arena().to_vec();
    assert_eq!(engine.load_pe32_input_at(0), Err(HostError::InvalidRequest));
    engine.begin_image_input(6144).unwrap();
    engine.append_image_input(0, &ORIGINAL[..4096]).unwrap();
    assert_eq!(
        engine.load_pe32_input_at(PREFERRED),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(engine.memory().unwrap().mapped_pages(), 0);
    assert_eq!(engine.arena(), arena);
    engine.append_image_input(4096, &ORIGINAL[4096..]).unwrap();
    for base in [0, PREFERRED + 1] {
        for _ in 0..2 {
            assert_eq!(
                engine.load_pe32_input_at(base),
                Err(HostError::Loader(LoadError::Malformed))
            );
            assert_eq!(engine.memory().unwrap().mapped_pages(), 0);
            assert_eq!(engine.arena(), arena);
        }
    }
    engine.load_pe32_input_at(PREFERRED).unwrap();
    engine.abort_image_input().unwrap();
    engine.abort_image_input().unwrap();
    check_original(&engine, PREFERRED);
    assert_eq!(engine.arena(), arena);
}

#[test]
fn old_direct_loader_can_win_while_staging_and_abort_does_not_unload_its_image() {
    let mut engine = engine();
    let arena = engine.arena().to_vec();
    engine.begin_image_input(6144).unwrap();
    engine.append_image_input(0, &ORIGINAL[..7]).unwrap();
    engine.load_pe32_at(ORIGINAL, PREFERRED).unwrap();
    assert_eq!(engine.load_pe32_input_at(0), Err(HostError::InvalidRequest));
    assert_eq!(
        engine.append_image_input(7, &ORIGINAL[7..8]),
        Err(HostError::InvalidRequest)
    );
    engine.abort_image_input().unwrap();
    engine.abort_image_input().unwrap();
    check_original(&engine, PREFERRED);
    assert_eq!(engine.begin_image_input(1), Err(HostError::InvalidRequest));
    assert_eq!(engine.arena(), arena);
}
