use ring3_engine::{
    abi::arena::{CANCEL_OFFSET, TRANSFER_OFFSET},
    cpu::dbt::GateSpec,
    loader::{ImageMetadata32, LinkedImageMetadata32V2, LoadError},
    memory::{Access, AddressSpace, FaultReason, GuestAddress, MemoryError, MemoryFault},
    process::{EngineInstance, HostError},
};

const KEY: u64 = 0xa349_cdef_1234_5678;
const BASE: u32 = 0x0100_0000;
const GATE: u32 = 0x7100_0000;
const FILE_SIZE: usize = 0x1600;
const INPUT_LIMIT: u32 = 16 * 1024 * 1024;

fn put16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn image() -> Vec<u8> {
    // independently authored file; initialized data and import names follow byte4096.
    let mut bytes = vec![0; FILE_SIZE];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 0x3c, 0x80);
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    for (at, value) in [
        (0x84, 0x014c),
        (0x86, 3),
        (0x94, 224),
        (0x96, 0x0103),
        (0x98, 0x010b),
        (0xdc, 3),
    ] {
        put16(&mut bytes, at, value);
    }
    for (at, value) in [
        (0xa8, 0x1000),
        (0xb4, BASE),
        (0xb8, 4096),
        (0xbc, 512),
        (0xd0, 0x5000),
        (0xd4, 0x400),
        (0xf4, 16),
        (0x100, 0x4000),
        (0x104, 40),
        (0x158, 0x4140),
        (0x15c, 8),
    ] {
        put32(&mut bytes, at, value);
    }
    for (at, name, rva, virtual_size, raw, raw_size, flags) in [
        (
            0x178,
            *b".text\0\0\0",
            0x1000,
            0x200,
            0x400,
            0x200,
            0x6000_0020,
        ),
        (
            0x1a0,
            *b".data\0\0\0",
            0x3000,
            0x208,
            0x1000,
            0x200,
            0xc000_0040,
        ),
        (
            0x1c8,
            *b".imports",
            0x4000,
            0x400,
            0x1200,
            0x400,
            0x4000_0040,
        ),
    ] {
        bytes[at..at + 8].copy_from_slice(&name);
        for (field, value) in [
            (8, virtual_size),
            (12, rva),
            (16, raw_size),
            (20, raw),
            (36, flags),
        ] {
            put32(&mut bytes, at + field, value);
        }
    }
    bytes[0x400..0x407].copy_from_slice(&[0xb8, 0x78, 0x56, 0x34, 0x12, 0xeb, 0]);
    put32(&mut bytes, 0x1000, 0xa1b2_c3d4);
    bytes[0x11ff] = 0x6d;
    for (at, value) in [
        (0x1200, 0x4100),
        (0x120c, 0x4180),
        (0x1210, 0x4140),
        (0x1300, 0x4190),
        (0x1340, 0x4190),
    ] {
        put32(&mut bytes, at, value);
    }
    bytes[0x1380..0x138d].copy_from_slice(b"KeRnEl32.dLl\0");
    put16(&mut bytes, 0x1390, 0xbeef);
    bytes[0x1392..0x139e].copy_from_slice(b"ExitProcess\0");
    bytes
}

fn metadata(gate: u32) -> LinkedImageMetadata32V2 {
    LinkedImageMetadata32V2 {
        image: ImageMetadata32 {
            image_base: BASE,
            image_size: 0x5000,
            entry_point: BASE + 0x1000,
            mapped_pages: 5,
        },
        gate_base: gate,
        gate_count: 1,
        gates: [
            GateSpec {
                entry: GuestAddress(gate + 32),
                id: 0x0001_0003,
            },
            GateSpec {
                entry: GuestAddress(0),
                id: 0,
            },
            GateSpec {
                entry: GuestAddress(0),
                id: 0,
            },
        ],
    }
}

fn stage(engine: &mut EngineInstance, bytes: &[u8]) {
    engine.begin_image_input(bytes.len() as u32).unwrap();
    append_from(engine, bytes, 0);
}

