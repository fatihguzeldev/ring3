use super::*;
use crate::cpu::{
    core::{
        emitter::recovery::program::{Color, Error, ValueKey},
        region::values::executable::ExecValueId,
    },
    x86::SegmentRegister,
};
use std::panic::{AssertUnwindSafe, catch_unwind};

const BASE: u32 = 180;

fn add(program: &mut Program, local: u32, ty: ValueType, op: SourceOp) -> RefId {
    program
        .reference(
            ValueKey::Executable(ExecValueId(program.refs().len())),
            Color { local, ty },
            op,
        )
        .unwrap()
}

fn record(words: &[i64], recipes: Recipes, id: RefId) -> [u64; 4] {
    let start = (recipes.base + 4 * id.0) as usize;
    words[start..start + 4]
        .try_into()
        .map(|row: [i64; 4]| row.map(|word| word as u64))
        .unwrap()
}

#[test]
fn empty_recipe_program_preserves_the_existing_word_stream() {
    let mut words = vec![i64::MIN, 17, -1];
    assert_eq!(
        append(&Program::new(0), &mut words, BASE),
        Recipes {
            base: 3,
            count: 0,
            kinds: 0
        }
    );
    assert_eq!(words, [i64::MIN, 17, -1]);
}

#[test]
fn scalar_recipes_encode_exact_operations_arguments_and_one_based_dependencies() {
    let mut program = Program::new(32);
    let available = add(&mut program, BASE, ValueType::I32, SourceOp::Available);
    let physical = add(
        &mut program,
        BASE + 1,
        ValueType::I32,
        SourceOp::PhysicalInput { local: 9 },
    );
    let literal = add(
        &mut program,
        BASE + 2,
        ValueType::I32,
        SourceOp::Constant {
            bits: 0xffff_ffff_8000_0001,
        },
    );
    let mut operators = Vec::new();
    for kind in [
        Binary32::Add,
        Binary32::Sub,
        Binary32::And,
        Binary32::Or,
        Binary32::Xor,
    ] {
        let local = BASE + program.refs().len() as u32;
        operators.push(add(
            &mut program,
            local,
            ValueType::I32,
            SourceOp::Binary32 {
                kind,
                left: physical,
                right: literal,
            },
        ));
    }
    let shifted = add(
        &mut program,
        BASE + 8,
        ValueType::I32,
        SourceOp::ShiftLeft32 {
            value: available,
            amount: 255,
        },
    );
    let mut words = vec![0x1234];
    let recipes = append(&program, &mut words, BASE);
    assert_eq!(
        recipes,
        Recipes {
            base: 1,
            count: 9,
            kinds: 0x1ff
        }
    );
    assert_eq!(record(&words, recipes, available), [0, 0, 0, 0]);
    assert_eq!(record(&words, recipes, physical), [0x1_0000_0001, 9, 0, 0]);
    assert_eq!(
        record(&words, recipes, literal),
        [0x2_0000_0002, 0xffff_ffff_8000_0001, 0, 0]
    );
    for (index, id) in operators.into_iter().enumerate() {
        assert_eq!(
            record(&words, recipes, id),
            [
                ((index as u64 + 3) << 32) | (index as u64 + 3),
                0,
                0x3_0000_0002,
                0
            ]
        );
    }
    assert_eq!(record(&words, recipes, shifted), [0x8_0000_0008, 255, 1, 0]);
    assert_eq!(words[0], 0x1234);
}

#[test]
fn literal_recipes_preserve_full_signed_and_floating_point_bits() {
    let mut program = Program::new(8);
    let cases = [
        (ValueType::I32, 0x1234_5678_8000_0001, 0),
        (ValueType::I64, 0xffff_ffff_ffff_ffff, 1),
        (ValueType::F64, 0x8000_0000_0000_0000, 2),
        (ValueType::F64, 0x7ff8_1234_5678_9abc, 2),
    ];
    for (index, (ty, bits, _)) in cases.iter().enumerate() {
        add(
            &mut program,
            BASE + index as u32,
            *ty,
            SourceOp::Constant { bits: *bits },
        );
    }
    let mut words = Vec::new();
    let recipes = append(&program, &mut words, BASE);
    assert_eq!(recipes.kinds, 1 << 2);
    for (index, (_, bits, ty)) in cases.into_iter().enumerate() {
        assert_eq!(
            record(&words, recipes, RefId(index as u32)),
            [((index as u64) << 32) | (ty << 8) | 2, bits, 0, 0]
        );
    }
}

