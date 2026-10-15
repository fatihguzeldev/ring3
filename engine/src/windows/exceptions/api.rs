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
}
pub const EXCEPTION_RETURN_GATE: u32 = 0x00121000;
impl ExceptionApi32 {
    pub const KERNEL32: &'static [Self] = &[
        Self::AddVectoredExceptionHandler,
        Self::RemoveVectoredExceptionHandler,
        Self::AddVectoredContinueHandler,
        Self::RemoveVectoredContinueHandler,
        Self::SetUnhandledExceptionFilter,
        Self::UnhandledExceptionFilter,
        Self::RaiseException,
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
        }
    }
    pub fn stack_words(self) -> u32 {
        match self {
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
