use super::*;
use crate::process::windows::{
    callbacks::WindowsCallbackProvider32, resources::types::ResourceCallOutcome32 as Outcome,
};
#[test]
fn enumeration_callbacks_have_typed_arguments_and_cancel_reclaims_owned_name_scratch() {
    let mut engine = engine(true, 8);
    let Outcome::Callback(request) = engine
        .resource_call(Api::EnumResourceTypesW, &[0, BASE + 0x1000, 0xfeedbeef])
        .unwrap()
    else {
        panic!("callback")
    };
    assert_eq!(request.provider, WindowsCallbackProvider32::Resources);
    assert_eq!(request.count, 3);
    assert_eq!(request.arguments[0], BASE);
    assert_eq!(request.arguments[2], 0xfeedbeef);
    let pointer = request.arguments[1];
    assert!(pointer >= 0x16000000);
    assert_eq!(
        read::<10>(&engine, pointer),
        [b'T', 0, b'Y', 0, b'P', 0, b'E', 0, 0, 0]
    );
    let Outcome::Return {
        value: 0,
        last_error: None,
    } = engine
        .resume_resource_callback(request.continuation, 0)
        .unwrap()
    else {
        panic!("cancel")
    };
    assert!(
        engine
            .memory
            .as_ref()
            .unwrap()
            .permissions_at(GuestAddress(pointer))
            .is_err()
    );
    assert_eq!(engine.resources.as_ref().unwrap().enum_bytes, 0);
}
#[test]
fn complete_integer_type_and_language_enumeration_preserves_real_parameters() {
    let mut engine = engine(false, 8);
    let Outcome::Callback(first) = engine
        .resource_call(Api::EnumResourceTypesA, &[0, BASE + 0x1000, 77])
        .unwrap()
    else {
        panic!("callback")
    };
    assert_eq!(&first.arguments[..3], &[BASE, 6, 77]);
    let Outcome::Callback(second) = engine
        .resume_resource_callback(first.continuation, 1)
        .unwrap()
    else {
        panic!("callback")
    };
    assert_eq!(&second.arguments[..3], &[BASE, 10, 77]);
    assert!(matches!(
        engine.resume_resource_callback(first.continuation, 1),
        Ok(Outcome::Return {
            value: 1,
            last_error: None
        })
    ));
    let Outcome::Callback(language) = engine
        .resource_call(Api::EnumResourceLanguagesW, &[0, 10, 7, BASE + 0x1000, 88])
        .unwrap()
    else {
        panic!("callback")
    };
    assert_eq!(language.count, 5);
    assert_eq!(&language.arguments[..5], &[BASE, 10, 7, 0x409, 88]);
    assert!(matches!(
        engine.resume_resource_callback(language.continuation, 1),
        Ok(Outcome::Return {
            value: 1,
            last_error: None
        })
    ));
}
