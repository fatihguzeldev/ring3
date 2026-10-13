use super::*;
use crate::cpu::x86::simd::PackedKind as P;

fn register(index: u8) -> VectorRegister {
    VectorRegister::new(index).unwrap()
}

#[test]
fn explicit_width_matches_the_delivered_integer_forms() {
    let mut narrow = 0;
    let mut wide = 0;
    for kind in [
        P::Add,
        P::Subtract,
        P::Equal,
        P::GreaterSigned,
        P::AddSignedSaturate,
        P::AddUnsignedSaturate,
        P::SubtractSignedSaturate,
        P::SubtractUnsignedSaturate,
        P::MinimumSigned,
        P::MaximumSigned,
        P::MinimumUnsigned,
        P::MaximumUnsigned,
        P::AverageUnsigned,
        P::Absolute,
        P::Sign,
    ] {
        for lane in [1, 2, 4, 8] {
            let Some(operation) = IntegerLaneOperation::new(kind, lane) else {
                continue;
            };
            let first = (!operation.unary()).then_some(register(1));
            let source = MoveSource::Register(register(2));
            let xmm = IntegerInstruction::with_width(
                VectorWidth::Xmm,
                operation,
                register(0),
                first,
                source,
            )
            .unwrap();
            assert_eq!(
                xmm,
                IntegerInstruction::new(operation, register(0), first, source).unwrap()
            );
            assert_eq!(xmm.width(), VectorWidth::Xmm);
            narrow += 1;
            let ymm = IntegerInstruction::with_width(
                VectorWidth::Ymm,
                operation,
                register(0),
                first,
                source,
            );
            assert!(ymm.is_some());
            wide += usize::from(ymm.is_some());
            assert!(
                IntegerInstruction::with_width(
                    VectorWidth::Ymm,
                    operation,
                    register(0),
                    operation.unary().then_some(register(1)),
                    source
                )
                .is_none()
            );
        }
    }
    assert_eq!((narrow, wide), (44, 44));
}