fn append_from(engine: &mut EngineInstance, bytes: &[u8], start: usize) {
    for (index, chunk) in bytes[start..].chunks(4096).enumerate() {
        engine
            .append_image_input((start + index * 4096) as u32, chunk)
            .unwrap();
    }
}

fn read(memory: &AddressSpace, address: u32, length: usize) -> Vec<u8> {
    let mut bytes = vec![0; length];
    memory.read(GuestAddress(address), &mut bytes).unwrap();
    bytes
}

fn fault(address: u32, access: Access, reason: FaultReason) -> MemoryError {
    MemoryError::Fault(MemoryFault {
        address: GuestAddress(address),
        access,
        reason,
    })
}

fn check_image(engine: &EngineInstance, source: &[u8], gate: u32) {
    let memory = engine.memory().unwrap();
    assert_eq!(memory.mapped_pages(), 5);
    assert_eq!(read(memory, BASE, 0x400), source[..0x400]);
    assert_eq!(read(memory, BASE + 0x1000, 0x200), source[0x400..0x600]);
    assert_eq!(read(memory, BASE + 0x3000, 0x200), source[0x1000..0x1200]);
    assert_eq!(read(memory, BASE + 0x3200, 0xe00), vec![0; 0xe00]);
    assert_eq!(
        read(memory, BASE + 0x4140, 8),
        [gate + 32, 0].map(u32::to_le_bytes).concat()
    );
    assert_eq!(read(memory, BASE + 0x4180, 13), b"KeRnEl32.dLl\0");
    let mut expected_gate = vec![0; 4096];
    expected_gate[32..34].copy_from_slice(&[0x0f, 0x0b]);
    assert_eq!(read(memory, gate, 4096), expected_gate);
    for (address, allowed, denied) in [
        (BASE, Access::Read, Access::Write),
        (BASE + 0x1000, Access::Execute, Access::Write),
        (BASE + 0x3000, Access::Write, Access::Execute),
        (BASE + 0x4000, Access::Read, Access::Write),
        (gate + 32, Access::Execute, Access::Write),
    ] {
        assert!(memory.resolve(GuestAddress(address), allowed).is_ok());
        assert_eq!(
            memory.resolve(GuestAddress(address), denied),
            Err(fault(address, denied, FaultReason::Permission))
        );
    }
    assert_eq!(
        memory.resolve(GuestAddress(BASE + 0x2000), Access::Read),
        Err(fault(BASE + 0x2000, Access::Read, FaultReason::Unmapped))
    );
}

#[test]
fn ordered_copied_chunks_load_literal_late_bytes_permissions_and_zero_fill() {
    let source = image();
    let mut engine = EngineInstance::new(5, KEY).unwrap();
    for (index, byte) in engine.arena_mut().unwrap().iter_mut().enumerate() {
        *byte = (index as u8).wrapping_mul(23).wrapping_add(7);
    }
    put32(engine.arena_mut().unwrap(), CANCEL_OFFSET, 1);
    let arena = engine.arena().to_vec();
    engine.begin_image_input(FILE_SIZE as u32).unwrap();
    for (offset, end) in [(0, 1), (1, 4097), (4097, FILE_SIZE)] {
        let mut copied = source[offset..end].to_vec();
        engine.append_image_input(offset as u32, &copied).unwrap();
        copied.fill(0xee);
        assert_eq!(engine.arena(), arena);
    }
    assert_eq!(
        engine.load_pe32_linked_v2_input_at(BASE, GATE),
        Ok(metadata(GATE))
    );
    assert_eq!(engine.arena(), arena);
    check_image(&engine, &source, GATE);
}

