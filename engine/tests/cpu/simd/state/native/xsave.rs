use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    cpu::x86::decode::decode_one,
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::EngineInstance,
};

fn memory(bytes: &[u8]) -> AddressSpace {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    memory
}

#[test]
fn xstate_decodes_standard_save_restore_and_control_read() {
    for body in [[0x0f, 0xae, 0x20], [0x0f, 0xae, 0x28]] {
        for prefix in [None, Some(0x67), Some(0x64), Some(0x65)] {
            let bytes: Vec<_> = prefix.into_iter().chain(body).collect();
            let instruction = decode_one(&memory(&bytes), GuestAddress(0x1000))
                .unwrap_or_else(|error| panic!("{bytes:02x?}: {error:?}"));
            assert_eq!(usize::from(instruction.length()), bytes.len());
            assert_eq!(
                instruction.next_pc(),
                GuestAddress(0x1000 + bytes.len() as u32)
            );
        }
    }
    let instruction = decode_one(&memory(&[0x0f, 0x01, 0xd0]), GuestAddress(0x1000)).unwrap();
    assert_eq!(instruction.length(), 3);
}

fn engine(bytes: &[u8]) -> EngineInstance {
    let mut engine = EngineInstance::new(1, 7).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    let arena = engine.arena_mut().unwrap();
    arena[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()].copy_from_slice(bytes);
    engine.upload(0x1000, bytes.len() as u32).unwrap();
    engine
}

fn spec(engine: &mut EngineInstance, length: usize, entry: bool) {
    let arena = engine.arena_mut().unwrap();
    arena[TRANSFER_OFFSET..TRANSFER_OFFSET + 4].copy_from_slice(&0x1000u32.to_le_bytes());
    if !entry {
        arena[TRANSFER_OFFSET + 4..TRANSFER_OFFSET + 8]
            .copy_from_slice(&(length as u32).to_le_bytes());
    }
}

#[test]
fn xstate_bounded_compilation_never_reads_save_restore_operands() {
    for bytes in [
        &[0x0f, 0xae, 0x20, 0xeb, 0xfe][..],
        &[0x0f, 0xae, 0x28, 0xeb, 0xfe][..],
        &[0x0f, 0x01, 0xd0, 0xeb, 0xfe][..],
    ] {
        for resident in [false, true] {
            let mut engine = engine(bytes);
            spec(&mut engine, bytes.len(), false);
            let before = engine.arena().to_vec();
            if resident {
                engine.compile_resident(1).unwrap();
            } else {
                engine.compile(1).unwrap();
            }
            assert_eq!(engine.arena(), before);
        }
    }
}

#[test]
fn xstate_entry_compilation_admits_replacement_and_resident_paths() {
    for bytes in [
        &[0x0f, 0xae, 0x20, 0xeb, 0xfe][..],
        &[0x0f, 0xae, 0x28, 0xeb, 0xfe][..],
        &[0x0f, 0x01, 0xd0, 0xeb, 0xfe][..],
    ] {
        for resident in [false, true] {
            let mut engine = engine(bytes);
            spec(&mut engine, bytes.len(), true);
            let before = engine.arena().to_vec();
            if resident {
                engine.compile_resident_entries(1, 0).unwrap();
            } else {
                engine.compile_entries(1, 0).unwrap();
            }
            assert_eq!(engine.arena(), before);
        }
    }
}

#[test]
fn xstate_keeps_optimized_supervisor_and_invalid_encodings_explicitly_unsupported() {
    for bytes in [
        &[0x0f, 0xae, 0x30][..],
        &[0x0f, 0xc7, 0x20][..],
        &[0x0f, 0xc7, 0x28][..],
        &[0x0f, 0xc7, 0x18][..],
        &[0xf0, 0x0f, 0xae, 0x20][..],
        &[0xf0, 0x0f, 0xae, 0x28][..],
        &[0xf0, 0x0f, 0x01, 0xd0][..],
        &[0x0f, 0xae, 0xe0][..],
    ] {
        assert!(
            decode_one(&memory(bytes), GuestAddress(0x1000)).is_err(),
            "{bytes:02x?}"
        );
    }
}

use ring3_engine::{
    abi::{
        arena::{HELPER_OFFSET, SIMD_OFFSET, X87_OFFSET},
        x86::{SIMD_SIZE, X87_SIZE, encode_simd, encode_x87},
    },
    cpu::x86::{X87State, simd::SimdState, xstate},
    process::{HostError, StoreCompletion},
};

