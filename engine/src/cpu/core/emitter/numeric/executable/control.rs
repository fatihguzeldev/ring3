//! Typed CW effects keep the actual payload view; only environment authority changes.
use super::*;

/// Resolve the exact deferred sticky-status contribution before a waiting test.
/// The result remains local until the current effect has actually succeeded.
pub(super) fn resolve_status(out: &mut Output<'_>) {
    out.c().local_get(100).if_(BlockType::Empty);
    for local in [100, STATUS, 101, 102, 103] {
        out.c().local_get(local);
    }
    let resolver = out.internal(8);
    out.c()
        .call(resolver)
        .local_set(STATUS)
        .i32_const(0)
        .local_set(100)
        .end();
}

pub(super) fn waiting_check(out: &mut Output<'_>, waiting: bool) {
    if waiting {
        resolve_status(out);
        out.c()
            .local_get(STATUS)
            .local_get(87)
            .i32_const(-1)
            .i32_xor()
            .i32_and()
            .i32_const(63)
            .i32_and()
            .br_if(0);
    }
}

pub(super) fn emit(out: &mut Output<'_>, effect: &FpEffect) -> bool {
    match &effect.kind {
        FpEffectKind::LoadControl { address, result } => {
            // Old unmasked pending state wins over every address/memory fault.
            if !effect.waiting {
                resolve_status(out);
            }
            waiting_check(out, effect.waiting);
            memory::prepare_value(out, address, 2, false, false, POINTER);
            out.c().local_get(POINTER).i32_load16_u(mem(0, 0));
            out.define(*result);
            take(out, *result, 87);
            // FLDCW refreshes ES/B against the new masks without changing C1,
            // condition codes, TOP, sticky exception flags or any metadata.
            out.c()
                .local_get(STATUS)
                .i32_const(!0x8080)
                .i32_and()
                .local_get(STATUS)
                .local_get(87)
                .i32_const(-1)
                .i32_xor()
                .i32_and()
                .i32_const(63)
                .i32_and()
                .i32_eqz()
                .i32_eqz()
                .i32_const(0x8080)
                .i32_mul()
                .i32_or()
                .local_set(STATUS)
                .i32_const(0)
                .local_set(100)
                .local_get(pool::ADMITTED)
                .i32_const(!(pool::ENTERED | 256))
                .i32_and()
                .local_set(pool::ADMITTED)
                .i64_const(0)
                .local_set(super::super::super::FP_PAYLOAD_TOKENS);
            true
        }
        FpEffectKind::StoreControl { address } => {
            waiting_check(out, effect.waiting);
            memory::prepare_value(out, address, 2, true, false, POINTER);
            out.c()
                .local_get(POINTER)
                .local_get(87)
                .i32_store16(mem(0, 0));
            memory::commit(out);
            true
        }
        _ => false,
    }
}
