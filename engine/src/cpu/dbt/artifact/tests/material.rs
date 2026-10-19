use super::*;
use crate::{
    cpu::{
        dbt::region,
        x86::{SegmentContext, decode::DecodeAuthority},
    },
    memory::{PageRange, Permissions},
};

fn memory() -> AddressSpace {
    let mut memory = AddressSpace::new(2).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 2).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory
}

fn cs() -> CodeFetchContext {
    CodeFetchContext::from_segments(&SegmentContext::default()).unwrap()
}

fn spec(pc: u32, length: u32) -> BlockSpec {
    BlockSpec {
        entry: GuestAddress(pc),
        byte_length: length,
    }
}

fn binding() -> Option<wasm::EmbeddedBinding> {
    Some(wasm::EmbeddedBinding::Resident { key: 7, id: 1 })
}

fn emit(memory: &AddressSpace, prepared: PreparedRegion, limits: CompileLimits) -> CompiledRegion {
    emit_entry_prepared_tier(
        memory,
        prepared,
        limits,
        binding(),
        Some(cs()),
        super::super::CompilationTier32::Optimized,
    )
    .unwrap()
}

#[test]
fn final_material_spans_redecode_the_shortened_nonbranch_prefix() {
    let mut memory = memory();
    let mut code = vec![0x90; 16];
    code.extend_from_slice(&[0xeb, 0xfe]);
    memory.write(GuestAddress(0x1000), &code).unwrap();
    let full = compile_runtime_entry(
        &memory,
        GuestAddress(0x1000),
        CompileLimits::default(),
        7,
        1,
        cs(),
        DecodeAuthority::default(),
    )
    .unwrap();
    let limits = CompileLimits {
        wasm_bytes: full.retained_wasm_byte_len() - 1,
        ..CompileLimits::default()
    };
    let shortened = compile_runtime_entry(
        &memory,
        GuestAddress(0x1000),
        limits,
        7,
        1,
        cs(),
        DecodeAuthority::default(),
    )
    .unwrap();
    let specs = shortened.material_block_specs().unwrap().unwrap();
    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].entry, GuestAddress(0x1000));
    assert_eq!(
        specs[0].byte_length as usize,
        shortened.metadata().instructions
    );
    assert!(specs[0].byte_length < 18);
    let rebuild_limits = CompileLimits {
        wasm_bytes: 512 * 1024,
        ..limits
    };
    let prepared = region::prepare_resident_cohort_region(
        &memory,
        &specs,
        rebuild_limits,
        &[],
        Some(cs()),
        DecodeAuthority::default(),
    )
    .unwrap();
    let rebuilt = emit(&memory, prepared, rebuild_limits);
    assert_eq!(
        rebuilt.metadata().instructions,
        shortened.metadata().instructions
    );
    for pc in 0x1000..0x1000 + specs[0].byte_length {
        assert!(rebuilt.contains_instruction(pc));
    }
}

#[test]
fn material_spans_decline_gates_faults_empty_and_noncontiguous_blocks() {
    let mut memory = memory();
    memory.write(GuestAddress(0x1000), &[0x0f, 0x0b]).unwrap();
    let gate = GateSpec {
        entry: GuestAddress(0x1000),
        id: 9,
    };
    let prepared = region::prepare_resident_region(
        &memory,
        &[spec(0x1000, 2)],
        CompileLimits::default(),
        &[gate],
        Some(cs()),
        DecodeAuthority::default(),
    )
    .unwrap();
    let gate = emit(&memory, prepared, CompileLimits::default());
    assert!(gate.material_block_specs().unwrap().is_none());
    memory.write(GuestAddress(0x2ffe), &[0x90, 0xb8]).unwrap();
    let fault = compile_runtime_entry(
        &memory,
        GuestAddress(0x2ffe),
        CompileLimits::default(),
        7,
        1,
        cs(),
        DecodeAuthority::default(),
    )
    .unwrap();
    assert!(fault.material_block_specs().unwrap().is_none());
    memory.write(GuestAddress(0x1000), &[0x90; 3]).unwrap();
    let mut material =
        compile_region(&memory, &[spec(0x1000, 3)], CompileLimits::default()).unwrap();
    material.prepared.blocks[0].instructions.remove(1);
    assert!(material.material_block_specs().unwrap().is_none());
    material.prepared.blocks[0].instructions.clear();
    assert!(material.material_block_specs().unwrap().is_none());
    material.prepared.blocks.clear();
    assert!(material.material_block_specs().unwrap().is_none());
}

#[test]
fn material_spans_decline_overlapping_final_blocks() {
    let mut memory = memory();
    memory.write(GuestAddress(0x1000), &[0x90; 2]).unwrap();
    let mut material =
        compile_region(&memory, &[spec(0x1000, 2)], CompileLimits::default()).unwrap();
    let extra = region::prepare_resident_region(
        &memory,
        &[spec(0x1001, 1)],
        CompileLimits::default(),
        &[],
        Some(cs()),
        DecodeAuthority::default(),
    )
    .unwrap();
    material.prepared.blocks.extend(extra.blocks);
    assert!(material.material_block_specs().unwrap().is_none());
}

