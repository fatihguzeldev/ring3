use super::context::{NativeReference32, TransferCall32, TransferParent32, TransferPhase32};
use crate::{
    process::calls::call::PendingOwner,
    process::{
        EngineInstance, HostError,
        windows::{
            callbacks::GuestCallbackScratch32,
            ole32::{
                activation::ActivationRuntime32,
                marshaling::{ExecutionApartment32, Remote32},
            },
        },
    },
    windows::{
        CallFrame32,
        com::InterfaceId,
        ole32::{ApartmentId32, IID_IMARSHAL},
    },
};
pub(in crate::process::windows::ole32) struct TransferRequest32 {
    pub parent: TransferParent32,
    pub source: ExecutionApartment32,
    pub client: ApartmentId32,
    pub sender: u32,
    pub owner: PendingOwner,
    pub token: u32,
    pub frame: CallFrame32,
    pub pointer: u32,
    pub requested: InterfaceId,
}
impl EngineInstance {
    pub(in crate::process::windows::ole32) fn begin_com_transfer(
        &mut self,
        request: TransferRequest32,
    ) -> Result<(), HostError> {
        if self.com_transfer_index(request.parent).is_some() {
            return Ok(());
        }
        if self
            .checked_pending_call(request.owner, request.token)?
            .frame
            != request.frame
        {
            return Err(HostError::InvalidRequest);
        }
        let runtime = self.ole32.as_mut().ok_or(HostError::InvalidRequest)?;
        if runtime.marshaling.transfers.len() >= 64 {
            return Err(HostError::Infrastructure);
        }
        runtime
            .marshaling
            .transfers
            .try_reserve(1)
            .map_err(|_| HostError::Infrastructure)?;
        let continuation = runtime
            .activation
            .next
            .checked_add(1)
            .ok_or(HostError::Infrastructure)?;
        let mut bytes = [0u8; 128];
        bytes[..16].copy_from_slice(&request.requested.0);
        bytes[16..32].copy_from_slice(&IID_IMARSHAL.0);
        let scratch = GuestCallbackScratch32::allocate(
            self.memory.as_mut().ok_or(HostError::Closed)?,
            ActivationRuntime32::scratch_reservation(),
            &bytes,
        )
        .map_err(HostError::Memory)?;
        let empty = NativeReference32 {
            pointer: 0,
            execution: request.source,
        };
        let mut references = [empty; 4];
        references[0].pointer = request.pointer;
        let client = ExecutionApartment32 {
            apartment: request.client,
            thread: request.sender,
        };
        let runtime = self.ole32.as_mut().unwrap();
        runtime.activation.next = continuation;
        runtime.marshaling.transfers.push(TransferCall32 {
            continuation,
            parent: request.parent,
            owner: request.owner,
            token: request.token,
            frame: request.frame,
            sender: request.sender,
            source: request.source,
            client,
            execution: request.source,
            scratch: Some(scratch),
            remote: Remote32::Idle,
            phase: TransferPhase32::QueryMarshal,
            references,
            stream: None,
            packet: false,
            result: 0,
            output: 0,
            abandoned: false,
            cancelled: false,
            dead_sta: [0; 2],
        });
        self.advance_com_transfer(self.ole32.as_ref().unwrap().marshaling.transfers.len() - 1)
    }
}
