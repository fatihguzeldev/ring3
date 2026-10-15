#[path = "../../../../loader/modules/linking/native/image.rs"]
#[allow(dead_code)]
mod image;
#[path = "support.rs"]
mod support;

use ring3_engine::{
    abi::{
        arena::EXIT_OFFSET,
        x86::{decode_exit, decode_state},
    },
    cpu::ExitReason,
    memory::{GuestAddress, MemoryError},
    process::{HostError, ModuleOperation32},
    windows::WindowsApi32,
};
use support::*;

fn word(f: &Fixture, address: u32) -> u32 {
    let mut bytes = [0; 4];
    f.engine
        .memory()
        .unwrap()
        .read(GuestAddress(address), &mut bytes)
        .unwrap();
    u32::from_le_bytes(bytes)
}

#[test]
fn captured_module_handles_and_ordinal_data_exports_complete_once_for_both_owners() {
    for owner in [Owner::Replacement, Owner::Resident] {
        let mut f = fixture(owner, false);
        text(&mut f, TEXT, "KERNEL32.DLL", true);
        let (token, state, before) = capture(&mut f, WindowsApi32::GetModuleHandleW, &[TEXT]);
        complete(&mut f, token).unwrap();
        returned(&f, before, state, WindowsApi32::GetModuleHandleW, KERNEL);
        let unchanged = f.engine.arena().to_vec();
        assert!(complete(&mut f, token).is_err());
        assert_eq!(f.engine.arena(), unchanged);

        text(&mut f, TEXT, "main.exe", false);
        let (token, state, before) = capture(&mut f, WindowsApi32::GetModuleHandleA, &[TEXT]);
        complete(&mut f, token).unwrap();
        returned(&f, before, state, WindowsApi32::GetModuleHandleA, MAIN);

        assert!(matches!(
            f.engine.load_library_module("helper.dll", None).unwrap(),
            ModuleOperation32::Pending(_)
        ));
        let init = f.engine.module_initializer().unwrap().unwrap();
        return_initializer(&mut f, init, 1);
        f.engine.finish_module_initializer(init.token).unwrap();
        let (token, state, before) = capture(&mut f, WindowsApi32::GetProcAddress, &[LIBRARY, 2]);
        complete(&mut f, token).unwrap();
        returned(
            &f,
            before,
            state,
            WindowsApi32::GetProcAddress,
            LIBRARY + 0x2f00,
        );
    }
}

#[test]
fn module_failures_publish_teb_last_error_and_readonly_get_last_error_is_read_only() {
    for owner in [Owner::Replacement, Owner::Resident] {
        let mut f = fixture(owner, false);
        let teb = f.engine.module_teb_address().unwrap();
        assert_eq!(f.engine.segment_state().unwrap().bases[4], teb);
        for (api, text_value, args, error) in [
            (WindowsApi32::LoadLibraryA, "missing.dll", vec![TEXT], 126),
            (
                WindowsApi32::GetProcAddress,
                "missing",
                vec![KERNEL, TEXT],
                127,
            ),
            (WindowsApi32::FreeLibrary, "", vec![0x1234_0000], 6),
        ] {
            text(&mut f, TEXT, text_value, false);
            let (token, state, before) = capture(&mut f, api, &args);
            complete(&mut f, token).unwrap();
            returned(&f, before, state, api, 0);
            assert_eq!(word(&f, teb + 0x34), error);
        }
        f.engine.write32(teb + 0x34, 0xfeed_babe).unwrap();
        f.engine.protect(teb, 1, 1).unwrap();
        let (token, state, before) = capture(&mut f, WindowsApi32::GetLastError, &[]);
        complete(&mut f, token).unwrap();
        returned(&f, before, state, WindowsApi32::GetLastError, 0xfeed_babe);
        assert_eq!(word(&f, teb + 0x34), 0xfeed_babe);
    }
}

