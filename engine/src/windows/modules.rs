use super::WindowsApi32;
use crate::loader::{SystemExport32, SystemExportTarget32, SystemModule32};

pub fn kernel32_module(preferred_base: u32) -> SystemModule32 {
    SystemModule32 {
        name: "kernel32.dll".into(),
        preferred_base,
        exports: WindowsApi32::catalog()
            .map(|api| SystemExport32 {
                name: Some(api.name().into()),
                ordinal: None,
                target: SystemExportTarget32::Gate(api.id()),
            })
            .chain(
                super::files::FileApi32::ALL
                    .iter()
                    .copied()
                    .map(|api| SystemExport32 {
                        name: Some(api.name().into()),
                        ordinal: None,
                        target: SystemExportTarget32::Gate(api.id()),
                    }),
            )
            .collect(),
    }
}
