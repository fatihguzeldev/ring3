mod descriptor;
mod execute;
mod operation;
mod state;
pub mod ymm;

pub use super::xstate::fxsave::{restore_extended, save_extended};
pub use descriptor::decode_descriptor;
pub use execute::execute;
pub(crate) use execute::execute_validated;
pub use operation::{
    ConvertKind, CryptoKind, FloatKind, PackedKind, ShuffleKind, SimdError, SimdKind, SimdOperand,
    SimdOperation, SimdRegister, SimdResult,
};
pub use state::SimdState;

mod crypto;
mod float;
mod packed;

pub(crate) use float::{ElementwiseKind, elementwise};

mod integer;
pub(crate) use integer::IntegerLaneOperation;

pub(crate) use float::{CompareMode, compare_lanes, conversion_lanes};

pub(crate) use float::{PairwiseKind, dot_product_lanes, pairwise_lanes, round_lanes};

mod rearrange;
pub(crate) use rearrange::RearrangeOperation;

mod shift;
pub(crate) use shift::ShiftOperation;

mod multiply;
pub(crate) use multiply::MultiplyOperation;

mod scalar;
pub(crate) use scalar::{ScalarOperation, ScalarWidth};

mod predicates;
pub(crate) use predicates::BitTestKind;
pub(crate) use predicates::flags as test_flags;

pub(crate) use crypto::execute as crypto_value;

pub(crate) use float::{FusedKind, fused_lanes};
