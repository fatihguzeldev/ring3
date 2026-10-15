use super::D3D8Runtime32;
use crate::{
    graphics::{
        ResourceId,
        d3d8::{
            api::{HResult, InterfaceKind, PresentationParameters32},
            device::Device,
            resources::SurfaceRef,
        },
    },
    memory::GuestAddress,
};

pub(super) struct Object32 {
    pub pointer: GuestAddress,
    pub interface: InterfaceKind,
    pub kind: ObjectKind32,
    pub private_unknowns: Vec<PrivateUnknown32>,
}

#[derive(Clone, Copy)]
pub(super) struct PrivateUnknown32 {
    pub key: [u8; 16],
    pub pointer: GuestAddress,
}

pub(super) enum ObjectKind32 {
    Direct3D,
    Device(Box<Device32>),
    Resource(Resource32),
    View(View32),
    SwapChain(SwapChain32),
}

pub(super) struct Device32 {
    pub backend: Device,
    pub parent: GuestAddress,
    pub focus_window: u32,
    pub behavior: u32,
    pub presentation: PresentationParameters32,
    pub implicit: Vec<GuestAddress>,
    pub resource_holds: Vec<GuestAddress>,
    pub target_holds: [Option<GuestAddress>; 2],
}

#[derive(Clone, Copy)]
pub(super) struct Resource32 {
    pub device: GuestAddress,
    pub resource: ResourceId,
}

#[derive(Clone, Copy)]
pub(super) struct View32 {
    pub device: GuestAddress,
    pub container: GuestAddress,
    pub surface: SurfaceRef,
}

pub(super) struct SwapChain32 {
    pub device: GuestAddress,
    pub presentation: PresentationParameters32,
    pub surfaces: Vec<GuestAddress>,
    pub resources: Vec<SurfaceRef>,
}

impl D3D8Runtime32 {
    pub(super) fn record_slot(&self, pointer: GuestAddress) -> Result<usize, HResult> {
        let identity = self.com.lookup(pointer)?;
        let slot = usize::try_from(identity.payload).map_err(|_| HResult::INVALID_CALL)?;
        self.records
            .get(slot)
            .and_then(Option::as_ref)
            .ok_or(HResult::INVALID_CALL)?;
        Ok(slot)
    }

    pub(super) fn object(&self, pointer: GuestAddress) -> Result<&Object32, HResult> {
        Ok(self.records[self.record_slot(pointer)?].as_ref().unwrap())
    }

    pub(super) fn counter_pointer(&self, pointer: GuestAddress) -> Result<GuestAddress, HResult> {
        match self.object(pointer)?.kind {
            ObjectKind32::View(view) => Ok(view.container),
            _ => Ok(self.object(pointer)?.pointer),
        }
    }

    pub(super) fn resource(
        &self,
        pointer: GuestAddress,
        device: GuestAddress,
    ) -> Result<SurfaceRef, HResult> {
        match self.object(pointer)?.kind {
            ObjectKind32::Resource(resource) if resource.device == device => Ok(SurfaceRef {
                resource: resource.resource,
                subresource: 0,
            }),
            ObjectKind32::View(view) if view.device == device => Ok(view.surface),
            _ => Err(HResult::INVALID_CALL),
        }
    }
}