#[test]
fn load_library_attach_continuation_resumes_the_saved_windows_call_and_free_detaches() {
    for owner in [Owner::Replacement, Owner::Resident] {
        let mut f = fixture(owner, false);
        text(&mut f, TEXT, "helper.dll", true);
        let (token, state, before) = capture(&mut f, WindowsApi32::LoadLibraryW, &[TEXT]);
        complete(&mut f, token).unwrap();
        let init = f.engine.module_initializer().unwrap().unwrap();
        assert_eq!((init.entry, init.reason), (LIBRARY + 0x1000, 1));
        assert_eq!(word(&f, init.return_esp - 12), LIBRARY);
        assert_eq!(word(&f, init.return_esp - 8), 1);
        assert_eq!(word(&f, init.return_esp - 4), 0);
        return_initializer(&mut f, init, 1);
        assert_eq!(
            f.engine.finish_module_initializer(init.token).unwrap(),
            ModuleOperation32::Loaded { handle: LIBRARY }
        );
        returned(&f, before, state, WindowsApi32::LoadLibraryW, LIBRARY);
        assert!(complete(&mut f, token).is_err());

        let (token, state, before) = capture(&mut f, WindowsApi32::FreeLibrary, &[LIBRARY]);
        complete(&mut f, token).unwrap();
        let detach = f.engine.module_initializer().unwrap().unwrap();
        assert_eq!(detach.reason, 0);
        return_initializer(&mut f, detach, 1);
        assert_eq!(
            f.engine.finish_module_initializer(detach.token).unwrap(),
            ModuleOperation32::Released
        );
        returned(&f, before, state, WindowsApi32::FreeLibrary, 1);
        assert_eq!(f.engine.module_handle(Some("helper.dll")).unwrap(), None);
    }
}

#[test]
fn live_get_proc_address_demands_a_forwarded_module_and_resumes_after_attach() {
    for owner in [Owner::Replacement, Owner::Resident] {
        let mut f = fixture(owner, true);
        text(&mut f, TEXT, "forward", false);
        assert_eq!(f.engine.module_handle(Some("helper.dll")).unwrap(), None);
        let (token, state, before) = capture(&mut f, WindowsApi32::GetProcAddress, &[MAIN, TEXT]);
        complete(&mut f, token).unwrap();
        let init = f.engine.module_initializer().unwrap().unwrap();
        assert_eq!(init.entry, LIBRARY + 0x1000);
        return_initializer(&mut f, init, 1);
        assert_eq!(
            f.engine.finish_module_initializer(init.token).unwrap(),
            ModuleOperation32::ExportResolved {
                address: LIBRARY + 0x1000
            }
        );
        returned(
            &f,
            before,
            state,
            WindowsApi32::GetProcAddress,
            LIBRARY + 0x1000,
        );
        assert!(
            f.engine.free_library_module(LIBRARY).is_err(),
            "forwarded dependency creates no explicit LoadLibrary reference"
        );
    }
}

#[test]
fn failed_attach_windows_reply_retries_after_teb_repair_without_loading_again() {
    for owner in [Owner::Replacement, Owner::Resident] {
        let mut f = fixture(owner, false);
        text(&mut f, TEXT, "helper.dll", false);
        let (token, state, before) = capture(&mut f, WindowsApi32::LoadLibraryA, &[TEXT]);
        let pages = f.engine.memory().unwrap().mapped_pages();
        complete(&mut f, token).unwrap();
        let attach = f.engine.module_initializer().unwrap().unwrap();
        return_initializer(&mut f, attach, 0);
        let ModuleOperation32::Pending(detach) =
            f.engine.finish_module_initializer(attach.token).unwrap()
        else {
            panic!("failed attach schedules detach");
        };
        let teb = f.engine.module_teb_address().unwrap();
        f.engine.protect(teb, 1, 1).unwrap();
        return_initializer(&mut f, detach, 1);
        assert!(matches!(
            f.engine.finish_module_initializer(detach.token),
            Err(HostError::Memory(MemoryError::Fault(_)))
        ));
        assert_eq!(f.engine.module_handle(Some("helper.dll")).unwrap(), None);
        assert_eq!(f.engine.memory().unwrap().mapped_pages(), pages);
        f.engine.protect(teb, 1, 3).unwrap();
        match owner {
            Owner::Replacement => {
                assert_eq!(
                    f.engine.finish_module_initializer(detach.token).unwrap(),
                    ModuleOperation32::InitializationFailed,
                );
            }
            Owner::Resident => complete(&mut f, token).unwrap(),
        }
        assert_eq!(f.engine.module_initializer().unwrap(), None);
        assert_eq!(f.engine.module_handle(Some("helper.dll")).unwrap(), None);
        assert_eq!(f.engine.memory().unwrap().mapped_pages(), pages);
        returned(&f, before, state, WindowsApi32::LoadLibraryA, 0);
        assert_eq!(word(&f, teb + 0x34), 1114);
        let completed = f.engine.arena().to_vec();
        assert!(complete(&mut f, token).is_err());
        assert!(f.engine.finish_module_initializer(detach.token).is_err());
        assert_eq!(f.engine.arena(), completed);
    }
}

