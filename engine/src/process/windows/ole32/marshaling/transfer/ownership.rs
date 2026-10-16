use super::context::{TransferCall32, TransferParent32};
use crate::process::{EngineInstance, HostError};
impl EngineInstance {
    pub(in crate::process::windows::ole32) fn validate_com_transfer_identity(
        &self,
        call: &TransferCall32,
    ) -> Result<(), HostError> {
        let runtime = self.ole32.as_ref().ok_or(HostError::InvalidRequest)?;
        let original = match call.parent {
            TransferParent32::Activation(id) => runtime
                .activation
                .pending
                .iter()
                .find(|pending| pending.continuation == id)
                .map(|pending| (pending.owner, pending.token, pending.frame, pending.thread)),
            TransferParent32::RpcQuery(id) => self.rpc_parent_identity(id),
            TransferParent32::Proxy(id) => runtime
                .marshaling
                .calls
                .iter()
                .find(|pending| pending.continuation == id)
                .map(|pending| (pending.owner, pending.token, pending.frame, pending.thread)),
        };
        if original == Some((call.owner, call.token, call.frame, call.sender)) {
            Ok(())
        } else {
            Err(HostError::InvalidRequest)
        }
    }
    pub(in crate::process::windows::ole32) fn check_com_transfer_parent(
        &self,
        parent: TransferParent32,
    ) -> Result<(), HostError> {
        if let Some(index) = self.com_transfer_index(parent) {
            self.validate_com_transfer_identity(
                &self.ole32.as_ref().unwrap().marshaling.transfers[index],
            )?;
        }
        Ok(())
    }
}
