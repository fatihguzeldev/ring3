use crate::{
    memory::GuestAddress,
    process::{
        EngineInstance, HostError,
        calls::call::PendingOwner,
        windows::{
            callbacks::GuestCallbackScratch32,
            ole32::marshaling::{ExecutionApartment32, Remote32},
        },
    },
    windows::CallFrame32,
};
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::process::windows::ole32) enum TransferParent32 {
    Activation(u64),
    Proxy(u64),
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::process::windows::ole32) enum TransferPhase32 {
    QueryMarshal,
    Class,
    Size,
    Marshal,
    Activate,
    Unmarshal,
    DiscardPacket,
    Release(usize),
    Complete,
}
#[derive(Clone, Copy)]
pub(in crate::process::windows::ole32) struct NativeReference32 {
    pub pointer: u32,
    pub execution: ExecutionApartment32,
}
pub(in crate::process::windows::ole32) struct TransferCall32 {
    pub continuation: u64,
    pub parent: TransferParent32,
    pub owner: PendingOwner,
    pub token: u32,
    pub frame: CallFrame32,
    pub sender: u32,
    pub source: ExecutionApartment32,
    pub client: ExecutionApartment32,
    pub execution: ExecutionApartment32,
    pub scratch: Option<GuestCallbackScratch32>,
    pub remote: Remote32,
    pub phase: TransferPhase32,
    pub(in crate::process::windows::ole32) references: [NativeReference32; 4],
    pub stream: Option<GuestAddress>,
    pub packet: bool,
    pub result: u32,
    pub output: u32,
    pub abandoned: bool,
    pub cancelled: bool,
    pub dead_sta: [u32; 2],
}
impl TransferCall32 {
    pub(super) fn pointer(&self, index: usize) -> u32 {
        self.references[index].pointer
    }
    pub(in crate::process::windows::ole32) fn scratch(&self) -> GuestCallbackScratch32 {
        self.scratch
            .expect("active transfer retains its exact scratch")
    }
}
impl EngineInstance {
    pub(in crate::process::windows::ole32) fn com_transfer_index(
        &self,
        parent: TransferParent32,
    ) -> Option<usize> {
        self.ole32
            .as_ref()?
            .marshaling
            .transfers
            .iter()
            .position(|call| call.parent == parent)
    }
    pub(in crate::process::windows::ole32) fn com_transfer_result(
        &self,
        parent: TransferParent32,
    ) -> Option<(u32, u32)> {
        self.com_transfer_index(parent).and_then(|index| {
            let call = &self.ole32.as_ref().unwrap().marshaling.transfers[index];
            (call.phase == TransferPhase32::Complete).then_some((call.result, call.output))
        })
    }
    pub(in crate::process::windows::ole32) fn finish_com_transfer_metadata(
        &mut self,
        parent: TransferParent32,
    ) {
        let index = self
            .com_transfer_index(parent)
            .expect("parent owns completed transfer");
        let call = &self.ole32.as_ref().unwrap().marshaling.transfers[index];
        assert!(
            call.phase == TransferPhase32::Complete
                && call.scratch.is_none()
                && call.stream.is_none()
        );
        self.ole32
            .as_mut()
            .unwrap()
            .marshaling
            .transfers
            .remove(index);
    }
    pub(in crate::process::windows::ole32) fn transfer_scratch_check(
        &self,
        index: usize,
    ) -> Result<(), HostError> {
        let call = &self.ole32.as_ref().unwrap().marshaling.transfers[index];
        if let Some(scratch) = call.scratch {
            scratch.check(self.memory()?).map_err(HostError::Memory)
        } else if matches!(
            call.phase,
            TransferPhase32::Release(_) | TransferPhase32::Complete
        ) {
            Ok(())
        } else {
            Err(HostError::Infrastructure)
        }
    }
}
