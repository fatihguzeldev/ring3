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
            .chain(super::memory::MemoryApi32::ALL.iter().copied().map(|api| SystemExport32 { name: Some(api.name().into()), ordinal: None, target: SystemExportTarget32::Gate(api.id()) }))
            .chain(super::process::ProcessApi32::ALL.iter().copied().map(|api| SystemExport32 { name: Some(api.name().into()), ordinal: None, target: SystemExportTarget32::Gate(api.id()) }))
            .chain(super::synchronization::SynchronizationApi32::object_apis().iter().copied().map(|api| SystemExport32 {name:Some(api.name().into()),ordinal:None,target:SystemExportTarget32::Gate(api.id())}))
            .chain(super::thread::ThreadApi32::core_apis().iter().copied().map(|api| SystemExport32 {name:Some(api.name().into()),ordinal:None,target:SystemExportTarget32::Gate(api.id())}))
            .chain([0x000f_fffe, 0x000f_ffff].into_iter().map(|id| SystemExport32 { name: None, ordinal: None, target: SystemExportTarget32::PrivateGate(id) }))
            .chain(super::exceptions::ExceptionApi32::KERNEL32.iter().copied().map(|api|SystemExport32{name:Some(api.name().into()),ordinal:None,target:if super::exceptions::ExceptionApi32::NTDLL.contains(&api){SystemExportTarget32::Forwarder(format!("ntdll.{}",api.name()))}else{SystemExportTarget32::Gate(api.id())}}))
            .chain(super::nls::NlsApi32::ALL.iter().copied().map(|api|SystemExport32{name:Some(api.name().into()),ordinal:None,target:SystemExportTarget32::Gate(api.id())}))
            .chain([SystemExport32{name:None,ordinal:None,target:SystemExportTarget32::PrivateGate(super::exceptions::EXCEPTION_RETURN_GATE)}])
            .chain(super::resources::kernel32_resource_exports())
            .collect(),
    }
}
