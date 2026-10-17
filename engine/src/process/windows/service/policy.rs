use crate::windows::{
    WindowsApi32, files::FileApi32, memory::MemoryApi32, nls::NlsApi32, process::ProcessApi32,
    rawinput::RawInputApi32, registry::Advapi32Api32, resources::ResourceApi32,
    synchronization::SynchronizationApi32, thread::ThreadApi32, user32::User32Api32,
};

pub(super) struct GuardFailure32 {
    pub(super) value: u32,
    pub(super) last_error: Option<u32>,
}

impl GuardFailure32 {
    pub(super) fn for_gate(id: u32) -> Option<Self> {
        let noaccess = |value| {
            Some(Self {
                value,
                last_error: Some(998),
            })
        };
        if let Some(api) = crate::windows::gdi::GdiApi32::from_id(id) {
            return if api.font_enumeration() || api.font_memory_resource() {
                Some(Self {
                    value: 0,
                    last_error: None,
                })
            } else {
                noaccess(api.failure_value())
            };
        }
        if let Some(api) = crate::windows::automation::Oleaut32Api32::from_id(id) {
            return if api.returns_hresult() {
                Some(Self { value: 0x8007_03e6, last_error: None })
            } else { noaccess(0) };
        }
        if let Some(api) = crate::windows::shell::ShellApi32::from_id(id) {
            return if matches!(api, crate::windows::shell::ShellApi32::SHGetFolderPathA | crate::windows::shell::ShellApi32::SHGetFolderPathW) {
                Some(Self { value: 0x8007_03e6, last_error: None })
            } else { noaccess(0) };
        }
        if crate::windows::version::VersionApi32::from_id(id).is_some()
            || crate::windows::shlwapi::ShlwapiApi32::from_id(id).is_some()
        { return noaccess(0); }
        if let Some(api) = crate::windows::activation::ActivationApi32::from_id(id) {
            return noaccess(api.failure_value());
        }
        if crate::windows::console::ConsoleApi32::from_id(id).is_some() {
            return noaccess(0);
        }
        if let Some(api) = crate::windows::security::SecurityApi32::from_id(id) {
            return if api.returns_status() {
                Some(Self {
                    value: 998,
                    last_error: None,
                })
            } else {
                noaccess(0)
            };
        }
        if crate::windows::native::NativeApi32::from_id(id).is_some() {
            return Some(Self {
                value: 0xc0000005,
                last_error: None,
            });
        }
        if let Some(api) = FileApi32::from_id(id) {
            return noaccess(api.failure_value());
        }
        if Advapi32Api32::from_id(id).is_some() {
            return Some(Self {
                value: 998,
                last_error: None,
            });
        }
        if let Some(api) = RawInputApi32::from_id(id) {
            return noaccess(api.failure_value());
        }
        if let Some(api) = User32Api32::from_id(id) {
            return noaccess(api.failure_value());
        }
        if ResourceApi32::from_id(id).is_some()
            || ProcessApi32::from_id(id).is_some()
            || NlsApi32::from_id(id).is_some()
        {
            return noaccess(0);
        }
        if let Some(api) = MemoryApi32::from_id(id) {
            return match api {
                MemoryApi32::HeapSize => noaccess(u32::MAX),
                MemoryApi32::LocalFree | MemoryApi32::GlobalFree => None,
                MemoryApi32::LocalFlags | MemoryApi32::GlobalFlags => noaccess(0x8000),
                MemoryApi32::HeapAlloc | MemoryApi32::HeapReAlloc => Some(Self {
                    value: 0,
                    last_error: None,
                }),
                _ => noaccess(0),
            };
        }
        if let Some(api) = ThreadApi32::from_id(id) {
            return match api {
                ThreadApi32::ExitThread => None,
                ThreadApi32::SuspendThread | ThreadApi32::ResumeThread | ThreadApi32::TlsAlloc => {
                    noaccess(u32::MAX)
                }
                ThreadApi32::GetThreadPriority => noaccess(0x7fff_ffff),
                _ => noaccess(0),
            };
        }
        if let Some(api) = SynchronizationApi32::from_id(id) {
            return match api {
                SynchronizationApi32::InitializeCriticalSection
                | SynchronizationApi32::DeleteCriticalSection
                | SynchronizationApi32::EnterCriticalSection
                | SynchronizationApi32::LeaveCriticalSection => None,
                _ => noaccess(0),
            };
        }
        if let Some(api) = WindowsApi32::from_id(id) {
            return match api {
                WindowsApi32::GetLastError
                | WindowsApi32::SetLastError
                | WindowsApi32::ExitProcess
                | WindowsApi32::Sleep => None,
                WindowsApi32::WaitForSingleObject
                | WindowsApi32::WaitForSingleObjectEx
                | WindowsApi32::WaitForMultipleObjects
                | WindowsApi32::WaitForMultipleObjectsEx
                | WindowsApi32::SignalObjectAndWait => noaccess(u32::MAX),
                _ => noaccess(0),
            };
        }
        None
    }
}

pub(super) fn is_memory_fault(error: crate::process::HostError) -> bool {
    use crate::{
        memory::MemoryError,
        process::{CallError, HostError},
        windows::user32::User32Error32,
    };
    matches!(
        error,
        HostError::Memory(MemoryError::Fault(_))
            | HostError::Call(CallError::Memory(MemoryError::Fault(_)))
            | HostError::User32(User32Error32::Memory(MemoryError::Fault(_)))
            | HostError::Gdi(crate::windows::gdi::GdiError32::Memory(MemoryError::Fault(_)))
    )
}
