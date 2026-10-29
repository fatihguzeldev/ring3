use super::{
    D3D8Error32, D3D8ReadbackOwner32, D3D8Runtime32, objects::ObjectKind32, runtime::ReplyKind32,
};
use crate::{
    abi::graphics::d3d8::{ReadbackStatus, readback_receipt},
    graphics::{
        BatchId,
        d3d8::{
            api::HResult,
            commands::Progress,
            device::{Device, DeviceError, DeviceStatus},
        },
    },
    memory::GuestAddress,
    process::{EngineInstance, HostError},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct D3D8DeviceInfo32 {
    pub epoch: u64,
    pub pointer: GuestAddress,
    pub generation: u64,
    pub retired: bool,
    pub status: DeviceStatus,
}

impl D3D8Runtime32 {
    pub(super) fn device_by_epoch(&self, epoch: u64) -> Result<&Device, D3D8Error32> {
        for slot in &self.device_slots {
            if let Some(ObjectKind32::Device(device)) =
                self.records[*slot].as_ref().map(|object| &object.kind)
                && device.backend.queue().epoch() == epoch
            {
                return Ok(&device.backend);
            }
        }
        self.retired_devices
            .iter()
            .find(|retired| retired.device.backend.queue().epoch() == epoch)
            .map(|retired| &retired.device.backend)
            .ok_or(D3D8Error32::InvalidDevice)
    }
    pub(super) fn device_by_epoch_mut(&mut self, epoch: u64) -> Result<&mut Device, D3D8Error32> {
        let slot=self.device_slots.iter().copied().find(|slot|matches!(self.records[*slot].as_ref().map(|object|&object.kind),Some(ObjectKind32::Device(device)) if device.backend.queue().epoch()==epoch));
        if let Some(slot) = slot {
            let ObjectKind32::Device(device) = &mut self.records[slot].as_mut().unwrap().kind
            else {
                unreachable!()
            };
            return Ok(&mut device.backend);
        }
        self.retired_devices
            .iter_mut()
            .find(|retired| retired.device.backend.queue().epoch() == epoch)
            .map(|retired| &mut retired.device.backend)
            .ok_or(D3D8Error32::InvalidDevice)
    }
}

impl EngineInstance {
    pub fn d3d8_device(&self, index: usize) -> Result<Option<D3D8DeviceInfo32>, HostError> {
        self.check_media_output_session()?;
        let runtime = self
            .d3d8
            .as_ref()
            .ok_or(HostError::D3D8(D3D8Error32::NotConfigured))?;
        let (pointer, device, retired) = if let Some(slot) = runtime.device_slots.get(index) {
            let object = runtime.records[*slot]
                .as_ref()
                .ok_or(HostError::Infrastructure)?;
            let ObjectKind32::Device(device) = &object.kind else {
                return Err(HostError::Infrastructure);
            };
            (object.pointer, &device.backend, false)
        } else if let Some(retired) = runtime
            .retired_devices
            .get(index.saturating_sub(runtime.device_slots.len()))
        {
            (retired.pointer, &retired.device.backend, true)
        } else {
            return Ok(None);
        };
        Ok(Some(D3D8DeviceInfo32 {
            epoch: device.queue().epoch(),
            pointer,
            generation: device.generation(),
            retired,
            status: device.status(),
        }))
    }
    pub fn d3d8_seal_batch(&mut self, epoch: u64) -> Result<Option<BatchId>, HostError> {
        self.check_media_output_session()?;
        let runtime = self
            .d3d8
            .as_mut()
            .ok_or(HostError::D3D8(D3D8Error32::NotConfigured))?;
        runtime.collect_garbage().map_err(|error| match error {
            super::DispatchError32::Device(error) => HostError::D3D8(D3D8Error32::Device(error)),
            _ => HostError::Infrastructure,
        })?;
        let batch = runtime
            .device_by_epoch_mut(epoch)
            .map_err(HostError::D3D8)?
            .seal_batch()
            .map_err(device_error)?;
        if batch.is_none()
            && let Some(index) = runtime.retired_devices.iter().position(|retired| {
                retired.closed && retired.device.backend.queue().epoch() == epoch
            })
        {
            runtime.retired_devices.remove(index);
            if let Some(reply) = &mut self.d3d8_reply
                && reply.epoch == epoch
                && let ReplyKind32::Drain(result) = reply.kind
            {
                reply.kind = ReplyKind32::Ready(result);
            }
        }
        Ok(batch)
    }
    pub fn d3d8_batch_info(&self, epoch: u64) -> Result<Option<(BatchId, usize)>, HostError> {
        Ok(self
            .d3d8_batch_bytes(epoch)?
            .map(|(id, bytes)| (id, bytes.len())))
    }
    pub(in crate::process) fn d3d8_batch_bytes(
        &self,
        epoch: u64,
    ) -> Result<Option<(BatchId, &[u8])>, HostError> {
        self.check_media_output_session()?;
        Ok(self
            .d3d8
            .as_ref()
            .ok_or(HostError::D3D8(D3D8Error32::NotConfigured))?
            .device_by_epoch(epoch)
            .map_err(HostError::D3D8)?
            .queue()
            .peek())
    }
    pub fn d3d8_copy_batch(
        &self,
        epoch: u64,
        offset: u32,
        output: &mut [u8],
    ) -> Result<usize, HostError> {
        self.check_media_output_session()?;
        self.d3d8
            .as_ref()
            .ok_or(HostError::D3D8(D3D8Error32::NotConfigured))?
            .device_by_epoch(epoch)
            .map_err(HostError::D3D8)?
            .queue()
            .copy_range(offset, output)
            .map_err(|error| device_error(DeviceError::Command(error)))
    }
    pub fn d3d8_acknowledge_batch(&mut self, id: BatchId) -> Result<Progress, HostError> {
        self.check_media_output_session()?;
        self.d3d8
            .as_mut()
            .ok_or(HostError::D3D8(D3D8Error32::NotConfigured))?
            .device_by_epoch_mut(id.epoch)
            .map_err(HostError::D3D8)?
            .acknowledge_batch(id)
            .map_err(device_error)
    }
    pub fn d3d8_observe_device(
        &mut self,
        epoch: u64,
        generation: u64,
        status: DeviceStatus,
    ) -> Result<Progress, HostError> {
        self.memory()?;
        let progress = self
            .d3d8
            .as_mut()
            .ok_or(HostError::D3D8(D3D8Error32::NotConfigured))?
            .device_by_epoch_mut(epoch)
            .map_err(HostError::D3D8)?
            .observe_status(generation, status)
            .map_err(device_error)?
            .progress;
        if matches!(status, DeviceStatus::Lost | DeviceStatus::Removed) {
            let runtime = self.d3d8.as_mut().unwrap();
            if runtime
                .readback_input
                .as_ref()
                .is_some_and(|input| input.epoch() == epoch)
            {
                runtime.readback_input = None;
            }
        }
        if matches!(status, DeviceStatus::Lost | DeviceStatus::Removed) {
            self.fail_d3d8_readbacks_for_epoch(epoch, HResult::DEVICE_LOST.0);
        }
        Ok(progress)
    }
    pub fn d3d8_complete_readback(&mut self, bytes: &[u8]) -> Result<(), HostError> {
        self.complete_d3d8_readback(bytes, None)
    }

    pub(super) fn d3d8_complete_owned_readback(
        &mut self,
        bytes: &[u8],
        owner: D3D8ReadbackOwner32,
    ) -> Result<(), HostError> {
        self.complete_d3d8_readback(bytes, Some(owner))
    }

    fn complete_d3d8_readback(
        &mut self,
        bytes: &[u8],
        owner: Option<D3D8ReadbackOwner32>,
    ) -> Result<(), HostError> {
        self.memory()?;
        let receipt = readback_receipt(bytes).map_err(|_| HostError::InvalidRequest)?;
        let owner = if let Some(owner) = owner {
            self.checked_d3d8_readback_owner(owner, receipt.id)?;
            owner
        } else {
            self.d3d8_readback_owner(receipt.id)?
        };
        let device = self
            .d3d8
            .as_mut()
            .ok_or(HostError::D3D8(D3D8Error32::NotConfigured))?
            .device_by_epoch_mut(receipt.id.epoch)
            .map_err(HostError::D3D8)?;
        let (generation, _, source, _) = device
            .pending_readback_details(receipt.id)
            .ok_or(HostError::D3D8(D3D8Error32::StaleReply))?;
        if source != receipt.source {
            return Err(HostError::D3D8(D3D8Error32::StaleReply));
        }
        let failure = match receipt.status {
            ReadbackStatus::Complete => None,
            ReadbackStatus::DeviceLost => Some(DeviceError::Lost),
            ReadbackStatus::DeviceRemoved => Some(DeviceError::Removed),
            ReadbackStatus::Allocation => Some(DeviceError::Allocation),
        };
        if let Some(failure) = failure {
            device
                .cancel_readback(receipt.id, generation, failure)
                .map_err(device_error)?;
            self.fail_d3d8_readback_owner(owner, receipt.id, HResult::from(failure).0)?;
        } else {
            device
                .complete_readback(receipt.id, generation, receipt.bytes)
                .map_err(device_error)?;
        }
        Ok(())
    }
}
fn device_error(error: DeviceError) -> HostError {
    HostError::D3D8(D3D8Error32::Device(error))
}