#[test]
fn every_fp_input_field_keeps_its_type_index_and_optional_top_identity() {
    let mut program = Program::new(32);
    let top = add(&mut program, BASE, ValueType::I32, SourceOp::Available);
    let fields = [
        (FpInput::Native(7), ValueType::F64, 0, 7),
        (FpInput::Low(6), ValueType::I64, 1, 6),
        (FpInput::High(5), ValueType::I32, 2, 5),
        (FpInput::Valid(4), ValueType::I32, 3, 4),
        (FpInput::Tag(3), ValueType::I32, 4, 3),
        (FpInput::Top, ValueType::I32, 5, 0),
        (FpInput::Control, ValueType::I32, 6, 0),
        (FpInput::Status, ValueType::I32, 7, 0),
        (FpInput::Pending, ValueType::I32, 8, 0),
        (FpInput::PendingLeft, ValueType::F64, 9, 0),
        (FpInput::PendingRight, ValueType::F64, 10, 0),
        (FpInput::PendingResult, ValueType::F64, 11, 0),
        (FpInput::Fip, ValueType::I32, 12, 0),
        (FpInput::Fdp, ValueType::I32, 13, 0),
        (FpInput::Opcode, ValueType::I32, 14, 0),
        (FpInput::Cs, ValueType::I32, 15, 0),
        (FpInput::Ds, ValueType::I32, 16, 0),
        (
            FpInput::Selector(SegmentRegister::Fs),
            ValueType::I32,
            17,
            4,
        ),
    ];
    for (index, (field, ty, _, _)) in fields.iter().enumerate() {
        add(
            &mut program,
            BASE + index as u32 + 1,
            *ty,
            SourceOp::Fp(Source::Input {
                field: *field,
                top: Some(top),
            }),
        );
    }
    let no_top = add(
        &mut program,
        BASE + 19,
        ValueType::I32,
        SourceOp::Fp(Source::Input {
            field: FpInput::Selector(SegmentRegister::Fs),
            top: None,
        }),
    );
    let mut words = Vec::new();
    let recipes = append(&program, &mut words, BASE);
    assert_eq!(recipes.kinds, (1 << 0) | (1 << 9));
    for (index, (_, ty, field, argument)) in fields.into_iter().enumerate() {
        let ty = match ty {
            ValueType::I32 => 0,
            ValueType::I64 => 1,
            ValueType::F64 => 2,
        };
        assert_eq!(
            record(&words, recipes, RefId(index as u32 + 1)),
            [
                (((index + 1) as u64) << 32) | (ty << 8) | 9,
                ((argument as u64) << 32) | field,
                1,
                0
            ]
        );
    }
    assert_eq!(
        record(&words, recipes, no_top),
        [0x13_0000_0009, 0x4_0000_0011, 0, 0]
    );
}

#[test]
fn fp_derived_recipes_preserve_typed_dependency_order_for_all_payload_forms() {
    let mut program = Program::new(16);
    let top = add(&mut program, BASE, ValueType::I32, SourceOp::Available);
    let control = add(&mut program, BASE + 1, ValueType::I32, SourceOp::Available);
    let valid = add(&mut program, BASE + 2, ValueType::I32, SourceOp::Available);
    let native = add(&mut program, BASE + 3, ValueType::F64, SourceOp::Available);
    let low = add(&mut program, BASE + 4, ValueType::I64, SourceOp::Available);
    let high = add(&mut program, BASE + 5, ValueType::I32, SourceOp::Available);
    let precision = add(
        &mut program,
        BASE + 6,
        ValueType::I32,
        SourceOp::Fp(Source::PrecisionMode { control }),
    );
    let certificate = add(
        &mut program,
        BASE + 7,
        ValueType::I32,
        SourceOp::Fp(Source::Certificate {
            logical: 7,
            top,
            control,
            valid,
        }),
    );
    let classify_native = add(
        &mut program,
        BASE + 8,
        ValueType::I32,
        SourceOp::Fp(Source::Classify(Payload::Native { value: native })),
    );
    let classify_raw = add(
        &mut program,
        BASE + 9,
        ValueType::I32,
        SourceOp::Fp(Source::Classify(Payload::Opaque { low, high })),
    );
    let classify_tagged = add(
        &mut program,
        BASE + 10,
        ValueType::I32,
        SourceOp::Fp(Source::Classify(Payload::Tagged {
            valid,
            native,
            low,
            high,
        })),
    );
    let mut words = vec![91, 92];
    let recipes = append(&program, &mut words, BASE);
    assert_eq!(recipes.kinds, 1 | (0x1f << 10));
    assert_eq!(record(&words, recipes, precision), [0x6_0000_000a, 0, 2, 0]);
    assert_eq!(
        record(&words, recipes, certificate),
        [0x7_0000_000b, 7, 0x2_0000_0001, 3]
    );
    assert_eq!(
        record(&words, recipes, classify_native),
        [0x8_0000_000c, 0, 4, 0]
    );
    assert_eq!(
        record(&words, recipes, classify_raw),
        [0x9_0000_000d, 0, 0x6_0000_0005, 0]
    );
    assert_eq!(
        record(&words, recipes, classify_tagged),
        [0xa_0000_000e, 0, 0x4_0000_0003, 0x6_0000_0005]
    );
}