#[test]
fn material_spans_decline_a_wrapping_final_instruction_end() {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0xffff_f000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(u32::MAX), &[0x90]).unwrap();
    let material = compile_region(&memory, &[spec(u32::MAX, 1)], CompileLimits::default()).unwrap();
    assert!(material.material_block_specs().unwrap().is_none());
}

#[test]
fn sixteen_block_cohort_preparation_keeps_public_limits_and_emission_authority_separate() {
    let mut memory = memory();
    let specs: Vec<_> = (0..region::MAX_COHORT_BLOCKS)
        .map(|i| spec(0x1000 + i as u32 * 16, 2))
        .collect();
    for s in &specs {
        memory.write(s.entry, &[0x90; 2]).unwrap();
    }
    let limits = CompileLimits {
        blocks: 16,
        instructions: 32,
        wasm_bytes: 512 * 1024,
    };
    assert_eq!(CompileLimits::default().blocks, 8);
    assert_eq!(
        region::validate_limits(limits),
        Err(CompileError::InvalidLimits)
    );
    assert!(matches!(
        prepare_region(&memory, &specs, limits),
        Err(CompileError::InvalidLimits)
    ));
    assert!(matches!(
        region::prepare_resident_region(
            &memory,
            &specs,
            limits,
            &[],
            Some(cs()),
            DecodeAuthority::default()
        ),
        Err(CompileError::InvalidLimits)
    ));
    assert!(matches!(
        region::prepare_resident_cohort_region(
            &memory,
            &specs,
            CompileLimits {
                blocks: 17,
                ..limits
            },
            &[],
            Some(cs()),
            DecodeAuthority::default()
        ),
        Err(CompileError::InvalidLimits)
    ));
    assert!(matches!(
        region::prepare_resident_cohort_region(
            &memory,
            &specs,
            CompileLimits {
                instructions: 31,
                ..limits
            },
            &[],
            Some(cs()),
            DecodeAuthority::default()
        ),
        Err(CompileError::InstructionLimit)
    ));
    assert!(matches!(
        region::prepare_resident_cohort_region(
            &memory,
            &specs,
            CompileLimits {
                instructions: 257,
                ..limits
            },
            &[],
            Some(cs()),
            DecodeAuthority::default()
        ),
        Err(CompileError::InvalidLimits)
    ));
    assert!(matches!(
        region::prepare_resident_cohort_region(
            &memory,
            &specs,
            CompileLimits {
                wasm_bytes: 512 * 1024 + 1,
                ..limits
            },
            &[],
            Some(cs()),
            DecodeAuthority::default()
        ),
        Err(CompileError::InvalidLimits)
    ));
    let normal = prepare_region(&memory, &[spec(0x1000, 2)], CompileLimits::default()).unwrap();
    assert!(matches!(
        emit_entry_prepared_tier(
            &memory,
            normal,
            limits,
            binding(),
            Some(cs()),
            super::super::CompilationTier32::Optimized
        ),
        Err(CompileError::InvalidLimits)
    ));
    let prepared = region::prepare_resident_cohort_region(
        &memory,
        &specs,
        limits,
        &[],
        Some(cs()),
        DecodeAuthority::default(),
    )
    .unwrap();
    let region = emit(&memory, prepared, limits);
    assert_eq!(
        (region.metadata().blocks, region.metadata().instructions),
        (16, 32)
    );
    let retained = region.material_block_specs().unwrap().unwrap();
    assert_eq!(
        retained
            .iter()
            .map(|s| (s.entry, s.byte_length))
            .collect::<Vec<_>>(),
        specs
            .iter()
            .map(|s| (s.entry, s.byte_length))
            .collect::<Vec<_>>()
    );
}

#[test]
fn sixteen_block_shortening_refreshes_every_changed_snapshot() {
    let mut memory = memory();
    let specs: Vec<_> = (0..16).map(|i| spec(0x1000 + i * 16, 2)).collect();
    for s in &specs {
        memory.write(s.entry, &[0x90; 2]).unwrap();
    }
    let limits = CompileLimits {
        blocks: 16,
        instructions: 32,
        wasm_bytes: 512 * 1024,
    };
    let prepare = |limits| {
        region::prepare_resident_cohort_region(
            &memory,
            &specs,
            limits,
            &[],
            Some(cs()),
            DecodeAuthority::default(),
        )
        .unwrap()
    };
    let full = emit(&memory, prepare(limits), limits);
    let tight = CompileLimits {
        wasm_bytes: full.retained_wasm_byte_len() - 1,
        ..limits
    };
    let shortened = emit(&memory, prepare(tight), tight);
    assert_eq!(shortened.metadata().blocks, 16);
    assert!(shortened.metadata().instructions < 32);
    assert!(shortened.retained_wasm_byte_len() <= tight.wasm_bytes);
    let final_specs = shortened.material_block_specs().unwrap().unwrap();
    assert_eq!(final_specs.len(), 16);
    assert_eq!(
        final_specs
            .iter()
            .map(|s| s.byte_length as usize)
            .sum::<usize>(),
        shortened.metadata().instructions
    );
    for s in &specs {
        assert!(shortened.contains_instruction(s.entry.0));
    }
    assert!(shortened.wasm_bytes(&memory).is_ok());
}
