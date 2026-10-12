use ring3_engine::{
    abi::{
        arena::{ARENA_SIZE, SEGMENTS_OFFSET, SIMD_OFFSET, TRANSFER_OFFSET, X87_OFFSET},
        x86::{
            SEGMENTS_SIZE, SIMD_SIZE, X87_SIZE, decode_exit, decode_segments, decode_simd,
            decode_state, decode_x87,
        },
    },
    cpu::{
        ExecutionExit, ExitReason,
        dbt::{CompileLimits, GateSpec},
        x86::{
            Register32, SegmentState, State32, X87State,
            decode::decode_one,
            ir::{BranchTarget, Location32, Operation, Value32},
            simd::SimdState,
        },
    },
    loader::{ImageMetadata32, LinkedImageMetadata32V2},
    memory::{Access, GuestAddress},
    process::EngineInstance,
};

const PE: &[u8] = include_bytes!("../fixtures/p2-c-imports/program.exe");
const TEXT: [u8; 51] = [
    0x57, 0x56, 0x8b, 0x35, 0x38, 0x20, 0x40, 0, 0x68, 0x67, 0x45, 0x23, 0xf1, 0xff, 0xd6, 0x8b,
    0x3d, 0x34, 0x20, 0x40, 0, 0xff, 0xd7, 0xa3, 0, 0x30, 0x40, 0, 0x6a, 0, 0xff, 0xd6, 0xff, 0xd7,
    0xa3, 4, 0x30, 0x40, 0, 0xc7, 5, 8, 0x30, 0x40, 0, 0x47, 0x4e, 0x49, 0x52, 0xeb, 0xfe,
];
const KEY: u64 = 0x4349_4d50_4f52_5401;
const PREFERRED: u32 = 0x0040_0000;
const GATE: u32 = 0x6000_0000;
const STACK: u32 = 0x8000;
const GATES: [GateSpec; 2] = [
    GateSpec {
        entry: GuestAddress(GATE),
        id: 0x0001_0001,
    },
    GateSpec {
        entry: GuestAddress(GATE + 16),
        id: 0x0001_0002,
    },
];
const BLOCKS: [(u32, u32, usize); 5] = [
    (0x1000, 15, 5),
    (0x100f, 8, 2),
    (0x1017, 9, 3),
    (0x1020, 2, 1),
    (0x1022, 17, 3),
];
const LENGTHS: [u8; 14] = [1, 1, 6, 5, 2, 6, 2, 5, 2, 2, 2, 5, 10, 2];

fn read(engine: &EngineInstance, address: u32, length: usize) -> Vec<u8> {
    let mut bytes = vec![0; length];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(address), &mut bytes)
        .unwrap();
    bytes
}

fn permissions(engine: &EngineInstance, address: u32, expected: [bool; 3]) {
    for (access, permitted) in [Access::Read, Access::Write, Access::Execute]
        .into_iter()
        .zip(expected)
    {
        assert_eq!(
            engine
                .memory()
                .unwrap()
                .resolve(GuestAddress(address), access)
                .is_ok(),
            permitted
        );
    }
}

