use super::*;

fn register(index: u8) -> VectorRegister {
    VectorRegister::new(index).unwrap()
}

fn seed() -> VectorState {
    let mut state = VectorState::default();
    for index in 0..8 {
        state.simd.xmm[index].fill(0xa0 + index as u8);
        state.upper[index].fill(0xb0 + index as u8);
    }
    state
}

fn set_lane(state: &mut VectorState, register: usize, lane: usize, element: Element, bits: u64) {
    let width = element.bytes();
    let offset = lane * width;
    let target = if offset < 16 {
        &mut state.simd.xmm[register]
    } else {
        &mut state.upper[register]
    };
    target[offset % 16..offset % 16 + width].copy_from_slice(&bits.to_le_bytes()[..width]);
}

fn fill(state: &mut VectorState, register: usize, element: Element, bits: u64) {
    for lane in 0..32 / element.bytes() {
        set_lane(state, register, lane, element, bits);
    }
}

fn instruction(kind: FmaKind, order: FmaOrder, shape: FmaShape) -> FmaInstruction {
    FmaInstruction {
        kind,
        order,
        shape,
        destination: register(0),
        second: register(1),
        third: MoveSource::Register(register(2)),
    }
}

fn shapes(element: Element) -> [FmaShape; 3] {
    [
        FmaShape::Scalar { element },
        FmaShape::Packed {
            width: VectorWidth::Xmm,
            element,
        },
        FmaShape::Packed {
            width: VectorWidth::Ymm,
            element,
        },
    ]
}

fn expected_result(before: VectorState, shape: FmaShape, values: &[u64]) -> VectorState {
    let mut expected = before;
    for lane in 0..shape.bytes() / shape.element().bytes() {
        set_lane(
            &mut expected,
            0,
            lane,
            shape.element(),
            values[lane % values.len()],
        );
    }
    if !matches!(
        shape,
        FmaShape::Packed {
            width: VectorWidth::Ymm,
            ..
        }
    ) {
        expected.upper[0].fill(0);
    }
    expected
}

#[path = "numeric/native.rs"]
mod numeric;

#[path = "state/native.rs"]
mod state;

#[path = "exceptions/native.rs"]
mod exceptions;
