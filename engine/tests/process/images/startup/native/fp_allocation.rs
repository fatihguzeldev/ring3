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
    loader::{ImageMetadata32, LinkedImageMetadata32V5},
    memory::{Access, GuestAddress},
    process::EngineInstance,
};

const PE: &[u8] = include_bytes!("../fixtures/p2-c-imports/fp-alloc/program.exe");
const KERNEL: &[u8] =
    include_bytes!("../../../../programs/numeric/fixtures/p2-fp-compiled/sse2/kernel.text.bin");
const ENTRY: [u8; 135] = [
    0x56, 0x83, 0xec, 0x10, 0x0f, 0x28, 0x05, 0x70, 0x20, 0x40, 0x00, 0x0f, 0x11, 0x04, 0x24, 0xff,
    0x15, 0xb4, 0x20, 0x40, 0x00, 0x89, 0xc6, 0x83, 0xc0, 0x30, 0x89, 0x35, 0x04, 0x30, 0x40, 0x00,
    0x50, 0x6a, 0x03, 0x68, 0x30, 0x20, 0x40, 0x00, 0x68, 0x00, 0x20, 0x40, 0x00, 0x56, 0xe8, 0x5d,
    0x00, 0x00, 0x00, 0x83, 0xc4, 0x14, 0xa3, 0x08, 0x30, 0x40, 0x00, 0x0f, 0x28, 0x06, 0x0f, 0x29,
    0x05, 0x20, 0x30, 0x40, 0x00, 0x0f, 0x28, 0x46, 0x10, 0x0f, 0x29, 0x05, 0x30, 0x30, 0x40, 0x00,
    0x0f, 0x28, 0x46, 0x20, 0x0f, 0x29, 0x05, 0x40, 0x30, 0x40, 0x00, 0xf2, 0x0f, 0x10, 0x46, 0x30,
    0xf2, 0x0f, 0x11, 0x05, 0x50, 0x30, 0x40, 0x00, 0x68, 0x00, 0x80, 0x00, 0x00, 0x6a, 0x00, 0x56,
    0xff, 0x15, 0xb8, 0x20, 0x40, 0x00, 0xa3, 0x0c, 0x30, 0x40, 0x00, 0xc7, 0x05, 0x10, 0x30, 0x40,
    0x00, 0x47, 0x4e, 0x49, 0x52, 0xeb, 0xfe,
];
const KEY: u64 = 0x4650_414c_5045_0001;
const PREFERRED: u32 = 0x0040_0000;
const GATE: u32 = 0x6000_0000;
const STACK: u32 = 0x8000;
const GATES: [GateSpec; 2] = [
    GateSpec {
        entry: GuestAddress(GATE + 64),
        id: 0x0001_0005,
    },
    GateSpec {
        entry: GuestAddress(GATE + 80),
        id: 0x0001_0006,
    },
];
const GROUPS: [&[(u32, u32, usize)]; 5] = [
    &[(0x1000, 21, 5)],
    &[(0x1015, 30, 9)],
    &[
        (0x1090, 24, 10),
        (0x10a8, 22, 6),
        (0x10be, 32, 9),
        (0x10de, 38, 13),
    ],
    &[(0x1033, 67, 14)],
    &[(0x1076, 17, 3)],
];
const FIXUPS: [(usize, u32); 13] = [
    (0x07, 0x2070),
    (0x11, 0x20b4),
    (0x1c, 0x3004),
    (0x24, 0x2030),
    (0x29, 0x2000),
    (0x37, 0x3008),
    (0x41, 0x3020),
    (0x4c, 0x3030),
    (0x57, 0x3040),
    (0x64, 0x3050),
    (0x72, 0x20b8),
    (0x77, 0x300c),
    (0x7d, 0x3010),
];
const CALLER_LENGTHS: [u8; 31] = [
    1, 3, 7, 4, 6, 2, 3, 6, 1, 2, 5, 5, 1, 5, 3, 5, 3, 7, 4, 7, 4, 7, 5, 8, 5, 2, 1, 6, 5, 10, 2,
];

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
fn original_fp_allocation_pe_stages_links_rebases_starts_and_admits_six_resident_owners() {
    assert_eq!(PE.len(), 2560);
    assert_eq!(KERNEL.len(), 116);
    assert_eq!(&PE[0x400..0x487], ENTRY);
    assert_eq!(&PE[0x490..0x504], KERNEL);
    assert_eq!(&PE[0x487..0x490], &[0xcc; 9]);
    assert_eq!(
        &PE[0x800..0x824],
        &[
            0x00, 0x10, 0x00, 0x00, 0x24, 0x00, 0x00, 0x00, 0x07, 0x30, 0x11, 0x30, 0x1c, 0x30,
            0x24, 0x30, 0x29, 0x30, 0x37, 0x30, 0x41, 0x30, 0x4c, 0x30, 0x57, 0x30, 0x64, 0x30,
            0x72, 0x30, 0x77, 0x30, 0x7d, 0x30, 0x00, 0x00,
        ]
    );
    assert_eq!(
        &PE[0x680..0x6a8],
        &[
            0xa8, 0x20, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xde, 0x20,
            0x00, 0x00, 0xb4, 0x20, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ]
    );
    let thunks = [0xc0, 0x20, 0, 0, 0xd0, 0x20, 0, 0, 0, 0, 0, 0];
    assert_eq!(&PE[0x6a8..0x6b4], thunks);
    assert_eq!(&PE[0x6b4..0x6c0], thunks);
    assert_eq!(&PE[0x6c0..0x6cf], b"\0\0VirtualAlloc\0");
    assert_eq!(&PE[0x6d0..0x6de], b"\0\0VirtualFree\0");
    assert_eq!(&PE[0x6de..0x6eb], b"KERNEL32.dll\0");
    assert_eq!(
        &PE[0x670..0x680],
        &[0, 0, 0, 0, 0, 0x10, 0, 0, 0, 0x30, 0, 0, 4, 0, 0, 0,]
    );
    let limits = CompileLimits::default();
    assert_eq!(
        (limits.blocks, limits.instructions, limits.wasm_bytes),
        (8, 64, 65536)
    );
    assert_eq!(GROUPS.iter().map(|group| group.len()).sum::<usize>(), 8);
    assert_eq!(
        GROUPS
            .iter()
            .flat_map(|group| group.iter())
            .map(|block| block.2)
            .sum::<usize>(),
        69
    );

    for base in [PREFERRED, 0x0050_0000] {
        let mut engine = EngineInstance::new(8, KEY).unwrap();
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
        assert_eq!(
            engine.load_pe32_linked_v5_input_at(base, GATE).unwrap(),
            LinkedImageMetadata32V5 {
                image: ImageMetadata32 {
                    image_base: base,
                    image_size: 0x5000,
                    entry_point: base + 0x1000,
                    mapped_pages: 6
                },
                gate_base: GATE,
                gate_count: 2,
                gates: [
                    GATES[0],
                    GATES[1],
                    GateSpec {
                        entry: GuestAddress(0),
                        id: 0
                    },
                    GateSpec {
                        entry: GuestAddress(0),
                        id: 0
                    },
                    GateSpec {
                        entry: GuestAddress(0),
                        id: 0
                    },
                    GateSpec {
                        entry: GuestAddress(0),
                        id: 0
                    },
                ],
            }
        );
        assert_eq!(engine.arena(), initial);
        assert_eq!(engine.memory().unwrap().mapped_pages(), 6);
        let mut expected_text = vec![0; 4096];
        expected_text[..512].copy_from_slice(&PE[0x400..0x600]);
        for (offset, target) in FIXUPS {
            assert_eq!(
                u32::from_le_bytes(expected_text[offset..offset + 4].try_into().unwrap()),
                PREFERRED + target
            );
            expected_text[offset..offset + 4].copy_from_slice(&(base + target).to_le_bytes());
        }
        assert_eq!(read(&engine, base + 0x1000, 4096), expected_text);
        assert_eq!(read(&engine, base + 0x1090, 116), KERNEL);
        for (rva, file_offset, length) in [(0, 0, 1024), (0x2000, 0x600, 512), (0x4000, 0x800, 512)]
        {
            let mut expected = vec![0; 4096];
            expected[..length].copy_from_slice(&PE[file_offset..file_offset + length]);
            if rva == 0x2000 {
                expected[0xb4..0xb8].copy_from_slice(&(GATE + 64).to_le_bytes());
                expected[0xb8..0xbc].copy_from_slice(&(GATE + 80).to_le_bytes());
            }
            assert_eq!(read(&engine, base + rva, 4096), expected);
        }
        let mut expected_gates = vec![0; 4096];
        expected_gates[64..66].copy_from_slice(&[0x0f, 0x0b]);
        expected_gates[80..82].copy_from_slice(&[0x0f, 0x0b]);
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
        permissions(&engine, base + 0x2000 + 47, [true, false, false]);
        permissions(&engine, base + 0x2030 + 63, [true, false, false]);

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
        assert_eq!(
            operation(0x102e),
            Operation::Call {
                target: BranchTarget::Direct(GuestAddress(base + 0x1090)),
            }
        );
        for (rva, target) in [(0x100f, 0x20b4), (0x1070, 0x20b8)] {
            let Operation::Call {
                target: BranchTarget::Indirect(Location32::Memory(address)),
            } = operation(rva)
            else {
                panic!("original imported call");
            };
            assert_eq!(
                (
                    address.base,
                    address.index,
                    address.scale,
                    address.displacement
                ),
                (None, None, 1, base + target)
            );
        }
        for (rva, source) in [
            (0x1000, Value32::Register(Register32::Esi)),
            (0x1020, Value32::Register(Register32::Eax)),
            (0x1021, Value32::Immediate(3)),
            (0x102d, Value32::Register(Register32::Esi)),
            (0x1068, Value32::Immediate(0x8000)),
            (0x106d, Value32::Immediate(0)),
            (0x106f, Value32::Register(Register32::Esi)),
        ] {
            assert_eq!(operation(rva), Operation::Push { source });
        }
        for (rva, bytes, aligned) in [
            (0x1004, 16, true),
            (0x100b, 16, false),
            (0x105b, 8, false),
            (0x1060, 8, false),
        ] {
            let Operation::Simd(operation) = operation(rva) else {
                panic!("original allocation argument or scalar move");
            };
            assert_eq!(
                (operation.memory_bytes, operation.aligned),
                (bytes, aligned)
            );
        }
        assert_eq!(operation(0x1103), Operation::Return { stack_adjust: 0 });
        assert_eq!(
            operation(0x1085),
            Operation::Jump {
                target: BranchTarget::Direct(GuestAddress(base + 0x1085)),
            }
        );
        let mut pc = base + 0x1000;
        for length in CALLER_LENGTHS {
            let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
            assert_eq!(decoded.length(), length);
            pc = decoded.next_pc().0;
        }
        assert_eq!(pc, base + 0x1087);

        assert!(
            engine
                .memory()
                .unwrap()
                .resolve(GuestAddress(0x1000_0000), Access::Read)
                .is_err()
        );
        let mut retained = Vec::new();
        let mut all_pcs = Vec::new();
        for slot in 0..6 {
            let group = if slot == 0 { &[][..] } else { GROUPS[slot - 1] };
            let before_decode = engine.arena().to_vec();
            let mut pcs = Vec::new();
            for &(rva, length, count) in group {
                let mut pc = base + rva;
                for _ in 0..count {
                    let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
                    assert!(
                        engine
                            .memory()
                            .unwrap()
                            .is_code_current(decoded.code_snapshot())
                    );
                    pcs.push(pc);
                    pc = decoded.next_pc().0;
                }
                assert_eq!(pc, base + rva + length);
            }
            assert_eq!(engine.arena(), before_decode);
            assert!(group.len() <= limits.blocks);
            assert!(group.iter().map(|block| block.2).sum::<usize>() <= limits.instructions);
            let entries = if slot == 0 {
                GATES.iter().map(|gate| gate.entry.0).collect::<Vec<_>>()
            } else {
                group.iter().map(|block| base + block.0).collect()
            };
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
            for pc in entries.iter().chain(pcs.iter()) {
                let installed = engine.lookup_installed_resident(KEY, *pc).unwrap();
                assert_eq!((installed.unit_id, installed.slot), (id, slot as u32));
            }
            assert_eq!(engine.arena(), before_compile);
            all_pcs.extend(pcs);
            retained.push((id, wasm));
        }
        assert_eq!(all_pcs.len(), 69);
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