#[test]
fn original_c_import_pe_stages_links_rebases_starts_and_admits_six_resident_owners() {
    assert_eq!(PE.len(), 2560);
    assert_eq!(&PE[0x400..0x433], TEXT);
    assert_eq!(
        &PE[0x800..0x814],
        &[
            0, 0x10, 0, 0, 0x14, 0, 0, 0, 4, 0x30, 0x11, 0x30, 0x18, 0x30, 0x23, 0x30, 0x29, 0x30,
            0, 0,
        ]
    );
    assert_eq!(
        &PE[0x600..0x628],
        &[
            0x28, 0x20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x60, 0x20, 0, 0, 0x34, 0x20, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ]
    );
    let thunks = [0x40, 0x20, 0, 0, 0x50, 0x20, 0, 0, 0, 0, 0, 0];
    assert_eq!(&PE[0x628..0x634], thunks);
    assert_eq!(&PE[0x634..0x640], thunks);
    assert_eq!(&PE[0x640..0x64f], b"\0\0GetLastError\0");
    assert_eq!(&PE[0x650..0x65f], b"\0\0SetLastError\0");
    assert_eq!(&PE[0x660..0x66d], b"KERNEL32.dll\0");
    let limits = CompileLimits::default();
    assert_eq!(
        (limits.blocks, limits.instructions, limits.wasm_bytes),
        (8, 64, 65536)
    );
    assert_eq!(BLOCKS.iter().map(|block| block.2).sum::<usize>(), 14);

    for base in [PREFERRED, 0x0050_0000] {
        let mut engine = EngineInstance::new(7, KEY).unwrap();
        let initial = engine.arena().to_vec();
        assert_eq!(initial.len(), ARENA_SIZE);
        assert_eq!(ARENA_SIZE, 4612);
        engine.begin_image_input(PE.len() as u32).unwrap();
        for (index, chunk) in PE.chunks(1024).enumerate() {
            let mut owned = chunk.to_vec();
            engine
                .append_image_input((index * 1024) as u32, &owned)
                .unwrap();
            owned.fill(0xa5);
            assert_eq!(engine.arena(), initial);
        }
        let linked = engine.load_pe32_linked_v2_input_at(base, GATE).unwrap();
        assert_eq!(
            linked,
            LinkedImageMetadata32V2 {
                image: ImageMetadata32 {
                    image_base: base,
                    image_size: 0x5000,
                    entry_point: base + 0x1000,
                    mapped_pages: 6,
                },
                gate_base: GATE,
                gate_count: 2,
                gates: [
                    GATES[0],
                    GATES[1],
                    GateSpec {
                        entry: GuestAddress(0),
                        id: 0
                    }
                ],
            }
        );
        assert_eq!(engine.arena(), initial);
        assert_eq!(engine.memory().unwrap().mapped_pages(), 6);

        let mut expected_text = vec![0; 4096];
        expected_text[..512].copy_from_slice(&PE[0x400..0x600]);
        for (offset, target) in [
            (4, 0x2038),
            (17, 0x2034),
            (24, 0x3000),
            (35, 0x3004),
            (41, 0x3008),
        ] {
            assert_eq!(
                u32::from_le_bytes(expected_text[offset..offset + 4].try_into().unwrap()),
                PREFERRED + target
            );
            expected_text[offset..offset + 4].copy_from_slice(&(base + target).to_le_bytes());
        }
        assert_eq!(read(&engine, base + 0x1000, 4096), expected_text);
        for (rva, file_offset, length) in [(0, 0, 1024), (0x2000, 0x600, 512), (0x4000, 0x800, 512)]
        {
            let mut expected = vec![0; 4096];
            expected[..length].copy_from_slice(&PE[file_offset..file_offset + length]);
            if rva == 0x2000 {
                expected[0x34..0x38].copy_from_slice(&GATE.to_le_bytes());
                expected[0x38..0x3c].copy_from_slice(&(GATE + 16).to_le_bytes());
            }
            assert_eq!(read(&engine, base + rva, 4096), expected);
        }
        let mut expected_gates = vec![0; 4096];
        expected_gates[..2].copy_from_slice(&[0x0f, 0x0b]);
        expected_gates[16..18].copy_from_slice(&[0x0f, 0x0b]);
        assert_eq!(read(&engine, GATE, 4096), expected_gates);
        assert_eq!(read(&engine, base + 0x3000, 4096), vec![0; 4096]);
        for (address, expected) in [
            (base, [true, false, false]),
            (base + 0x1000, [true, false, true]),
            (base + 0x2000, [true, false, false]),
            (base + 0x3000, [true, true, false]),
            (base + 0x4000, [true, false, false]),
            (GATE, [true, false, true]),
        ] {
            permissions(&engine, address, expected);
        }

        engine.start_loaded_image(STACK, 1).unwrap();
        let startup = State32 {
            registers: [0, 0, 0, 0, STACK + 4096, 0, 0, 0],
            eip: base + 0x1000,
            eflags: 2,
        };
        assert_eq!(decode_state(&engine.arena()[..56]).unwrap(), startup);
        assert_eq!(
            decode_exit(&engine.arena()[56..96]).unwrap(),
            ExecutionExit {
                retired: 0,
                reason: ExitReason::NeedCode
            }
        );
        assert_eq!(&engine.arena()[96..], &initial[96..]);
        assert_eq!(
            decode_x87(&engine.arena()[X87_OFFSET..X87_OFFSET + X87_SIZE]).unwrap(),
            X87State::default()
        );
        assert_eq!(
            decode_simd(&engine.arena()[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE]).unwrap(),
            SimdState::default()
        );
        assert_eq!(
            decode_segments(&engine.arena()[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE])
                .unwrap(),
            SegmentState::default()
        );
        assert_eq!(engine.memory().unwrap().mapped_pages(), 7);
        assert_eq!(read(&engine, STACK, 4096), vec![0; 4096]);
        permissions(&engine, STACK, [true, true, false]);

        let operation = |rva| {
            *decode_one(engine.memory().unwrap(), GuestAddress(base + rva))
                .unwrap()
                .operation()
        };
        for (rva, source) in [
            (0x1000, Value32::Register(Register32::Edi)),
            (0x1001, Value32::Register(Register32::Esi)),
            (0x1008, Value32::Immediate(0xf123_4567)),
            (0x101c, Value32::Immediate(0)),
        ] {
            assert_eq!(operation(rva), Operation::Push { source });
        }
        for (rva, register) in [
            (0x100d, Register32::Esi),
            (0x1015, Register32::Edi),
            (0x101e, Register32::Esi),
            (0x1020, Register32::Edi),
        ] {
            assert_eq!(
                operation(rva),
                Operation::Call {
                    target: BranchTarget::Indirect(Location32::Register(register))
                }
            );
        }
        assert_eq!(
            operation(0x1031),
            Operation::Jump {
                target: BranchTarget::Direct(GuestAddress(base + 0x1031))
            }
        );
        let before_decode = engine.arena().to_vec();
        let mut pc = base + 0x1000;
        for length in LENGTHS {
            let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
            assert_eq!(decoded.length(), length);
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .is_code_current(decoded.code_snapshot())
            );
            pc = decoded.next_pc().0;
        }
        assert_eq!(pc, base + 0x1033);
        assert_eq!(engine.arena(), before_decode);

        let groups: Vec<Vec<u32>> =
            std::iter::once(GATES.iter().map(|gate| gate.entry.0).collect())
                .chain(BLOCKS.iter().map(|&(rva, _, _)| vec![base + rva]))
                .collect();
        assert_eq!(groups.len(), 6);
        let mut retained = Vec::new();
        for (slot, entries) in groups.iter().enumerate() {
            assert!(entries.len() <= limits.blocks);
            for (index, pc) in entries.iter().enumerate() {
                let at = TRANSFER_OFFSET + index * 4;
                engine.arena_mut().unwrap()[at..at + 4].copy_from_slice(&pc.to_le_bytes());
            }
            if slot == 0 {
                for (index, gate) in GATES.iter().enumerate() {
                    let at = TRANSFER_OFFSET + entries.len() * 4 + index * 8;
                    engine.arena_mut().unwrap()[at..at + 4]
                        .copy_from_slice(&gate.entry.0.to_le_bytes());
                    engine.arena_mut().unwrap()[at + 4..at + 8]
                        .copy_from_slice(&gate.id.to_le_bytes());
                }
            } else {
                let (rva, length, count) = BLOCKS[slot - 1];
                assert!(count <= limits.instructions);
                let mut pc = base + rva;
                for _ in 0..count {
                    pc = decode_one(engine.memory().unwrap(), GuestAddress(pc))
                        .unwrap()
                        .next_pc()
                        .0;
                }
                assert_eq!(pc, base + rva + length);
            }
            let before_compile = engine.arena().to_vec();
            let id = engine
                .compile_resident_entries(entries.len() as u32, if slot == 0 { 2 } else { 0 })
                .unwrap()
                .get();
            let wasm = engine.resident_bytes(id).unwrap().to_vec();
            assert!(wasm.starts_with(b"\0asm\x01\0\0\0"));
            assert!(wasm.len() <= limits.wasm_bytes);
            engine
                .acknowledge_resident_installation(KEY, id, slot as u32)
                .unwrap();
            for pc in entries {
                let installed = engine.lookup_installed_resident(KEY, *pc).unwrap();
                assert_eq!((installed.unit_id, installed.slot), (id, slot as u32));
            }
            if slot != 0 {
                let (rva, _, count) = BLOCKS[slot - 1];
                let mut pc = base + rva;
                for _ in 0..count {
                    assert_eq!(
                        engine.lookup_installed_resident(KEY, pc).unwrap().unit_id,
                        id
                    );
                    pc = decode_one(engine.memory().unwrap(), GuestAddress(pc))
                        .unwrap()
                        .next_pc()
                        .0;
                }
            }
            assert_eq!(engine.arena(), before_compile);
            retained.push((id, wasm));
        }
        for (id, wasm) in retained {
            engine.guard_resident(KEY, id).unwrap();
            assert_eq!(engine.resident_bytes(id).unwrap(), wasm);
        }
        assert_eq!(decode_state(&engine.arena()[..56]).unwrap(), startup);
        assert_eq!(&engine.arena()[X87_OFFSET..], &initial[X87_OFFSET..]);
        assert_eq!(read(&engine, STACK, 4096), vec![0; 4096]);
        assert_eq!(read(&engine, base + 0x3000, 4096), vec![0; 4096]);
    }
}
