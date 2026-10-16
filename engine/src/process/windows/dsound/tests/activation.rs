use super::support::*;
use crate::{
    audio::dsound::{HResult, InterfaceKind, MethodGate},
    memory::GuestAddress,
    windows::{com::IID_IUNKNOWN, ole32::KnownClass32},
};
#[test]
fn activated_sound_remains_uninitialized_until_successful_initialize_and_keeps_iunknown_group() {
    let (mut runtime, mut memory, mut audio, _) = context();
    runtime
        .activate_class(
            &mut memory,
            &mut audio,
            &[],
            KnownClass32::DirectSound8,
            IID_IUNKNOWN,
            DATA + 800,
        )
        .unwrap();
    let unknown = GuestAddress(read(&memory, DATA + 800));
    assert_eq!(
        runtime.add_ref(&memory, unknown, InterfaceKind::Unknown),
        Ok(2)
    );
    memory
        .write(GuestAddress(DATA + 820), &InterfaceKind::Device8.iid().0)
        .unwrap();
    runtime
        .query_interface(&mut memory, unknown, DATA + 820, DATA + 804)
        .unwrap();
    let device = GuestAddress(read(&memory, DATA + 804));
    let query = MethodGate {
        interface: InterfaceKind::Device8,
        slot: 8,
    };
    assert_eq!(
        runtime.method_call(
            &mut memory,
            &mut audio,
            None,
            None,
            query,
            &[device.0, DATA + 808]
        ),
        Err(HResult(0x8878_00aa))
    );
    memory.write(GuestAddress(DATA + 840), &[0xff; 16]).unwrap();
    let initialize = MethodGate {
        interface: InterfaceKind::Device8,
        slot: 10,
    };
    assert_eq!(
        runtime.method_call(
            &mut memory,
            &mut audio,
            None,
            None,
            initialize,
            &[device.0, DATA + 840]
        ),
        Err(HResult::NO_DRIVER)
    );
    assert_eq!(
        runtime.method_call(
            &mut memory,
            &mut audio,
            None,
            None,
            query,
            &[device.0, DATA + 808]
        ),
        Err(HResult::UNINITIALIZED)
    );
    assert_eq!(
        runtime.method_call(
            &mut memory,
            &mut audio,
            None,
            None,
            initialize,
            &[device.0, 0]
        ),
        Ok(0)
    );
    assert_eq!(
        runtime.method_call(
            &mut memory,
            &mut audio,
            None,
            None,
            query,
            &[device.0, DATA + 808]
        ),
        Ok(0)
    );
    assert_eq!(
        runtime.method_call(
            &mut memory,
            &mut audio,
            None,
            None,
            initialize,
            &[device.0, 0]
        ),
        Err(HResult::ALREADY_INITIALIZED)
    );
}
