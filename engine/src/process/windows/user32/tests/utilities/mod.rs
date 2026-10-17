mod geometry;
mod text;
mod windows;
use super::*;
fn call(
    context: &mut User32Context32,
    memory: &mut AddressSpace,
    c: User32CallContext32,
    api: User32Api32,
    args: &[u32],
) -> u32 {
    match context.prepare_arguments(api, args, memory, c).unwrap() {
        User32Outcome32::Return(v) => v,
        other => panic!("expected utility return, got {other:?}"),
    }
}
