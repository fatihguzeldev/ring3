use super::*;
use crate::{abi::x86::decode_state, memory::Permissions, windows::user32::User32Api32 as Api};

#[test]
fn captured_guarded_dialog_admission_preserves_modal_failure_and_modeless_null() {
    for (api, expected) in [
        (Api::DialogBoxIndirectParamA, u32::MAX),
        (Api::DialogBoxIndirectParamW, u32::MAX),
        (Api::CreateDialogIndirectParamA, 0),
        (Api::CreateDialogIndirectParamW, 0),
    ] {
        let mut engine = engine();
        let teb = engine.module_teb_address().unwrap();
        for address in [DATA, teb] {
            engine
                .memory
                .as_mut()
                .unwrap()
                .protect(
                    PageRange::new(GuestAddress(address & !4095), 1).unwrap(),
                    Permissions::READ_WRITE.with_guard(),
                )
                .unwrap();
        }
        call(&mut engine, api.id(), &[0x400000, DATA, 0, CODE + 0x100, 0]).unwrap();
        assert_eq!(
            decode_state(&engine.arena()[..STATE_SIZE])
                .unwrap()
                .registers[0],
            expected
        );
        assert_eq!(engine.current_windows_thread().unwrap().last_error(), 998);
        assert!(engine.pending_call.is_none());
        assert!(engine.pending_windows_callback().is_none());
        assert_eq!(engine.user32_windows().unwrap().live_windows(), 0);
        assert!(
            engine
                .memory
                .as_ref()
                .unwrap()
                .guard_page(GuestAddress(teb))
                .is_none()
        );
        assert!(
            engine
                .memory
                .as_ref()
                .unwrap()
                .guard_page(GuestAddress(DATA))
                .is_some()
        );
    }
}
