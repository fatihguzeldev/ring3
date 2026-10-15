use crate::{
    graphics::d3d8::{
        api::{AdapterProfile32, DisplayMode32, InterfaceKind, PresentationParameters32},
        device::{Device, DeviceDescription, DeviceLimits},
        resources::Format,
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::windows::{
        d3d8::{
            D3D8Config32, D3D8Runtime32,
            objects::{Device32, Object32, ObjectKind32},
        },
        objects::{GuestComCreation32, InterfaceLayout32},
    },
};

pub(in crate::process::windows::d3d8) const DATA: u32 = 0x2000;

pub(in crate::process::windows::d3d8) fn runtime() -> (D3D8Runtime32, AddressSpace, GuestAddress) {
    let mode = DisplayMode32 {
        width: 16,
        height: 16,
        refresh_rate: 60,
        format: Format::X8R8G8B8,
    };
    let config = D3D8Config32 {
        objects: PageRange::new(GuestAddress(0x1000_0000), 256).unwrap(),
        locks: PageRange::new(GuestAddress(0x2000_0000), 128).unwrap(),
        max_objects: 128,
        max_devices: 4,
        max_locks: 16,
        adapter: AdapterProfile32::new(mode, vec![mode]).unwrap(),
        device: DeviceLimits::default(),
    };
    let mut runtime = D3D8Runtime32::new(config).unwrap();
    runtime.methods = InterfaceKind::ALL
        .into_iter()
        .map(|interface| vec![GuestAddress(0x9000); interface.methods().len()])
        .collect();
    let mut memory = AddressSpace::new(256).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(DATA), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    let parent = runtime
        .com
        .create(
            &mut memory,
            0,
            &[InterfaceLayout32 {
                iid: InterfaceKind::Direct3D.iid(),
                methods: &runtime.methods[0],
            }],
        )
        .unwrap()
        .identity_pointer;
    runtime.records[0] = Some(Object32 {
        pointer: parent,
        interface: InterfaceKind::Direct3D,
        kind: ObjectKind32::Direct3D,
        private_unknowns: Vec::new(),
    });
    let device = runtime
        .com
        .create_with_lifetime(
            &mut memory,
            1,
            &[InterfaceLayout32 {
                iid: InterfaceKind::Device.iid(),
                methods: &runtime.methods[1],
            }],
            GuestComCreation32 {
                retain: &[parent],
                ..Default::default()
            },
        )
        .unwrap()
        .identity_pointer;
    let desc = DeviceDescription {
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
    };
    let mut backend = Device::new(1, desc, runtime.config.device).unwrap();
    while let Some(id) = backend.seal_batch().unwrap() {
        backend.acknowledge_batch(id).unwrap();
    }
    runtime.records[1] = Some(Object32 {
        pointer: device,
        interface: InterfaceKind::Device,
        private_unknowns: Vec::new(),
        kind: ObjectKind32::Device(Box::new(Device32 {
            backend,
            parent,
            focus_window: 1,
            behavior: 0x40,
            presentation: PresentationParameters32::decode([
                16, 16, 21, 1, 0, 3, 1, 1, 0, 0, 1, 0, 0,
            ]),
            implicit: Vec::with_capacity(4),
            resource_holds: Vec::with_capacity(6500),
            target_holds: [None; 2],
        })),
    });
    runtime.device_slots.push(1);
    (runtime, memory, device)
}

pub(in crate::process::windows::d3d8) fn read(memory: &AddressSpace, address: u32) -> u32 {
    let mut bytes = [0; 4];
    memory.read(GuestAddress(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

pub(in crate::process::windows::d3d8) fn texture(
    runtime: &mut D3D8Runtime32,
    memory: &mut AddressSpace,
    device: GuestAddress,
) -> GuestAddress {
    runtime
        .device_resource_call(memory, device, 20, &[device.0, 4, 4, 2, 0, 21, 2, DATA])
        .unwrap();
    GuestAddress(read(memory, DATA))
}
