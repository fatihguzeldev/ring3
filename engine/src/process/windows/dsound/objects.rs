use crate::{
    audio::dsound::{Buffer, CaptureRing, Device},
    memory::GuestAddress,
    windows::synchronization::EventBinding32,
};

pub(super) struct Object32 {
    pub pointer: GuestAddress,
    pub counts: [u32; 4],
    pub kind: ObjectKind32,
}
pub(super) enum ObjectKind32 {
    Device(Device32),
    Buffer(Buffer32),
    Effect(super::effects::EffectObject32),
    CaptureDevice {
        version8: bool,
        initialized: bool,
    },
    CaptureBuffer(CaptureBuffer32),
    FullDuplex {
        device: Option<GuestAddress>,
        capture: Option<GuestAddress>,
    },
}
pub(super) struct Device32 {
    pub initialized: bool,
    pub native: Device,
    pub primary: Option<GuestAddress>,
}
pub(super) struct Buffer32 {
    pub effects_generation: u64,
    pub native: Buffer,
    pub device: GuestAddress,
    pub version8: bool,
    pub notifications: Vec<Notification32>,
}
pub(super) struct CaptureBuffer32 {
    pub native: CaptureRing,
    pub device: GuestAddress,
    pub version8: bool,
    pub notifications: Vec<Notification32>,
}
#[derive(Clone, Copy)]
pub(super) struct Notification32 {
    pub offset: u32,
    pub binding: EventBinding32,
}
