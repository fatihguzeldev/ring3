use super::{FactoryGate, InterfaceKind32, MethodGate};
use crate::loader::{SystemExport32, SystemExportTarget32, SystemModule32};
pub fn system_module(base: u32, version8: bool) -> SystemModule32 {
    let mut exports = Vec::new();
    for gate in FactoryGate::ALL
        .into_iter()
        .filter(|gate| (*gate == FactoryGate::Create8) == version8)
    {
        exports.push(SystemExport32 {
            name: Some(gate.symbol().into()),
            ordinal: None,
            target: SystemExportTarget32::Gate(gate.id()),
        });
    }
    for interface in InterfaceKind32::ALL
        .into_iter()
        .filter(|interface| (interface.generation() == 8) == version8)
    {
        for slot in 0..interface.words().len() {
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
        name: if version8 {
            "dinput8.dll"
        } else {
            "dinput.dll"
        }
        .into(),
        preferred_base: base,
        exports,
    }
}