#[test]
fn source_identity_survives_interning_and_color_reuse_between_recovery_sites() {
    let mut program = Program::new(8);
    let available = add(&mut program, BASE, ValueType::I32, SourceOp::Available);
    let physical = add(
        &mut program,
        BASE,
        ValueType::I32,
        SourceOp::PhysicalInput { local: 2 },
    );
    let other_available = add(&mut program, BASE, ValueType::I32, SourceOp::Available);
    let constant = add(
        &mut program,
        BASE,
        ValueType::I32,
        SourceOp::Constant { bits: 0x8000_0001 },
    );
    let same_constant = add(
        &mut program,
        BASE,
        ValueType::I32,
        SourceOp::Constant { bits: 0x8000_0001 },
    );
    assert_eq!(constant, same_constant);
    assert_ne!(available, physical);
    assert_ne!(available, other_available);
    for id in [available, physical, other_available, constant] {
        assert_eq!(program.validate_roots(&[id]).unwrap(), [id]);
    }
    assert!(matches!(
        program.validate_roots(&[available, physical]),
        Err(Error::ColorConflict { .. })
    ));
    let mut words = Vec::new();
    let recipes = append(&program, &mut words, BASE);
    assert_eq!(recipes.count, 4);
    assert_eq!(record(&words, recipes, available), [0, 0, 0, 0]);
    assert_eq!(record(&words, recipes, other_available), [0, 0, 0, 0]);
    assert_eq!(record(&words, recipes, physical), [1, 2, 0, 0]);
    assert_eq!(record(&words, recipes, constant), [2, 0x8000_0001, 0, 0]);
}

#[test]
fn invalid_vm_sources_reject_before_appending_any_records() {
    let cases = [
        (
            BASE + 1,
            ValueType::F64,
            SourceOp::Fp(Source::Input {
                field: FpInput::Control,
                top: None,
            }),
        ),
        (
            BASE + 1,
            ValueType::I32,
            SourceOp::Fp(Source::Input {
                field: FpInput::Tag(8),
                top: None,
            }),
        ),
        (
            BASE + 1,
            ValueType::I32,
            SourceOp::PhysicalInput { local: 10 },
        ),
        (
            BASE + 1,
            ValueType::I64,
            SourceOp::PhysicalInput { local: 2 },
        ),
        (BASE - 1, ValueType::I32, SourceOp::Available),
        (
            BASE + 1,
            ValueType::I32,
            SourceOp::Fp(Source::Classify(Payload::Native { value: RefId(0) })),
        ),
        (
            BASE + 1,
            ValueType::I32,
            SourceOp::Fp(Source::Certificate {
                logical: 8,
                top: RefId(0),
                control: RefId(0),
                valid: RefId(0),
            }),
        ),
    ];
    for (local, ty, op) in cases {
        let mut program = Program::new(4);
        add(&mut program, BASE, ValueType::I32, SourceOp::Available);
        add(&mut program, local, ty, op);
        let mut words = vec![i64::MIN, 123, -1];
        assert!(catch_unwind(AssertUnwindSafe(|| append(&program, &mut words, BASE))).is_err());
        assert_eq!(words, [i64::MIN, 123, -1]);
    }
}

#[test]
fn recipe_address_arithmetic_is_checked_at_the_last_u32_word() {
    assert_eq!(checked_end(u32::MAX - 4, 1), u32::MAX);
    assert!(catch_unwind(|| checked_end(u32::MAX - 3, 1)).is_err());
    assert!(catch_unwind(|| checked_end(0, u32::MAX / 4 + 1)).is_err());
    assert_eq!(KIND_MASK, 0xff);
    assert_eq!((TYPE_I32, TYPE_I64, TYPE_F64, TYPE_SHIFT), (0, 1, 2, 8));
}
