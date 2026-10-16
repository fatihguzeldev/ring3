use super::*;
use crate::windows::activation::ActivationError32 as E;

#[test]
fn invalid_activation_cookie_and_read_only_section_output_preserve_stack_and_references() {
    let (mut engine, _) = engine(2);
    let handle = create(&mut engine);
    engine
        .activation_call(1, Api::ActivateActCtx, &[handle, OUTPUT])
        .unwrap();
    let cookie = word(&engine, OUTPUT);
    write(&mut engine, INPUT + 0x100, &CLSID);
    write(&mut engine, OUTPUT + 0x40, &64u32.to_le_bytes());
    engine
        .activation_call(
            1,
            Api::FindActCtxSectionGuid,
            &[0, 0, 4, INPUT + 0x100, OUTPUT + 0x40],
        )
        .unwrap();
    let section = word(&engine, OUTPUT + 0x48);
    let before = engine.memory().unwrap().transaction_version();
    let references = engine
        .activation
        .as_ref()
        .unwrap()
        .native
        .references(handle)
        .unwrap();
    assert_eq!(
        engine.activation_call(1, Api::ActivateActCtx, &[handle, section]),
        Err(E::NoAccess)
    );
    assert_eq!(
        engine.activation_call(2, Api::DeactivateActCtx, &[0, cookie]),
        Err(E::InvalidDeactivation)
    );
    assert_eq!(
        engine.activation.as_ref().unwrap().native.current(1),
        handle
    );
    assert_eq!(engine.activation.as_ref().unwrap().native.current(2), 0);
    assert_eq!(
        engine
            .activation
            .as_ref()
            .unwrap()
            .native
            .references(handle)
            .unwrap(),
        references
    );
    assert_eq!(engine.memory().unwrap().transaction_version(), before);
}
