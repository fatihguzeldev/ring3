use super::{DetectedEvent32, DetectionSlot32};
use crate::{
    cpu::x86::{EventOrigin32, ExceptionCause32, PagingAccess32},
    memory::Access,
    process::{CallError, EngineInstance, HostError},
};

pub(super) fn access(access: PagingAccess32) -> Access {
    match access {
        PagingAccess32::Read => Access::Read,
        PagingAccess32::Write => Access::Write,
        PagingAccess32::Execute => Access::Execute,
    }
}

impl EngineInstance {
    pub(super) fn has_certified_page_fault32(&self) -> bool {
        matches!(
            self.exception_detection.as_ref().map(|slot| &slot[0]),
            Some(DetectionSlot32::Active {
                page_fault: Some(_),
                ..
            })
        )
    }

    pub(super) fn check_page_fault_detection32(
        &self,
        epoch: u64,
        event: DetectedEvent32,
    ) -> Result<(), HostError> {
        match (self.certified_page_fault32(epoch), event) {
            (
                Some(actual),
                DetectedEvent32::Exception(EventOrigin32::Exception(ExceptionCause32::PageFault(
                    reported,
                ))),
            ) if actual == reported && self.cr2() == Some(actual.linear()) => Ok(()),
            (
                None,
                DetectedEvent32::Exception(EventOrigin32::Exception(ExceptionCause32::PageFault(
                    _,
                ))),
            )
            | (Some(_), _) => Err(HostError::Call(CallError::InvalidStop)),
            (None, _) => Ok(()),
        }
    }
}
