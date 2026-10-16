use super::*;
use crate::{
    process::{HostError, WindowsCallbackProvider32, WindowsProviderOutcome32},
    windows::nls::NlsError32,
};
#[test]
fn locale_name_apis_preserve_default_invariant_neutral_and_alternate_sort_identities() {
    let mut engine = engine();
    wide(&mut engine, 0x10000, "tR-tr");
    assert_eq!(
        call(&mut engine, NlsApi32::LocaleNameToLCID, &[0x10000, 0]),
        0x41f
    );
    wide(&mut engine, 0x10000, "de-DE_phoneb");
    assert_eq!(
        call(&mut engine, NlsApi32::LocaleNameToLCID, &[0x10000, 0]),
        0x10407
    );
    assert_eq!(
        call(
            &mut engine,
            NlsApi32::LCIDToLocaleName,
            &[0x10407, 0x11000, 85, 0]
        ),
        13
    );
    assert_eq!(
        read::<26>(&engine, 0x11000),
        "de-DE_phoneb\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
            .as_slice()
    );
    wide(&mut engine, 0x10000, "en");
    assert_eq!(
        call(&mut engine, NlsApi32::LocaleNameToLCID, &[0x10000, 0]),
        0x409
    );
    assert_eq!(
        call(
            &mut engine,
            NlsApi32::LocaleNameToLCID,
            &[0x10000, 0x08000000]
        ),
        9
    );
    wide(&mut engine, 0x10000, "");
    assert_eq!(
        call(&mut engine, NlsApi32::LocaleNameToLCID, &[0x10000, 0]),
        0x7f
    );
    assert_eq!(
        call(&mut engine, NlsApi32::LocaleNameToLCID, &[0, 0]),
        0x409
    );
    assert_eq!(
        call(
            &mut engine,
            NlsApi32::GetUserDefaultLocaleName,
            &[0x11000, 85]
        ),
        6
    );
    write(&mut engine, 0x11000, &[0xa5; 16]);
    assert_eq!(
        engine.execute_nls_call(NlsApi32::LCIDToLocaleName, &[0x411, 0x11000, 2, 0]),
        Err(NlsError32::InsufficientBuffer)
    );
    assert_eq!(read::<16>(&engine, 0x11000), [0xa5; 16]);
    wide(&mut engine, 0x10000, "ja-JP");
    assert_eq!(
        call(
            &mut engine,
            NlsApi32::GetLocaleInfoEx,
            &[0x10000, 0x20001004, 0x11000, 2]
        ),
        2
    );
    assert_eq!(u32::from_le_bytes(read::<4>(&engine, 0x11000)), 932);
    wide(&mut engine, 0x10000, "unknown-locale");
    assert_eq!(
        call(&mut engine, NlsApi32::IsValidLocaleName, &[0x10000]),
        0
    );
}
#[test]
fn locale_enumeration_calls_real_three_argument_callbacks_and_stop_cleans_exact_scratch() {
    let mut engine = engine();
    engine
        .memory
        .as_mut()
        .unwrap()
        .protect(
            PageRange::new(GuestAddress(0x10000), 1).unwrap(),
            Permissions::READ_EXECUTE,
        )
        .unwrap();
    let request = match engine
        .begin_nls_enumeration(&[0x10000, 0x21, 0xabcdef, 0])
        .unwrap()
    {
        WindowsProviderOutcome32::Callback(request) => request,
        _ => panic!("specific locale inventory"),
    };
    assert_eq!(request.provider, WindowsCallbackProvider32::Nls);
    assert_eq!(request.count, 3);
    assert_eq!(request.arguments[1], 0x21);
    assert_eq!(request.arguments[2], 0xabcdef);
    let first = request.arguments[0];
    let next = match engine.resume_nls_callback(request.continuation, 1).unwrap() {
        WindowsProviderOutcome32::Callback(request) => request,
        _ => panic!("multiple installed locales"),
    };
    assert_eq!(next.arguments[0], first);
    assert_eq!(
        engine.resume_nls_callback(next.continuation, 0),
        Ok(WindowsProviderOutcome32::Return(1))
    );
    assert!(
        engine
            .memory()
            .unwrap()
            .permissions_at(GuestAddress(first))
            .is_err()
    );
    assert!(matches!(
        engine.resume_nls_callback(next.continuation, 1),
        Err(HostError::Nls(NlsError32::InvalidParameter))
    ));
    assert!(matches!(
        engine.begin_nls_enumeration(&[0x10000, 8, 0, 0]),
        Err(HostError::Nls(NlsError32::InvalidFlags))
    ));
    assert_eq!(
        engine.begin_nls_enumeration(&[0x10000, 2, 0, 0]),
        Ok(WindowsProviderOutcome32::Return(1))
    );
}
