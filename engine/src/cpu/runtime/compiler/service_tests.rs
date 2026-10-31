use super::*;
use crate::{
    cpu::{
        runtime::numeric::NumericState,
        x86::{Ext80, SegmentContext, State32, X87Operation, X87State, decode::decode_one},
    },
    memory::{PageRange, Permissions},
};

fn memory() -> AddressSpace {
    let mut memory = AddressSpace::new(2).unwrap();
    for address in [0x1000, 0x3000] {
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(address), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
    }
    memory
}

#[test]
fn service_capture_keeps_inline_numeric_interiors_and_exact_strict_operands() {
    let mut memory = memory();
    memory
        .write(
            GuestAddress(0x1000),
            &[0x90, 0xd9, 0xe8, 0x0f, 0x28, 0xc0, 0x90],
        )
        .unwrap();
    let code = compile(&memory, GuestAddress(0x1000), 4).unwrap();
    assert_eq!(code.entries, [0x1000, 0x1001, 0x1003, 0x1006]);
    assert_eq!(code.services.len(), 2);
    for (site, pc) in code.services.iter().zip([0x1001, 0x1003]) {
        assert_ne!(pc, 0x1000);
        assert!(code.entries.contains(&pc));
        let decoded = decode_one(&memory, GuestAddress(pc)).unwrap();
        assert_eq!(site.pc, pc);
        assert_eq!(site.next_pc, decoded.next_pc().0);
        assert_eq!(site.operation, *decoded.operation());
    }
    assert!(code.is_current(&memory));
    memory.write(GuestAddress(0x1002), &[0xee]).unwrap();
    assert!(!code.is_current(&memory));
}

#[test]
fn service_capture_omits_native_gates_and_keeps_strict_noninline_operations() {
    let mut memory = memory();
    memory.write(GuestAddress(0x1000), &[0x0f, 0x0b]).unwrap();
    let gate = compile_with_gates(
        &memory,
        GuestAddress(0x1000),
        1,
        &[(GuestAddress(0x1000), 17)],
    )
    .unwrap();
    assert!(gate.services.is_empty());
    for bytes in [&[0x0f, 0xa2][..], &[0xdb, 0xe3], &[0xd9, 0xfe]] {
        memory.write(GuestAddress(0x1000), bytes).unwrap();
        let code = compile(&memory, GuestAddress(0x1000), 1).unwrap();
        assert_eq!(code.services.len(), 1);
        let decoded = decode_one(&memory, GuestAddress(0x1000)).unwrap();
        assert_eq!(code.services[0].operation, *decoded.operation());
        assert_eq!(code.services[0].next_pc, decoded.next_pc().0);
    }
}

#[test]
fn service_capture_retains_instruction_boundary_wrap_without_wrapping_one_instruction() {
    let mut memory = AddressSpace::new(2).unwrap();
    for address in [0, 0xfffff000] {
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(address), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
    }
    memory.write(GuestAddress(u32::MAX), &[0x90]).unwrap();
    memory.write(GuestAddress(0), &[0xd9, 0xe8]).unwrap();
    let code = compile(&memory, GuestAddress(u32::MAX), 2).unwrap();
    assert_eq!(code.services.len(), 1);
    assert_eq!((code.services[0].pc, code.services[0].next_pc), (0, 2));
    assert!(code.is_current(&memory));
    memory.write(GuestAddress(u32::MAX), &[0xd9]).unwrap();
    let public = decode_one(&memory, GuestAddress(u32::MAX)).unwrap_err();
    let CompileError::Decode(internal) = compile(&memory, GuestAddress(u32::MAX), 1).unwrap_err()
    else {
        panic!("strict fetch error expected");
    };
    assert_eq!(internal, public);
}

#[test]
fn retained_operation_uses_each_callers_precision_and_segment_selectors() {
    let mut memory = memory();
    memory.write(GuestAddress(0x1000), &[0xd8, 0xc1]).unwrap();
    let site = compile(&memory, GuestAddress(0x1000), 1).unwrap().services[0];
    let one = Ext80::ONE;
    let tiny = Ext80::from_f64_bits((2_f64.powi(-30)).to_bits());
    for (control, selector, result, precision) in [
        (0x003f, 0x1b, one, 0x20),
        (
            0x023f,
            0x2b,
            Ext80::from_f64_bits((1.0 + 2_f64.powi(-30)).to_bits()),
            0,
        ),
    ] {
        let mut numeric = NumericState::default();
        numeric.x87 = X87State {
            control,
            tag: 0xfff0,
            ..X87State::default()
        };
        numeric.x87.registers[0] = one.0;
        numeric.x87.registers[1] = tiny.0;
        let mut scalar = State32 {
            eip: site.pc,
            eflags: 0x202,
            ..State32::default()
        };
        let mut segments = SegmentContext::default();
        segments.state.selectors[1] = selector;
        numeric
            .execute(
                &mut memory,
                &mut scalar,
                &segments,
                &site.operation,
                site.next_pc,
            )
            .unwrap();
        assert_eq!(numeric.x87.registers[0], result.0);
        assert_eq!(numeric.x87.control, control);
        assert_eq!(numeric.x87.status & 0x20, precision);
        assert_eq!(numeric.x87.code_selector, selector);
        assert_eq!(scalar.eip, site.next_pc);
        assert!(matches!(
            site.operation,
            Operation::X87 {
                operation: X87Operation::Binary { .. },
                ..
            }
        ));
    }
}
