use super::{
    borrowed::{BorrowedCall32, ObjectWitnesses32, OutputWitness32},
    cursor::ReferenceCursor32,
};
use crate::{
    process::HostError,
    process::{
        calls::call::PendingOwner,
        windows::{automation::variants::OwnedValuePlan32, callbacks::GuestCallbackScratch32},
    },
    windows::CallFrame32,
};

#[derive(Clone, Copy)]
pub(super) enum Action32 {
    Acquire,
    Release,
    Compensate,
    Query(u16),
    Invoke,
    ReleaseTemporary,
}

#[derive(Clone, Copy)]
pub(super) enum Stage32 {
    Acquire,
    Release,
    NeedCall {
        action: Action32,
        object: u32,
    },
    Calling {
        action: Action32,
        call: BorrowedCall32,
    },
    Received {
        action: Action32,
        result: u32,
    },
    QueryOwned(u16),
    InvokeReturned {
        status: u32,
        returned: crate::windows::automation::variants::Variant32,
    },
    InvokeConvert(u32),
    ReleaseTemporaries,
    RetireTemporaries,
    Rollback,
    Publish,
    Complete(u32),
    Failed(HostError),
}

pub(in crate::process::windows::automation) struct ReferenceWork32 {
    pub(super) owner: PendingOwner,
    pub(super) token: u32,
    pub(super) frame: CallFrame32,
    pub(in crate::process::windows::automation) thread: u32,
    pub(super) continuation: u64,
    pub(super) destination: u32,
    pub(super) output: Option<OutputWitness32>,
    pub(in crate::process::windows::automation) array:
        Option<crate::process::windows::automation::arrays::ArrayLoan32>,
    pub(super) plan: OwnedValuePlan32,
    pub(super) acquire: ReferenceCursor32,
    pub(super) release: ReferenceCursor32,
    pub(super) rollback: ReferenceCursor32,
    pub(super) acquired: usize,
    pub(super) rollback_remaining: usize,
    pub(super) query_owned: Option<u32>,
    pub(super) failure: Option<HostError>,
    pub(super) objects: ObjectWitnesses32,
    pub(super) stage: Stage32,
    pub(super) scratch: Option<GuestCallbackScratch32>,
    pub(super) invoke: Option<super::invoke::InvokeState32>,
}