fn states() -> (X87State, SimdState) {
    let x87 = X87State {
        control: 0x027e,
        status: 0x1801,
        tag: 0xaaaa,
        opcode: 0x371,
        instruction_pointer: 0x1234_5678,
        data_pointer: 0x8765_4321,
        code_selector: 0x1b,
        data_selector: 0x23,
        registers: std::array::from_fn(|index| [0x31 + index as u8; 10]),
    };
    let simd = SimdState {
        mxcsr: 0xdfe5,
        xmm: std::array::from_fn(|index| [0x71 + index as u8; 16]),
    };
    (x87, simd)
}

#[test]
fn xstate_save_preserves_unrequested_legacy_reserved_and_software_bytes() {
    let (x87, simd) = states();
    for request in 0..4 {
        let mut image = [0x9b; 576];
        image[512..520].copy_from_slice(&0x8000_0000_0000_0040u64.to_le_bytes());
        let before = image;
        xstate::save(request, &x87, &simd, &mut image);
        for index in 0..576 {
            let selected = request & 1 != 0 && (index < 24 || (32..160).contains(&index))
                || request & 2 != 0 && ((24..32).contains(&index) || (160..288).contains(&index))
                || (512..520).contains(&index);
            if !selected {
                assert_eq!(image[index], before[index], "request{request} byte{index}");
            }
        }
        assert_eq!(
            u64::from_le_bytes(image[512..520].try_into().unwrap()),
            0x8000_0000_0000_0040 | request
        );
        if request & 1 != 0 {
            assert_eq!(&image[32..42], &x87.registers[3]);
            assert_eq!(&image[112..122], &x87.registers[0]);
            assert_eq!(image[4], 0xff);
            assert_eq!(&image[42..48], &[0; 6]);
        }
        if request & 2 != 0 {
            assert_eq!(&image[24..32], &[0xe5, 0xdf, 0, 0, 0xff, 0xff, 0, 0]);
            assert_eq!(&image[160..176], &simd.xmm[0]);
        }
    }
    assert_eq!(xstate::requested(0xffff_ffff, 0xffff_ffff), 3);
    assert_eq!(xstate::read_control(0), Ok(7));
    assert!(xstate::read_control(1).is_err());
    assert!(xstate::read_control(u32::MAX).is_err());
}

#[test]
fn xstate_restore_request_presence_matrix_initializes_only_selected_components() {
    let (x87, simd) = states();
    let source_x87 = X87State::default();
    let source_simd = SimdState {
        mxcsr: 0x5f80,
        xmm: [[0x38; 16]; 8],
    };
    let mut image = [0; 576];
    xstate::save(3, &source_x87, &source_simd, &mut image);
    image[536..576].fill(0xff);
    for request in 0..4 {
        for present in 0u64..4 {
            image[512..520].copy_from_slice(&present.to_le_bytes());
            let (after_x87, after_simd) = xstate::restore(request, &x87, &simd, &image).unwrap();
            assert_eq!(after_x87, if request & 1 == 0 { x87 } else { source_x87 });
            assert_eq!(
                after_simd.mxcsr,
                if request & 2 == 0 {
                    simd.mxcsr
                } else {
                    source_simd.mxcsr
                }
            );
            assert_eq!(
                after_simd.xmm,
                if request & 2 == 0 {
                    simd.xmm
                } else if present & 2 == 0 {
                    [[0; 16]; 8]
                } else {
                    source_simd.xmm
                }
            );
        }
    }
    image[24..28].copy_from_slice(&0x10000u32.to_le_bytes());
    assert!(xstate::restore(2, &x87, &simd, &image).is_err());
    assert_eq!(xstate::restore(0, &x87, &simd, &image), Ok((x87, simd)));
    for offset in [512, 520, 527, 528, 535] {
        let mut bad = image;
        bad[offset] = if offset == 512 { 4 } else { 1 };
        assert!(
            xstate::restore(0, &x87, &simd, &bad).is_err(),
            "offset{offset}"
        );
    }
}

fn bound_engine() -> EngineInstance {
    let mut engine = EngineInstance::new(3, 7).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.map(0x2000, 1, 3).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
        .copy_from_slice(&[0xeb, 0xfe]);
    engine.upload(0x1000, 2).unwrap();
    spec(&mut engine, 2, false);
    engine.compile(1).unwrap();
    let (x87, simd) = states();
    let arena = engine.arena_mut().unwrap();
    encode_x87(&x87, &mut arena[X87_OFFSET..X87_OFFSET + X87_SIZE]).unwrap();
    encode_simd(&simd, &mut arena[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE]).unwrap();
    engine
}

fn seed(engine: &mut EngineInstance, address: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(address, bytes.len() as u32).unwrap();
}

fn assert_packet(engine: &EngineInstance, status: u32, detail: u32, address: u32, access: u32) {
    let p = &engine.arena()[HELPER_OFFSET..HELPER_OFFSET + 40];
    assert_eq!(&p[..4], b"R3MH");
    for (offset, value) in [
        (16, status),
        (20, 0),
        (24, detail),
        (28, address),
        (32, access),
        (36, 576),
    ] {
        assert_eq!(
            u32::from_le_bytes(p[offset..offset + 4].try_into().unwrap()),
            value,
            "packet{offset}"
        );
    }
}