#[test]
fn declared_total_limits_include_sixteen_mib_and_abort_is_idempotent() {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    let arena = engine.arena().to_vec();
    for total in [0, INPUT_LIMIT + 1, u32::MAX] {
        assert_eq!(
            engine.begin_image_input(total),
            Err(HostError::InvalidRequest)
        );
        assert_eq!(engine.arena(), arena);
    }
    engine.begin_image_input(INPUT_LIMIT).unwrap();
    engine.append_image_input(0, b"M").unwrap();
    assert_eq!(engine.begin_image_input(1), Err(HostError::InvalidRequest));
    engine.abort_image_input().unwrap();
    engine.abort_image_input().unwrap();
    engine.begin_image_input(1).unwrap();
    engine.append_image_input(0, b"x").unwrap();
    assert_eq!(
        engine.load_pe32_linked_v2_input_at(BASE, GATE),
        Err(HostError::Loader(LoadError::Malformed))
    );
    engine.abort_image_input().unwrap();
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.memory().unwrap().mapped_pages(), 0);
}

#[test]
fn refused_cursor_and_chunk_shapes_keep_prefix_for_later_valid_commit() {
    let source = image();
    let mut engine = EngineInstance::new(5, KEY).unwrap();
    engine.begin_image_input(FILE_SIZE as u32).unwrap();
    engine.append_image_input(0, &source[..7]).unwrap();
    let arena = engine.arena().to_vec();
    for (offset, bytes) in [
        (0, b"x".as_slice()),
        (6, b"x"),
        (8, b"x"),
        (u32::MAX, b"x"),
        (7, b""),
    ] {
        assert_eq!(
            engine.append_image_input(offset, bytes),
            Err(HostError::InvalidRequest)
        );
        assert_eq!(engine.arena(), arena);
    }
    assert_eq!(
        engine.append_image_input(7, &[0; 4097]),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(engine.begin_image_input(0), Err(HostError::InvalidRequest));
    assert_eq!(
        engine.load_pe32_linked_v2_input_at(0, 0),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(engine.memory().unwrap().mapped_pages(), 0);
    assert_eq!(engine.arena(), arena);
    append_from(&mut engine, &source, 7);
    assert_eq!(
        engine.load_pe32_linked_v2_input_at(BASE, GATE),
        Ok(metadata(GATE))
    );
    check_image(&engine, &source, GATE);
}

#[test]
fn overrun_incomplete_and_missing_input_refuse_before_parser_and_never_advance() {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    let arena = engine.arena().to_vec();
    assert_eq!(
        engine.append_image_input(0, b"M"),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(
        engine.load_pe32_linked_v2_input_at(0, 0),
        Err(HostError::InvalidRequest)
    );
    engine.begin_image_input(2).unwrap();
    engine.append_image_input(0, b"M").unwrap();
    assert_eq!(
        engine.append_image_input(1, b"Z?"),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(
        engine.append_image_input(u32::MAX, b"Z"),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(
        engine.load_pe32_linked_v2_input_at(BASE, GATE),
        Err(HostError::InvalidRequest)
    );
    engine.append_image_input(1, b"Z").unwrap();
    assert_eq!(
        engine.append_image_input(2, b"?"),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(
        engine.load_pe32_linked_v2_input_at(BASE, GATE),
        Err(HostError::Loader(LoadError::Malformed))
    );
    assert_eq!(engine.arena(), arena);
    engine.abort_image_input().unwrap();
}

#[test]
fn complete_input_retains_address_failures_and_can_retry_without_reupload() {
    let source = image();
    let mut engine = EngineInstance::new(5, KEY).unwrap();
    stage(&mut engine, &source);
    let arena = engine.arena().to_vec();
    for (base, gate, error) in [
        (0, GATE, LoadError::Malformed),
        (BASE + 1, GATE, LoadError::Malformed),
        (BASE, BASE + 0x2000, LoadError::Malformed),
        (BASE + 0x10000, GATE, LoadError::Unsupported),
    ] {
        assert_eq!(
            engine.load_pe32_linked_v2_input_at(base, gate),
            Err(HostError::Loader(error))
        );
        assert_eq!(engine.memory().unwrap().mapped_pages(), 0);
        assert_eq!(engine.arena(), arena);
        assert_eq!(engine.begin_image_input(1), Err(HostError::InvalidRequest));
    }
    assert_eq!(
        engine.load_pe32_linked_v2_input_at(BASE, GATE),
        Ok(metadata(GATE))
    );
    assert_eq!(engine.arena(), arena);
    check_image(&engine, &source, GATE);
    assert_eq!(
        engine.append_image_input(FILE_SIZE as u32, b"x"),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(
        engine.load_pe32_linked_v2_input_at(BASE, GATE),
        Err(HostError::InvalidRequest)
    );
    engine.abort_image_input().unwrap();
    engine.abort_image_input().unwrap();
    assert_eq!(engine.arena(), arena);
}

#[test]
fn malformed_and_unsupported_full_files_remain_staged_until_abort_and_rebegin() {
    for unsupported in [false, true] {
        let mut source = image();
        let error = if unsupported {
            source[0x1392] = b'x';
            LoadError::Unsupported
        } else {
            source[0] = 0;
            LoadError::Malformed
        };
        let mut engine = EngineInstance::new(5, KEY).unwrap();
        stage(&mut engine, &source);
        let arena = engine.arena().to_vec();
        for _ in 0..2 {
            assert_eq!(
                engine.load_pe32_linked_v2_input_at(BASE, GATE),
                Err(HostError::Loader(error))
            );
            assert_eq!(engine.memory().unwrap().mapped_pages(), 0);
            assert_eq!(engine.arena(), arena);
        }
        assert_eq!(
            engine.begin_image_input(FILE_SIZE as u32),
            Err(HostError::InvalidRequest)
        );
        engine.abort_image_input().unwrap();
        let valid = image();
        stage(&mut engine, &valid);
        assert_eq!(
            engine.load_pe32_linked_v2_input_at(BASE, GATE),
            Ok(metadata(GATE))
        );
        check_image(&engine, &valid, GATE);
    }
}

#[test]
fn loader_capacity_failure_keeps_input_pristine_and_abort_releases_it() {
    let mut engine = EngineInstance::new(4, KEY).unwrap();
    stage(&mut engine, &image());
    let arena = engine.arena().to_vec();
    for _ in 0..2 {
        assert_eq!(
            engine.load_pe32_linked_v2_input_at(BASE, GATE),
            Err(HostError::Loader(LoadError::Capacity))
        );
        assert_eq!(engine.memory().unwrap().mapped_pages(), 0);
        assert_eq!(engine.arena(), arena);
    }
    assert_eq!(engine.begin_image_input(3), Err(HostError::InvalidRequest));
    engine.abort_image_input().unwrap();
    stage(&mut engine, b"bad");
    assert_eq!(
        engine.load_pe32_linked_v2_input_at(BASE, GATE),
        Err(HostError::Loader(LoadError::Malformed))
    );
    assert_eq!(engine.arena(), arena);
}

#[test]
fn old_loader_can_win_while_staging_and_live_abort_does_not_unload_the_image() {
    let source = image();
    let mut engine = EngineInstance::new(5, KEY).unwrap();
    engine.begin_image_input(FILE_SIZE as u32).unwrap();
    engine.append_image_input(0, &source[..11]).unwrap();
    let arena = engine.arena().to_vec();
    assert_eq!(
        engine.load_pe32_linked_v2_at(&source, BASE, GATE),
        Ok(metadata(GATE))
    );
    assert_eq!(
        engine.append_image_input(11, &source[11..12]),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(engine.begin_image_input(1), Err(HostError::InvalidRequest));
    assert_eq!(
        engine.load_pe32_linked_v2_input_at(BASE, GATE),
        Err(HostError::InvalidRequest)
    );
    engine.abort_image_input().unwrap();
    engine.abort_image_input().unwrap();
    assert_eq!(engine.arena(), arena);
    check_image(&engine, &source, GATE);
    for address in [BASE, BASE + 0x1000, BASE + 0x3000, BASE + 0x4000, GATE] {
        engine.unmap(address, 1).unwrap();
    }
    assert_eq!(engine.memory().unwrap().mapped_pages(), 0);
    assert_eq!(engine.begin_image_input(1), Err(HostError::InvalidRequest));
    assert_eq!(engine.arena(), arena);
}

#[test]
fn map_then_unmap_restores_pristine_for_existing_staged_prefix() {
    let source = image();
    let mut engine = EngineInstance::new(5, KEY).unwrap();
    engine.begin_image_input(FILE_SIZE as u32).unwrap();
    engine.append_image_input(0, &source[..4096]).unwrap();
    engine.map(0x9000, 1, 3).unwrap();
    engine.write32(0x9000, 0xdead_beef).unwrap();
    let arena = engine.arena().to_vec();
    assert_eq!(
        engine.append_image_input(4096, &source[4096..]),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(
        engine.load_pe32_linked_v2_input_at(0, 0),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(
        read(engine.memory().unwrap(), 0x9000, 4),
        0xdead_beef_u32.to_le_bytes()
    );
    assert_eq!(engine.arena(), arena);
    engine.unmap(0x9000, 1).unwrap();
    engine.append_image_input(4096, &source[4096..]).unwrap();
    assert_eq!(
        engine.load_pe32_linked_v2_input_at(BASE, GATE),
        Ok(metadata(GATE))
    );
    assert_eq!(engine.arena(), arena);
    check_image(&engine, &source, GATE);
}

#[test]
fn staged_input_does_not_fence_old_compilers_and_abort_cannot_erase_retained_units() {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    stage(&mut engine, b"bad");
    engine.map(0x1000, 1, 7).unwrap();
    engine.write32(0x1000, 0x90).unwrap();
    engine.protect(0x1000, 1, 5).unwrap();
    put32(engine.arena_mut().unwrap(), TRANSFER_OFFSET, 0x1000);
    put32(engine.arena_mut().unwrap(), TRANSFER_OFFSET + 4, 1);
    engine.compile(1).unwrap();
    let resident = engine.compile_resident(1).unwrap();
    let artifact = engine.artifact_bytes().unwrap().to_vec();
    let unit = engine.resident_bytes(resident.get()).unwrap().to_vec();
    let generation = engine.generation();
    let arena = engine.arena().to_vec();
    assert_eq!(
        engine.load_pe32_linked_v2_input_at(0, 0),
        Err(HostError::InvalidRequest)
    );
    engine.abort_image_input().unwrap();
    assert_eq!(engine.artifact_bytes().unwrap(), artifact);
    assert_eq!(engine.resident_bytes(resident.get()).unwrap(), unit);
    assert_eq!(engine.generation(), generation);
    assert_eq!(engine.arena(), arena);
    engine.unmap(0x1000, 1).unwrap();
    assert_eq!(engine.memory().unwrap().mapped_pages(), 0);
    assert_eq!(engine.begin_image_input(1), Err(HostError::InvalidRequest));
    assert_eq!(engine.generation(), generation);
    assert_eq!(engine.arena(), arena);
}

#[test]
fn close_with_active_input_preserves_arena_and_closed_wins_invalid_parameters() {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.begin_image_input(100).unwrap();
    engine.append_image_input(0, b"copied").unwrap();
    let arena = engine.arena().to_vec();
    engine.close();
    engine.close();
    assert!(!engine.is_open());
    assert_eq!(engine.begin_image_input(0), Err(HostError::Closed));
    assert_eq!(
        engine.append_image_input(u32::MAX, &[]),
        Err(HostError::Closed)
    );
    assert_eq!(engine.abort_image_input(), Err(HostError::Closed));
    assert_eq!(
        engine.load_pe32_linked_v2_input_at(0, 0),
        Err(HostError::Closed)
    );
    assert_eq!(engine.arena(), arena);
}
