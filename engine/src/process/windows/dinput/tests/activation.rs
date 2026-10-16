use super::support::*;
use crate::{
    input::dinput::{DirectInputResult32 as H, InterfaceKind32, MethodGate},
    memory::GuestAddress,
    process::WindowsProviderOutcome32,
    windows::com::IID_IUNKNOWN,
};
#[test]
fn activated_input_initialization_is_version_checked_and_failure_neutral() {
    let (mut runtime, mut memory, _, source) = runtime(true);
    runtime
        .activate_class(&mut memory, &[], true, IID_IUNKNOWN, DATA + 800)
        .unwrap();
    let pointer = GuestAddress(super::super::memory::words::<1>(&memory, DATA + 800).unwrap()[0]);
    let status = MethodGate {
        interface: InterfaceKind32::Input8A,
        slot: 5,
    };
    assert_eq!(
        runtime.method(
            &mut memory,
            None,
            None,
            &source,
            status,
            &[pointer.0, DATA + 32]
        ),
        Err(H::NOT_INITIALIZED)
    );
    let initialize = MethodGate {
        interface: InterfaceKind32::Input8A,
        slot: 7,
    };
    assert_eq!(
        runtime.method(
            &mut memory,
            None,
            None,
            &source,
            initialize,
            &[pointer.0, 0x400000, 0x700]
        ),
        Err(H(0x8007_047e))
    );
    assert_eq!(
        runtime.method(
            &mut memory,
            None,
            None,
            &source,
            status,
            &[pointer.0, DATA + 32]
        ),
        Err(H::NOT_INITIALIZED)
    );
    assert_eq!(
        runtime.method(
            &mut memory,
            None,
            None,
            &source,
            initialize,
            &[pointer.0, 0x400000, 0x800]
        ),
        Ok(WindowsProviderOutcome32::Return(0))
    );
    assert_eq!(
        runtime.method(
            &mut memory,
            None,
            None,
            &source,
            status,
            &[pointer.0, DATA + 32]
        ),
        Ok(WindowsProviderOutcome32::Return(0))
    );
    assert_eq!(
        runtime.method(
            &mut memory,
            None,
            None,
            &source,
            initialize,
            &[pointer.0, 0x400000, 0x800]
        ),
        Ok(WindowsProviderOutcome32::Return(0))
    );
}