#[test]
fn xstate_helpers_preserve_transfer_and_stage_restore_before_publication() {
    let mut engine = bound_engine();
    let mut image = [0xa5; 576];
    image[512..536].fill(0);
    image[24..28].copy_from_slice(&0x5f80u32.to_le_bytes());
    seed(&mut engine, 0x2000, &image);
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.cpu_xsave(0x2000, 0, u32::MAX),
        Ok(StoreCompletion::Complete)
    );
    assert_packet(&engine, 0, 0, 0, 0);
    assert_eq!(
        &engine.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + 4096],
        &before[TRANSFER_OFFSET..TRANSFER_OFFSET + 4096]
    );
    let mut saved = [0; 576];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x2000), &mut saved)
        .unwrap();
    assert_eq!(saved, image);
    engine.cpu_xrstor(0x2000, 2, 0).unwrap();
    assert_packet(&engine, 0, 0, 0, 0);
    assert_eq!(
        engine.arena()[SIMD_OFFSET + 32..SIMD_OFFSET + 160],
        [0; 128]
    );
    assert_eq!(
        &engine.arena()[SIMD_OFFSET + 16..SIMD_OFFSET + 20],
        &0x5f80u32.to_le_bytes()
    );
    assert_eq!(
        &engine.arena()[X87_OFFSET..X87_OFFSET + X87_SIZE],
        &before[X87_OFFSET..X87_OFFSET + X87_SIZE]
    );
    image[512] = 4;
    seed(&mut engine, 0x2000, &image);
    let before = engine.arena().to_vec();
    engine.cpu_xrstor(0x2000, 3, 0).unwrap();
    assert_packet(&engine, 3, 0, 0, 0);
    assert_eq!(
        &engine.arena()[X87_OFFSET..SIMD_OFFSET + SIMD_SIZE],
        &before[X87_OFFSET..SIMD_OFFSET + SIMD_SIZE]
    );
    assert_eq!(
        &engine.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + 4096],
        &before[TRANSFER_OFFSET..TRANSFER_OFFSET + 4096]
    );
}

#[test]
fn xstate_alignment_header_read_and_late_write_faults_are_atomic() {
    let mut engine = bound_engine();
    for restore in [false, true] {
        if restore {
            engine.cpu_xrstor(0x6001, 3, 0).unwrap();
        } else {
            engine.cpu_xsave(0x6001, 3, 0).unwrap();
        }
        assert_packet(&engine, 3, 0, 0, 0);
    }
    let mut before = [0; 512];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x2e00), &mut before)
        .unwrap();
    engine.cpu_xsave(0x2e00, 3, 0).unwrap();
    assert_packet(&engine, 1, 1, 0x3000, 1);
    let mut after = [0; 512];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x2e00), &mut after)
        .unwrap();
    assert_eq!(after, before);
    engine.map(0x3000, 1, 3).unwrap();
    engine.protect(0x3000, 1, 1).unwrap();
    engine.cpu_xsave(0x2e00, 3, 0).unwrap();
    assert_packet(&engine, 1, 2, 0x3000, 2);
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x2e00), &mut after)
        .unwrap();
    assert_eq!(after, before);
    engine.protect(0x3000, 1, 2).unwrap();
    engine.cpu_xsave(0x2e00, 0, 0).unwrap();
    assert_packet(&engine, 1, 2, 0x3000, 1);
    engine.cpu_xsave(0xffff_fe00, 0, 0).unwrap();
    // the selected header starts at (FFFFFE00 + 512) mod2^32 = 0.
    assert_packet(&engine, 1, 1, 0, 1);
}

#[test]
fn xstate_save_invalidates_modified_code_and_rejects_invalid_state_before_mutation() {
    let mut engine = bound_engine();
    assert_eq!(
        engine.cpu_xsave(0x1000, 1, 0),
        Ok(StoreCompletion::CodeInvalidated)
    );
    assert_packet(&engine, 0, 0, 0, 0);
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.cpu_xsave(0x2000, 3, 0),
        Err(HostError::CodeInvalidated)
    );
    assert_eq!(engine.arena(), before);
    let mut engine = bound_engine();
    engine.arena_mut().unwrap()[SIMD_OFFSET + 20] = 1;
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.cpu_xsave(0x6001, 3, 0),
        Err(HostError::Infrastructure)
    );
    assert_eq!(
        engine.cpu_xrstor(0x6001, 3, 0),
        Err(HostError::Infrastructure)
    );
    assert_eq!(engine.arena(), before);
}
