use super::{CTX, REMAIN, link};
use wasm_encoder::{BlockType, Function, ValType};

pub(super) const REASON: u32 = 108;
pub(super) const PRECISE: i32 = -1;

// The typed sink has already closed the common region exit wrapper. Append the
// sole publication epilogue directly; generated Wasm is never decoded/re-encoded.
pub(super) fn finish(
    mut function: Function,
    plan: &crate::cpu::core::region::values::RegionPlan,
    numeric: bool,
    precise: bool,
) -> Vec<u8> {
    let mut code = function.instructions();
    super::numeric::emit_normalizer(plan, &mut code, numeric);
    code.local_get(CTX);
    for (local, _) in link::boundary_fields(numeric) {
        code.local_get(local);
    }
    code.call(super::support::owner_internal(5, numeric));
    if precise {
        code.local_get(REASON);
    } else {
        code.local_get(REASON)
            .i32_const(PRECISE)
            .i32_eq()
            .if_(BlockType::Result(ValType::I32))
            .local_get(CTX)
            .local_get(REMAIN)
            .call(super::support::owner_internal(6, numeric))
            .else_()
            .local_get(REASON)
            .end();
    }
    code.return_().end();
    function.into_raw_body()
}
