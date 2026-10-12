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
        dbt::CompileLimits,
        x86::{
            Register32, SegmentState, State32, X87State,
            decode::decode_one,
            ir::{BranchTarget, Operation, Value32},
            simd::SimdState,
        },
    },
    loader::ImageMetadata32,
    memory::{Access, GuestAddress},
    process::EngineInstance,
};

const PE: &[u8] = include_bytes!("../fixtures/p2-sse2-pe-startup/program.exe");
const KERNEL: &[u8] =
    include_bytes!("../../../../programs/numeric/fixtures/p2-fp-compiled/sse2/kernel.text.bin");
const KEY: u64 = 0x5353_4532_5045_0001;
const PREFERRED: u32 = 0x0040_0000;
const STACK: u32 = 0x8000;
const GROUPS: [&[(u32, u32, usize)]; 3] = [
    &[(0x1000, 27, 6)],
    &[
        (0x1030, 24, 10),
        (0x1048, 22, 6),
        (0x105e, 32, 9),
        (0x107e, 38, 13),
    ],
    &[(0x101b, 20, 4)],
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
fn original_linked_sse2_pe_stages_relocates_starts_and_admits_three_resident_units() {
    assert_eq!(PE.len(), 2560);
    assert_eq!(KERNEL.len(), 116);
    assert_eq!(&PE[0x430..0x4a4], KERNEL);
    assert_eq!(
        &PE[0x800..0x814],
        &[
            0, 0x10, 0, 0, 0x14, 0, 0, 0, 1, 0x30, 8, 0x30, 0x0d, 0x30, 0x12, 0x30, 0x1f, 0x30,
            0x25, 0x30
        ]
    );
    let limits = CompileLimits::default();
    assert_eq!(
        (limits.blocks, limits.instructions, limits.wasm_bytes),
        (8, 64, 65536)
    );
    assert_eq!(GROUPS.iter().map(|group| group.len()).sum::<usize>(), 6);
    assert_eq!(
        GROUPS
            .iter()
            .flat_map(|group| group.iter())
            .map(|block| block.2)
            .sum::<usize>(),
        48
    );

    for base in [PREFERRED, 0x0050_0000] {
        let mut engine = EngineInstance::new(6, KEY).unwrap();
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
            engine.load_pe32_input_at(base).unwrap(),
            ImageMetadata32 {
                image_base: base,
                image_size: 0x5000,
                entry_point: base + 0x1000,
                mapped_pages: 5,
            }
        );
        assert_eq!(engine.arena(), initial);
        assert_eq!(engine.memory().unwrap().mapped_pages(), 5);

        let mut expected_text = vec![0; 4096];
        expected_text[..512].copy_from_slice(&PE[0x400..0x600]);
        for (offset, target) in [
            (1, 0x3008),
            (8, 0x2030),
            (13, 0x2000),
            (18, 0x3010),
            (31, 0x3040),
            (37, 0x3044),
        ] {
            assert_eq!(
                u32::from_le_bytes(expected_text[offset..offset + 4].try_into().unwrap()),
                PREFERRED + target
            );
            expected_text[offset..offset + 4].copy_from_slice(&(base + target).to_le_bytes());
        }
        assert_eq!(read(&engine, base + 0x1000, 4096), expected_text);
        assert_eq!(read(&engine, base + 0x1030, 116), KERNEL);
        for (rva, file_offset, length) in [(0, 0, 1024), (0x2000, 0x600, 512), (0x4000, 0x800, 512)]
        {
            let mut expected = vec![0; 4096];
            expected[..length].copy_from_slice(&PE[file_offset..file_offset + length]);
            assert_eq!(read(&engine, base + rva, 4096), expected);
        }
        assert_eq!(read(&engine, base + 0x3000, 4096), vec![0; 4096]);
        for (rva, expected) in [
            (0, [true, false, false]),
            (0x1000, [true, false, true]),
            (0x2000, [true, false, false]),
            (0x3000, [true, true, false]),
            (0x4000, [true, false, false]),
        ] {
            permissions(&engine, base + rva, expected);
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
        assert_eq!(engine.memory().unwrap().mapped_pages(), 6);
        assert_eq!(read(&engine, STACK, 4096), vec![0; 4096]);
        permissions(&engine, STACK, [true, true, false]);

        let operation = |rva| {
            *decode_one(engine.memory().unwrap(), GuestAddress(base + rva))
                .unwrap()
                .operation()
        };
        assert_eq!(
            operation(0x1016),
            Operation::Call {
                target: BranchTarget::Direct(GuestAddress(base + 0x1030))
            }
        );
        assert_eq!(operation(0x10a3), Operation::Return { stack_adjust: 0 });
        assert_eq!(
            operation(0x102d),
            Operation::Jump {
                target: BranchTarget::Direct(GuestAddress(base + 0x102d))
            }
        );
        for (index, register) in [
            Register32::Ebp,
            Register32::Ebx,
            Register32::Edi,
            Register32::Esi,
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(
                operation(0x1030 + index as u32),
                Operation::Push {
                    source: Value32::Register(register)
                }
            );
        }

        let mut retained = Vec::new();
        for (slot, group) in GROUPS.into_iter().enumerate() {
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
            for (index, &(rva, _, _)) in group.iter().enumerate() {
                let at = TRANSFER_OFFSET + index * 4;
                engine.arena_mut().unwrap()[at..at + 4]
                    .copy_from_slice(&(base + rva).to_le_bytes());
            }
            let before_compile = engine.arena().to_vec();
            let id = engine
                .compile_resident_entries(group.len() as u32, 0)
                .unwrap()
                .get();
            assert_eq!(engine.arena(), before_compile);
            let wasm = engine.resident_bytes(id).unwrap().to_vec();
            assert!(wasm.starts_with(b"\0asm\x01\0\0\0"));
            assert!(wasm.len() <= limits.wasm_bytes);
            engine
                .acknowledge_resident_installation(KEY, id, slot as u32)
                .unwrap();
            for pc in pcs {
                assert_eq!(
                    engine.lookup_installed_resident(KEY, pc).unwrap().unit_id,
                    id
                );
            }
            retained.push((id, wasm));
        }
        for (id, wasm) in retained {
            engine.guard_resident(KEY, id).unwrap();
            assert_eq!(engine.resident_bytes(id).unwrap(), wasm);
        }
        assert_eq!(decode_state(&engine.arena()[..56]).unwrap(), startup);
        assert_eq!(read(&engine, STACK, 4096), vec![0; 4096]);
        assert_eq!(read(&engine, base + 0x3000, 4096), vec![0; 4096]);
    }
}
