use super::{CTX, link};
use wasm_encoder::Function;

pub(super) const REASON: u32 = 108;

// The typed sink has already closed the common region exit wrapper. Append the
// sole publication epilogue directly; generated Wasm is never decoded/re-encoded.
pub(super) fn finish(mut function: Function, numeric: bool) -> Vec<u8> {
    let mut code = function.instructions();
    code.local_get(CTX);
    for (local, _) in link::boundary_fields(numeric) {
        code.local_get(local);
    }
    code.call(super::support::owner_internal(5, numeric));
    code.local_get(REASON).return_().end();
    function.into_raw_body()
}
