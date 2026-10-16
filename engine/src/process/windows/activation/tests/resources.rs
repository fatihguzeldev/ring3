use super::*;
use crate::windows::activation::ActivationError32 as E;

#[test]
fn hmodule_manifest_uses_both_resource_layouts_and_needs_no_virtual_filesystem() {
    for flags in [2, 0x20, 0x22] {
        let (mut engine, module) = engine(flags);
        assert!(engine.files.is_none());
        let handle = create(&mut engine);
        assert_eq!(engine.activation.as_ref().unwrap().native.current(1), 0);
        assert_eq!(
            engine
                .activation_call(1, Api::ActivateActCtx, &[handle, OUTPUT])
                .unwrap(),
            1
        );
        let cookie = word(&engine, OUTPUT);
        write(&mut engine, INPUT + 0x100, &CLSID);
        write(&mut engine, OUTPUT + 0x40, &64u32.to_le_bytes());
        assert_eq!(
            engine
                .activation_call(
                    1,
                    Api::FindActCtxSectionGuid,
                    &[1, 0, 4, INPUT + 0x100, OUTPUT + 0x40]
                )
                .unwrap(),
            1
        );
        let section = word(&engine, OUTPUT + 0x48);
        assert_eq!(word(&engine, section), 120);
        assert_eq!(word(&engine, section + 8), 4);
        assert_eq!(read::<16>(&engine, section + 12), CLSID);
        assert_eq!(word(&engine, OUTPUT + 0x60), handle);
        let path = engine.resource_module_file_name(module).unwrap();
        let resolved = engine.active_com_redirection(CLSID).unwrap().unwrap();
        let parent = path
            .iter()
            .rposition(|&unit| unit == u16::from(b'\\'))
            .unwrap();
        assert_eq!(&resolved.server[..parent + 1], &path[..parent + 1]);
        assert_eq!(
            &resolved.server[parent + 1..],
            &"private.dll".encode_utf16().collect::<Vec<_>>()
        );
        engine
            .activation_call(1, Api::ReleaseActCtx, &[handle])
            .unwrap();
        engine
            .activation_call(1, Api::ReleaseActCtx, &[handle])
            .unwrap();
        engine
            .activation_call(1, Api::DeactivateActCtx, &[0, cookie])
            .unwrap();
        assert!(
            engine
                .memory()
                .unwrap()
                .permissions_at(GuestAddress(section))
                .is_err()
        );
        assert_eq!(
            engine.activation_call(1, Api::ActivateActCtx, &[handle, OUTPUT]),
            Err(E::InvalidHandle)
        );
    }
}

#[test]
fn returned_context_and_query_outputs_are_fault_neutral_and_do_not_read_an_extra_index_word() {
    let (mut engine, _) = engine(2);
    let handle = create(&mut engine);
    engine
        .activation_call(1, Api::ActivateActCtx, &[handle, OUTPUT])
        .unwrap();
    let references = engine
        .activation
        .as_ref()
        .unwrap()
        .native
        .references(handle)
        .unwrap();
    let before = engine.memory().unwrap().transaction_version();
    assert_eq!(
        engine.activation_call(1, Api::GetCurrentActCtx, &[0x50000]),
        Err(E::NoAccess)
    );
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
    write(&mut engine, OUTPUT + 0xffc, &1u32.to_le_bytes());
    assert_eq!(
        engine
            .activation_call(
                1,
                Api::QueryActCtxW,
                &[
                    0x80000000,
                    handle,
                    OUTPUT + 0xffc,
                    3,
                    INPUT + 0x200,
                    0x800,
                    INPUT + 0x180
                ]
            )
            .unwrap(),
        1
    );
    assert_eq!(word(&engine, INPUT + 0x200 + 44), 1);
    assert_eq!(word(&engine, INPUT + 0x200 + 80), 1);
    write(&mut engine, INPUT + 0x200, &[0xa5; 32]);
    assert_eq!(
        engine.activation_call(
            1,
            Api::QueryActCtxW,
            &[0, handle, 0, 2, INPUT + 0x200, 32, INPUT + 0x180]
        ),
        Err(E::Win32(122))
    );
    assert_eq!(read::<32>(&engine, INPUT + 0x200), [0xa5; 32]);
    assert!(word(&engine, INPUT + 0x180) > 48);
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
}
