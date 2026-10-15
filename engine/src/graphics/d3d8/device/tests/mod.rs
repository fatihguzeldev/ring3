use super::super::{
    commands::{DrawBindings, DrawData, Primitive, StreamResource},
    resources::{
        BufferDesc, BufferKind, Format, Pool, ResourceDesc, SurfaceDesc, SurfaceRef, Usage,
    },
};
use super::*;

mod drawing;
mod lifetime;
mod retention;
mod transfers;

const DECLARATION: [u32; 3] = [0x2000_0000, 0x4002_0000, u32::MAX];
const VERTEX: [u32; 5] = [0xfffe_0101, 1, 0xc00f_0000, 0x90e4_0000, 0xffff];

fn description() -> DeviceDescription {
    DeviceDescription {
        width: 16,
        height: 16,
        format: Format::A8R8G8B8,
        back_buffers: 1,
        multisample: 0,
        depth: None,
        window: 1,
        interval: 0,
        swap_effect: 3,
        lockable_back_buffer: true,
        behavior: 0x40,
    }
}

fn device() -> Device {
    let mut device = Device::new(1, description(), DeviceLimits::default()).unwrap();
    drain(&mut device);
    device
}

fn drain(device: &mut Device) {
    while let Some(id) = device.seal_batch().unwrap() {
        device.acknowledge_batch(id).unwrap();
    }
}

fn buffer(length: u32, fvf: u32, pool: Pool) -> ResourceDesc {
    ResourceDesc::Buffer(BufferDesc {
        length,
        kind: BufferKind::Vertex { fvf },
        usage: Usage(0),
        pool,
    })
}

fn image(pool: Pool) -> ResourceDesc {
    ResourceDesc::Surface(SurfaceDesc {
        width: 16,
        height: 16,
        format: Format::A8R8G8B8,
        usage: Usage(0),
        pool,
        multisample: 0,
        lockable: true,
    })
}
