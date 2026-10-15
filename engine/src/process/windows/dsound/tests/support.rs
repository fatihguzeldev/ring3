use super::super::{DSoundConfig32, DSoundRuntime32, memory::write_words, objects::ObjectKind32};
use crate::{
    audio::{
        AudioConfig32, AudioContext, PcmFormat32, SampleEncoding32,
        dsound::{FactoryGate, InterfaceKind, WaveFormat},
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    windows::synchronization::{SynchronizationContext32, SynchronizationLimits32},
};
pub(super) const DATA: u32 = 0x2000;
pub(super) fn context() -> (DSoundRuntime32, AddressSpace, AudioContext, GuestAddress) {
    let mut runtime = DSoundRuntime32::new(DSoundConfig32 {
        objects: PageRange::new(GuestAddress(0x3000_0000), 128).unwrap(),
        locks: PageRange::new(GuestAddress(0x4000_0000), 128).unwrap(),
        max_objects: 64,
        max_locks: 16,
        max_notifications: 64,
        max_capture_bytes: 4096,
        capture_available: true,
    })
    .unwrap();
    runtime.methods = InterfaceKind::ALL
        .into_iter()
        .map(|interface| {
            (0..interface.methods().len())
                .map(|slot| GuestAddress(0x9000 + interface as u32 * 0x100 + slot as u32 * 4))
                .collect()
        })
        .collect();
    let mut memory = AddressSpace::new(256).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(DATA), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    let mut audio = AudioContext::new(
        7,
        AudioConfig32 {
            render_frames: 16,
            max_buffers: 16,
            max_voices: 16,
            max_buffer_bytes: 4096,
            ..Default::default()
        },
    )
    .unwrap();
    runtime
        .create_device(&mut memory, &mut audio, FactoryGate::Create8, &[0, DATA, 0])
        .unwrap();
    let device = GuestAddress(read(&memory, DATA));
    let index = runtime.slot(device).unwrap();
    let ObjectKind32::Device(record) = &mut runtime.records[index].as_mut().unwrap().kind else {
        unreachable!()
    };
    record.native.set_cooperative_level(1, 1).unwrap();
    (runtime, memory, audio, device)
}
pub(super) fn buffer(
    runtime: &mut DSoundRuntime32,
    memory: &mut AddressSpace,
    audio: &mut AudioContext,
    device: GuestAddress,
    flags: u32,
) -> GuestAddress {
    let format = WaveFormat {
        pcm: PcmFormat32 {
            sample_rate: 48_000,
            channels: 1,
            bits_per_sample: 8,
        },
        encoding: SampleEncoding32::Integer,
        extensible: false,
        channel_mask: 0,
    };
    memory
        .write(GuestAddress(DATA + 64), &format.encode()[..18])
        .unwrap();
    write_words(
        memory,
        DATA + 128,
        &[36, flags, 4, 0, DATA + 64, 0, 0, 0, 0],
    )
    .unwrap();
    runtime
        .create_buffer(memory, audio, device, DATA + 128, DATA + 4, 0)
        .unwrap();
    GuestAddress(read(memory, DATA + 4))
}
pub(super) fn read(memory: &AddressSpace, pointer: u32) -> u32 {
    let mut bytes = [0; 4];
    memory.read(GuestAddress(pointer), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}
pub(super) fn capture_buffer(
    runtime: &mut DSoundRuntime32,
    memory: &mut AddressSpace,
    audio: &mut AudioContext,
) -> GuestAddress {
    runtime
        .create_device(
            memory,
            audio,
            FactoryGate::CaptureCreate8,
            &[0, DATA + 400, 0],
        )
        .unwrap();
    let device = GuestAddress(read(memory, DATA + 400));
    let format = WaveFormat {
        pcm: PcmFormat32 {
            sample_rate: 48_000,
            channels: 1,
            bits_per_sample: 8,
        },
        encoding: SampleEncoding32::Integer,
        extensible: false,
        channel_mask: 0,
    };
    memory
        .write(GuestAddress(DATA + 64), &format.encode()[..18])
        .unwrap();
    write_words(memory, DATA + 128, &[28, 0, 4, 0, DATA + 64, 0, 0]).unwrap();
    runtime
        .capture_device_call(
            memory,
            audio,
            device,
            3,
            &[device.0, DATA + 128, DATA + 404, 0],
        )
        .unwrap();
    GuestAddress(read(memory, DATA + 404))
}
pub(super) fn sync() -> SynchronizationContext32 {
    SynchronizationContext32::new(
        9,
        SynchronizationLimits32 {
            events: 8,
            handles: 16,
            bindings: 64,
            waits: 8,
        },
        0,
    )
    .unwrap()
}
