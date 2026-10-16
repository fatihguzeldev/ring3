use super::*;

#[test]
fn nested_disposition_sets_the_i386_boundary_flag_until_that_handler_returns() {
    let mut engine = engine();
    let (original, frames) = chain(&mut engine);
    let first = engine
        .begin_exception_delivery(
            ExceptionRecord32::new(0xe0530001, 0, original.state.eip, &[]).unwrap(),
            original,
            PendingOwner::Replacement(0),
            None,
            None,
            None,
        )
        .unwrap();
    dispatcher(&mut engine, frames[1]);
    returned(&mut engine, first, 2, true);
    let nested = engine
        .finish_windows_exception_handler(77, first.id)
        .unwrap()
        .unwrap();
    assert_eq!(nested.procedure, 0x401001);
    assert_eq!(word(&engine, nested.record + 4), 0x10);
    returned(&mut engine, nested, 1, true);
    let outer = engine
        .finish_windows_exception_handler(77, first.id)
        .unwrap()
        .unwrap();
    assert_eq!(outer.procedure, 0x401002);
    assert_eq!(word(&engine, outer.record + 4), 0);
    assert_eq!(word(&engine, 0x7e000000), frames[0]);
}
