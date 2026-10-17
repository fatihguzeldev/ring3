use crate::{
    graphics::{
        GraphicsLimits,
        d3d8::{
            api::{AdapterProfile32, DisplayMode32},
            device::DeviceLimits,
            resources::Format,
        },
    },
    memory::{GuestAddress, PageRange},
    process::{D3D8Config32, D3D8Error32, EngineInstance, HostError},
};

fn config() -> D3D8Config32 {
    let display = DisplayMode32 {
        width: 640,
        height: 480,
        refresh_rate: 60,
        format: Format::X8R8G8B8,
    };
    let mut device = DeviceLimits::default();
    device.graphics.resources = GraphicsLimits::MAX_RESOURCES;
    D3D8Config32 {
        objects: PageRange::new(GuestAddress(0x1000_0000), 16).unwrap(),
        locks: PageRange::new(GuestAddress(0x2000_0000), 16).unwrap(),
        max_objects: 64,
        max_devices: 1,
        max_locks: 4,
        adapter: AdapterProfile32::new(display, vec![display]).unwrap(),
        device,
    }
}

#[test]
fn invalid_device_limits_are_rejected_before_configuration_is_published() {
    let mut engine = EngineInstance::new(8, 7).unwrap();
    let valid = config();
    let mut invalid = valid.clone();
    invalid.device.graphics.resources = 16384;
    let arena = engine.arena().to_vec();
    let version = engine.memory().unwrap().transaction_version();
    assert_eq!(
        engine.configure_d3d8(invalid),
        Err(HostError::D3D8(D3D8Error32::InvalidConfiguration))
    );
    assert!(engine.d3d8.is_none());
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    engine.configure_d3d8(valid).unwrap();
    assert_eq!(
        engine
            .d3d8
            .as_ref()
            .unwrap()
            .config
            .device
            .graphics
            .resources,
        GraphicsLimits::MAX_RESOURCES
    );
}

#[test]
fn configuration_uses_d3d8_storage_and_fragment_minima() {
    let mut engine = EngineInstance::new(8, 7).unwrap();
    let mut small = config();
    small.device.graphics.resource_bytes = 1;
    small.device.commands.batch_bytes = (crate::abi::graphics::d3d8::BATCH_HEADER_SIZE
        + crate::abi::graphics::d3d8::FRAGMENT_HEADER_SIZE
        + 1) as u32;
    small.device.commands.pending_bytes = 1;
    small.device.shaders.bytes = 1;
    engine.configure_d3d8(small).unwrap();
}

#[test]
fn configuration_rejects_invalid_command_shader_and_device_owner_limits() {
    let invalid_limits: [fn(&mut DeviceLimits); 3] = [
        |limits| limits.commands.pending_bytes = 0,
        |limits| limits.shaders.objects = 0,
        |limits| limits.readbacks = 0,
    ];
    for invalidate in invalid_limits {
        let mut engine = EngineInstance::new(8, 7).unwrap();
        let mut invalid = config();
        invalidate(&mut invalid.device);
        assert_eq!(
            engine.configure_d3d8(invalid),
            Err(HostError::D3D8(D3D8Error32::InvalidConfiguration))
        );
        assert!(engine.d3d8.is_none());
    }
}

#[test]
fn failed_reconfiguration_preserves_the_existing_device_and_its_resources() {
    let (runtime, memory, device) = super::support::runtime();
    let mut engine = EngineInstance::new(8, 7).unwrap();
    let previous = runtime.config.clone();
    let resources: Vec<_> = runtime.device(device).unwrap().resources().ids().collect();
    let bytes = runtime.device(device).unwrap().resources().resource_bytes();
    engine.memory = Some(memory);
    engine.d3d8 = Some(runtime);
    let mut invalid = config();
    invalid.device.graphics.resources = 16384;
    let arena = engine.arena().to_vec();
    let version = engine.memory().unwrap().transaction_version();
    assert_eq!(
        engine.configure_d3d8(invalid),
        Err(HostError::D3D8(D3D8Error32::AlreadyConfigured))
    );
    let runtime = engine.d3d8.as_ref().unwrap();
    assert_eq!(runtime.config.objects, previous.objects);
    assert_eq!(runtime.config.locks, previous.locks);
    assert_eq!(runtime.config.device, previous.device);
    assert_eq!(
        runtime
            .device(device)
            .unwrap()
            .resources()
            .ids()
            .collect::<Vec<_>>(),
        resources
    );
    assert_eq!(
        runtime.device(device).unwrap().resources().resource_bytes(),
        bytes
    );
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
}
