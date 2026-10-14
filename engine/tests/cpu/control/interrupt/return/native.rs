use super::far_support as support;
use ring3_engine::cpu::x86::decode::DecodeError;

#[test]
fn same_privilege_interrupt_return_decodes_in_both_operand_widths() {
    for bytes in [&[0xcf][..], &[0x66, 0xcf]] {
        assert!(support::decode(bytes).is_ok(), "{bytes:02x?}");
    }
}

#[test]
fn same_privilege_interrupt_return_compiles_without_stack_or_descriptor_access() {
    for bytes in [&[0xcf][..], &[0x66, 0xcf]] {
        support::bound(bytes);
    }
}

#[test]
fn nested_task_flags_compile_without_stack_or_hardware_task_configuration() {
    use ring3_engine::{
        abi::x86::{STATE_SIZE, decode_state, encode_state_v3},
        cpu::x86::State32,
        process::EngineInstance,
    };

    for bytes in [&[0xcf][..], &[0x66, 0xcf]] {
        for (resident, entries) in [(false, false), (false, true), (true, false), (true, true)] {
            let mut engine = EngineInstance::new(1, 0x4e54_4952).unwrap();
            engine.map(0x1000, 1, 7).unwrap();
            engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
            engine.upload(0x1000, bytes.len() as u32).unwrap();
            let state = State32 {
                registers: [0, 0, 0, 0, 0x9000, 0, 0, 0],
                eip: 0x1000,
                eflags: 0x14102,
            };
            encode_state_v3(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
            engine.arena_mut().unwrap()[140..144].copy_from_slice(&0x1000_u32.to_le_bytes());
            if !entries {
                engine.arena_mut().unwrap()[144..148]
                    .copy_from_slice(&(bytes.len() as u32).to_le_bytes());
            }
            match (resident, entries) {
                (false, false) => engine.compile(1).map(|_| ()),
                (false, true) => engine.compile_entries(1, 0).map(|_| ()),
                (true, false) => engine.compile_resident(1).map(|_| ()),
                (true, true) => engine.compile_resident_entries(1, 0).map(|_| ()),
            }
            .unwrap();
            assert_eq!(decode_state(&engine.arena()[..STATE_SIZE]), Ok(state));
            assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
        }
    }
}

#[test]
fn interrupt_return_keeps_lock_and_reserved_repeat_encodings_closed() {
    for bytes in [&[0xf0, 0xcf][..], &[0xf0, 0x66, 0xcf]] {
        assert_eq!(support::decode(bytes), Err(DecodeError::InvalidEncoding));
    }
    for bytes in [&[0xf2, 0xcf][..], &[0xf3, 0xcf], &[0x66, 0x66, 0xcf]] {
        assert!(matches!(
            support::decode(bytes),
            Err(DecodeError::Unsupported(_))
        ));
    }
}

#[test]
fn interrupt_return_uses_a_dedicated_terminal_operation_for_each_width() {
    use ring3_engine::cpu::x86::{FarWidth, ir::Operation};
    for (bytes, width) in [
        (&[0xcf][..], FarWidth::Dword),
        (&[0x66, 0xcf], FarWidth::Word),
    ] {
        assert_eq!(
            support::decode(bytes),
            Ok(Operation::InterruptReturn { width })
        );
    }
}

#[test]
fn resume_state_v3_is_additive_and_retains_old_codec_refusals() {
    use ring3_engine::{
        abi::x86::{STATE_SIZE, decode_state, encode_state, encode_state_v2, encode_state_v3},
        cpu::x86::State32,
    };
    let state = State32 {
        eflags: 0x10002,
        ..State32::default()
    };
    for encoder in [encode_state, encode_state_v2] {
        let mut bytes = [0xa5; STATE_SIZE];
        assert!(encoder(&state, &mut bytes).is_err());
        assert_eq!(bytes, [0xa5; STATE_SIZE]);
    }
    let mut bytes = [0; STATE_SIZE];
    encode_state_v3(&state, &mut bytes).unwrap();
    assert_eq!(decode_state(&bytes), Ok(state));
    assert_eq!(&bytes[4..8], &[3, 0, 1, 0]);
}