#[test]
fn nested_captured_load_finishes_inner_reply_then_restores_outer_initializer() {
    use ring3_engine::abi::arena::{TRANSFER_OFFSET, TRANSFER_SIZE};
    for owner in [Owner::Replacement, Owner::Resident] {
        let mut f = fixture(owner, false);
        text(&mut f, TEXT, "helper.dll", false);
        let (outer_token, outer_state, outer_before) =
            capture(&mut f, WindowsApi32::LoadLibraryA, &[TEXT]);
        complete(&mut f, outer_token).unwrap();
        let outer = f.engine.module_initializer().unwrap().unwrap();
        text(&mut f, TEXT + 0x40, "nested.dll", false);
        let outer_frame = [
            word(&f, outer.return_esp - 16),
            word(&f, outer.return_esp - 12),
            word(&f, outer.return_esp - 8),
            word(&f, outer.return_esp - 4),
        ];
        let (inner_token, inner_state, inner_before) = capture_at(
            &mut f,
            WindowsApi32::LoadLibraryA,
            &[TEXT + 0x40],
            outer.return_esp - 0x40,
            outer.entry + 5,
        );
        complete(&mut f, inner_token).unwrap();
        let inner = f.engine.module_initializer().unwrap().unwrap();
        assert_ne!(outer.token, inner.token);
        assert_eq!(inner.entry, NESTED + 0x1000);
        return_initializer(&mut f, inner, 1);
        assert_eq!(
            f.engine.finish_module_initializer(inner.token).unwrap(),
            ModuleOperation32::Loaded { handle: NESTED }
        );
        assert_eq!(
            f.engine.arena(),
            expected_return_to(
                inner_before,
                inner_state,
                WindowsApi32::LoadLibraryA,
                NESTED,
                outer.entry + 5,
            ),
        );
        assert_eq!(
            [
                word(&f, outer.return_esp - 16),
                word(&f, outer.return_esp - 12),
                word(&f, outer.return_esp - 8),
                word(&f, outer.return_esp - 4),
            ],
            outer_frame,
        );
        assert_eq!(f.engine.module_initializer().unwrap(), Some(outer));
        assert!(complete(&mut f, inner_token).is_err());
        return_initializer(&mut f, outer, 1);
        f.engine.finish_module_initializer(outer.token).unwrap();
        let mut expected = expected_return(
            outer_before,
            outer_state,
            WindowsApi32::LoadLibraryA,
            LIBRARY,
        );
        // each real nested capture publishes its own transfer record; outer return owns CPU state.
        expected[TRANSFER_OFFSET..TRANSFER_OFFSET + TRANSFER_SIZE]
            .copy_from_slice(&f.engine.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + TRANSFER_SIZE]);
        assert_eq!(f.engine.arena(), expected);
        assert_eq!(
            f.engine.module_handle(Some("helper.dll")).unwrap(),
            Some(LIBRARY)
        );
        assert_eq!(
            f.engine.module_handle(Some("nested.dll")).unwrap(),
            Some(NESTED)
        );
    }
}

#[test]
fn exit_process_detaches_dlls_then_publishes_a_nonreturning_exit() {
    for owner in [Owner::Replacement, Owner::Resident] {
        let mut f = fixture(owner, false);
        let ModuleOperation32::Pending(attach) =
            f.engine.load_library_module("helper.dll", None).unwrap()
        else {
            panic!("attach");
        };
        return_initializer(&mut f, attach, 1);
        f.engine.finish_module_initializer(attach.token).unwrap();
        let (token, _, _) = capture(&mut f, WindowsApi32::ExitProcess, &[0x89ab_cdef]);
        complete(&mut f, token).unwrap();
        let detach = f.engine.module_initializer().unwrap().unwrap();
        assert_eq!(detach.reason, 0);
        assert_ne!(word(&f, detach.return_esp - 4), 0);
        return_initializer(&mut f, detach, 1);
        f.engine.finish_module_initializer(detach.token).unwrap();
        assert_eq!(
            decode_exit(&f.engine.arena()[EXIT_OFFSET..EXIT_OFFSET + 40])
                .unwrap()
                .reason,
            ExitReason::ProcessExited { code: 0x89ab_cdef }
        );
        assert_ne!(
            decode_state(&f.engine.arena()[..56]).unwrap().eip,
            MAIN + 0x1000
        );
        assert!(complete(&mut f, token).is_err());
    }
}
