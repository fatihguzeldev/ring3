use super::{FactoryGate, InterfaceKind, MethodGate};
use crate::loader::{SystemExport32, SystemExportTarget32, SystemModule32};

pub fn system_module(preferred_base: u32) -> SystemModule32 {
    let mut exports = Vec::new();
    for gate in FactoryGate::ALL {
        let ordinal = match gate {
            FactoryGate::Create => 1,
            FactoryGate::EnumerateA => 2,
            FactoryGate::EnumerateW => 3,
            FactoryGate::CaptureCreate => 6,
            FactoryGate::CaptureEnumerateA => 7,
            FactoryGate::CaptureEnumerateW => 8,
            FactoryGate::GetDeviceId => 9,
            FactoryGate::Create8 => 10,
            FactoryGate::CaptureCreate8 => 11,
            FactoryGate::FullDuplexCreate8 => 12,
        };
        exports.push(SystemExport32 {
            name: Some(gate.name().into()),
            ordinal: Some(ordinal),
            target: SystemExportTarget32::Gate(gate.id()),
        });
    }
    for interface in InterfaceKind::ALL {
        for slot in 0..interface.methods().len() {
            exports.push(SystemExport32 {
                name: None,
                ordinal: None,
                target: SystemExportTarget32::PrivateGate(
                    MethodGate {
                        interface,
                        slot: slot as u8,
                    }
                    .id(),
                ),
            });
        }
    }
    SystemModule32 {
        name: "dsound.dll".into(),
        preferred_base,
        exports,
    }
}
