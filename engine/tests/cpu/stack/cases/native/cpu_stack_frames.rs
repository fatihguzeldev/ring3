use ring3_engine::{
    cpu::x86::{StackFrameKind, decode::decode_one, ir::Operation},
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::EngineInstance,
};

fn decode(bytes: &[u8]) -> Operation {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    *decode_one(&memory, GuestAddress(0x1000))
        .unwrap()
        .operation()
}

#[test]
fn push_all_and_general_enter_decode_width_allocation_and_modulo32_nesting() {
    for (bytes, kind) in [
        (vec![0x60], StackFrameKind::PushAll32),
        (vec![0x66, 0x60], StackFrameKind::PushAll16),
    ] {
        assert_eq!(
            decode(&bytes),
            Operation::StackFrame {
                kind,
                allocation: 0,
                nesting: 0
            }
        );
    }
    for (prefix, kind) in [
        (vec![], StackFrameKind::Enter32),
        (vec![0x66], StackFrameKind::Enter16),
    ] {
        for raw_nesting in [0, 1, 2, 31, 32, 255] {
            let mut bytes = prefix.clone();
            bytes.extend_from_slice(&[0xc8, 0x34, 0x12, raw_nesting]);
            assert_eq!(
                decode(&bytes),
                Operation::StackFrame {
                    kind,
                    allocation: 0x1234,
                    nesting: raw_nesting & 31
                }
            );
        }
    }
    assert_eq!(decode(&[0x61]), Operation::PopAll);
    assert_eq!(decode(&[0x66, 0x61]), Operation::PopAllWord);
    assert_eq!(decode(&[0xc8, 0, 0, 0]), Operation::EnterZero);
    assert_eq!(decode(&[0x66, 0xc8, 0, 0, 0]), Operation::EnterZeroWord);
}

#[test]
fn stack_control_rejects_reserved_bits_and_non_enter_immediates() {
    for kind in [
        StackFrameKind::PushAll32,
        StackFrameKind::PushAll16,
        StackFrameKind::PopAll32,
        StackFrameKind::PopAll16,
        StackFrameKind::Enter32,
        StackFrameKind::Enter16,
    ] {
        let allocation = if matches!(kind, StackFrameKind::Enter32 | StackFrameKind::Enter16) {
            0xabcd
        } else {
            0
        };
        let nesting = if allocation != 0 { 31 } else { 0 };
        assert_eq!(
            StackFrameKind::decode(kind.control(allocation, nesting)),
            Some((kind, allocation, nesting))
        );
    }
    for invalid in [6, 7, 8, 0x80000000, 0x20000000, 0x100, 0x1000000] {
        assert!(StackFrameKind::decode(invalid).is_none());
    }
}

#[test]
fn every_stack_family_compiles_bound_without_scalar_read_store_crossings() {
    for bytes in [
        vec![0x60],
        vec![0x66, 0x60],
        vec![0x61],
        vec![0x66, 0x61],
        vec![0xc8, 16, 0, 3],
        vec![0x66, 0xc8, 16, 0, 31],
    ] {
        for resident in [false, true] {
            let mut engine = EngineInstance::new(1, 0x5354).unwrap();
            engine.map(0x1000, 1, 7).unwrap();
            let code = [bytes.as_slice(), &[0xeb, 0xfe]].concat();
            engine.arena_mut().unwrap()[140..140 + code.len()].copy_from_slice(&code);
            engine.upload(0x1000, code.len() as u32).unwrap();
            engine.protect(0x1000, 1, 4).unwrap();
            engine.arena_mut().unwrap()[140..148].copy_from_slice(&[
                0x00,
                0x10,
                0,
                0,
                code.len() as u8,
                0,
                0,
                0,
            ]);
            if resident {
                engine.compile_resident(1).unwrap();
            } else {
                engine.compile(1).unwrap();
            }
        }
    }
}

#[test]
fn lock_stack_forms_refuse_without_publishing_an_instruction() {
    for bytes in [
        vec![0xf0, 0x60],
        vec![0xf0, 0x61],
        vec![0xf0, 0xc8, 16, 0, 1],
        vec![0x66, 0xf0, 0xc8, 16, 0, 1],
    ] {
        let mut memory = AddressSpace::new(1).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        memory.write(GuestAddress(0x1000), &bytes).unwrap();
        assert!(decode_one(&memory, GuestAddress(0x1000)).is_err());
    }
}
