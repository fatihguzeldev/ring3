use super::stopped_support::{KEY, expect_admitted, expect_captured_fetch, stopped};
use ring3_engine::{
    abi::{
        arena::TRANSFER_OFFSET,
        x86::{
            SEGMENTS_SIZE, STATE_SIZE, decode_segment_context, decode_state,
            encode_segment_context, encode_state_v2,
        },
    },
    cpu::{
        dbt::{CompileError, InstructionError},
        x86::{SegmentCache, SegmentContext, SegmentRegister, State32, decode::DecodeError},
    },
    process::HostError,
};

fn forms() -> [&'static [u8]; 6] {
    [
        &[0x6c],
        &[0x66, 0x6d],
        &[0x6d],
        &[0x6e],
        &[0x66, 0x6f],
        &[0x6f],
    ]
}

fn assert_refused(bytes: &[u8], strict: bool) {
    let mut engine = stopped(bytes, 0x3000);
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0x3000_u32.to_le_bytes(), (bytes.len() as u32).to_le_bytes()].concat());
    let before = engine.arena().to_vec();
    let artifact = engine.artifact_bytes().unwrap().to_vec();
    let result = if strict {
        engine.compile(1).map(|_| 0)
    } else {
        engine.compile_stopped_entry(KEY, 1)
    };
    assert!(
        matches!(
            result,
            Err(HostError::Compile(CompileError::Instruction {
                cause: InstructionError::Decode(DecodeError::Unsupported(_)),
                ..
            }))
        ),
        "{bytes:02x?}: {result:?}"
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.artifact_bytes().unwrap(), artifact);
    assert_eq!(engine.generation(), 1);
}

#[test]
fn complete_port_strings_prepare_without_reading_count_or_successor_bytes() {
    for form in forms() {
        for address in [&[][..], &[0x67]] {
            for repeat in [&[][..], &[0xf3]] {
                let bytes = [address, repeat, form].concat();
                expect_admitted(&bytes, 0x3000);
                expect_admitted(&bytes, 0x4000 - bytes.len() as u32);
            }
        }
    }
    for prefix in [0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65] {
        for opcode in [0x6c, 0x6e] {
            expect_admitted(&[prefix, 0xf3, opcode], 0x3000);
        }
    }
    for opcode in [0x6c, 0x6e] {
        expect_admitted(&[0x66, 0xf3, opcode], 0x3000);
    }
}

#[test]
fn strict_port_strings_and_reserved_repetition_keep_atomic_refusals() {
    for form in forms() {
        for address in [&[][..], &[0x67]] {
            for repeat in [&[][..], &[0xf3]] {
                assert_refused(&[address, repeat, form].concat(), true);
            }
        }
        for prefix in [&[0xf2][..], &[0xf3, 0xf3], &[0xf2, 0xf3], &[0xf3, 0xf2]] {
            assert_refused(&[prefix, form].concat(), false);
        }
        let opcode = &form[usize::from(form[0] == 0x66)..];
        assert_refused(&[&[0x66, 0x66][..], opcode].concat(), false);
        assert_refused(&[&[0x67, 0x67][..], form].concat(), false);
    }
    assert_refused(&[0x66, 0x66, 0xcd, 0x80], false);
}

#[test]
fn invalid_lock_and_existing_hlt_control_do_not_depend_on_count() {
    for form in forms() {
        for repeat in [&[][..], &[0xf3]] {
            expect_admitted(&[&[0xf0][..], repeat, form].concat(), 0x3000);
        }
    }
    expect_admitted(&[0xf4], 0x3000);
}

#[test]
fn zero_count_cannot_replace_missing_instruction_fetch_with_completion() {
    for bytes in [&[0xf3][..], &[0x66], &[0x67], &[0xf3, 0x66], &[0x67, 0xf3]] {
        let entry = 0x4000 - bytes.len() as u32;
        let mut engine = stopped(bytes, entry);
        engine.arena_mut().unwrap()[20..24].copy_from_slice(&0_u32.to_le_bytes());
        expect_captured_fetch(&mut engine, entry, 0x4000, false, bytes.len() as u32 + 1);
    }
}

#[test]
fn no_access_inputs_use_valid_flags_and_loaded_segment_contexts() {
    for flags in [2, 0x102, 0x402, 0x244cd7] {
        let state = State32 {
            eflags: flags,
            ..State32::default()
        };
        let mut record = [0; STATE_SIZE];
        encode_state_v2(&state, &mut record).unwrap();
        assert_eq!(decode_state(&record), Ok(state));
    }
    let mut contexts = Vec::new();
    for register in [
        SegmentRegister::Es,
        SegmentRegister::Ds,
        SegmentRegister::Fs,
        SegmentRegister::Gs,
    ] {
        let mut context = SegmentContext::default();
        context.state.selectors[register.index()] = 0;
        context.caches[register.index()] = SegmentCache::UNUSABLE;
        contexts.push(context);
    }
    for (register, bits) in [
        (SegmentRegister::Es, 0xf2100000),
        (SegmentRegister::Ds, 0xf2100000),
        (SegmentRegister::Ss, 0xf2300000),
    ] {
        let mut context = SegmentContext::default();
        context.caches[register.index()] = SegmentCache::from_bits(bits).unwrap();
        contexts.push(context);
    }
    for register in [SegmentRegister::Fs, SegmentRegister::Gs] {
        let mut context = SegmentContext::default();
        if register == SegmentRegister::Gs {
            context.state.selectors[register.index()] = 0x43;
            context.caches[register.index()] = SegmentCache::from_bits(0xf33fffff).unwrap();
        }
        context.state.bases[register.index()] = 0xfffffff0;
        contexts.push(context);
    }
    for context in contexts {
        assert!(context.is_valid());
        let mut record = [0; SEGMENTS_SIZE];
        encode_segment_context(&context, &mut record).unwrap();
        assert_eq!(decode_segment_context(&record), Ok(context));
    }
}
