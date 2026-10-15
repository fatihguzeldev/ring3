use super::{CREATE_GATE, InterfaceKind, MethodGate};
use crate::loader::{SystemExport32, SystemExportTarget32, SystemModule32};

pub fn system_module(preferred_base: u32) -> SystemModule32 {
    let mut exports = vec![SystemExport32 {
        name: Some("Direct3DCreate8".into()),
        ordinal: Some(1),
        target: SystemExportTarget32::Gate(CREATE_GATE),
    }];
    for interface in InterfaceKind::ALL {
        for slot in 0..interface.methods().len() {
            exports.push(SystemExport32 {
                name: None,
                ordinal: None,
                target: SystemExportTarget32::PrivateGate(
                    MethodGate { interface, slot: slot as u8 }.id(),
                ),
            });
        }
    }
    SystemModule32 { name: "d3d8.dll".into(), preferred_base, exports }
}
