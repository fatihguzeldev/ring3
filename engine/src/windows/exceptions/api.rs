#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum ExceptionApi32 {
    AddVectoredExceptionHandler = 0x00120001,
    RemoveVectoredExceptionHandler,
    AddVectoredContinueHandler,
    RemoveVectoredContinueHandler,
    SetUnhandledExceptionFilter,
    UnhandledExceptionFilter,
    RaiseException,
    RtlUnwind,
    RtlCaptureContext,
    RtlRestoreContext,
    NtContinue,
    RtlRaiseException,
    NtRaiseException,
    DebugBreak,
}
pub const EXCEPTION_RETURN_GATE: u32 = 0x00121000;
pub const EXCEPTION_NESTED_GATE: u32 = 0x00121001;
pub const EXCEPTION_UNWIND_GATE: u32 = 0x00121002;
impl ExceptionApi32 {
    pub const KERNEL32: &'static [Self] = &[
        Self::AddVectoredExceptionHandler,
        Self::RemoveVectoredExceptionHandler,
        Self::AddVectoredContinueHandler,
        Self::RemoveVectoredContinueHandler,
        Self::SetUnhandledExceptionFilter,
        Self::UnhandledExceptionFilter,
        Self::RaiseException,
        Self::DebugBreak,
        Self::RtlUnwind,
        Self::RtlCaptureContext,
        Self::RtlRestoreContext,
    ];
    pub const NTDLL: &'static [Self] = &[
        Self::RtlUnwind,
        Self::RtlCaptureContext,
        Self::RtlRestoreContext,
        Self::NtContinue,
        Self::RtlRaiseException,
        Self::NtRaiseException,
    ];
    pub fn from_id(id: u32) -> Option<Self> {
        Self::KERNEL32
            .iter()
            .chain(Self::NTDLL)
            .copied()
            .find(|api| api.id() == id)
    }
    pub fn id(self) -> u32 {
        self as u32
    }
    pub fn convention(self) -> crate::windows::CallingConvention32 {
        if self == Self::RtlRestoreContext {
            crate::windows::CallingConvention32::Cdecl
        } else {
            crate::windows::CallingConvention32::Stdcall
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::AddVectoredExceptionHandler => "AddVectoredExceptionHandler",
            Self::RemoveVectoredExceptionHandler => "RemoveVectoredExceptionHandler",
            Self::AddVectoredContinueHandler => "AddVectoredContinueHandler",
            Self::RemoveVectoredContinueHandler => "RemoveVectoredContinueHandler",
            Self::SetUnhandledExceptionFilter => "SetUnhandledExceptionFilter",
            Self::UnhandledExceptionFilter => "UnhandledExceptionFilter",
            Self::RaiseException => "RaiseException",
            Self::RtlUnwind => "RtlUnwind",
            Self::RtlCaptureContext => "RtlCaptureContext",
            Self::RtlRestoreContext => "RtlRestoreContext",
            Self::NtContinue => "NtContinue",
            Self::RtlRaiseException => "RtlRaiseException",
            Self::NtRaiseException => "NtRaiseException",
            Self::DebugBreak => "DebugBreak",
        }
    }
    pub fn stack_words(self) -> u32 {
        match self {
            Self::DebugBreak => 0,
            Self::AddVectoredExceptionHandler
            | Self::AddVectoredContinueHandler
            | Self::RtlRestoreContext
            | Self::NtContinue => 2,
            Self::RaiseException | Self::RtlUnwind => 4,
            Self::NtRaiseException => 3,
            _ => 1,
        }
    }
}

pub fn ntdll_module(preferred_base: u32) -> crate::loader::SystemModule32 {
    use crate::loader::{SystemExport32, SystemExportTarget32, SystemModule32};
    SystemModule32 {
        name: "ntdll.dll".into(),
        preferred_base,
        exports: crate::windows::native::NativeApi32::ALL
            .into_iter()
            .map(|api| SystemExport32 {
                name: Some(api.name().into()),
                ordinal: None,
                target: SystemExportTarget32::Gate(api.id()),
            })
            .chain(
                ExceptionApi32::NTDLL
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
