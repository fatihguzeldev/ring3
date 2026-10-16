use crate::{
    graphics::{
        GraphicsLimits,
        d3d8::{
            api::{AdapterProfile32, DisplayMode32},
            commands::CommandLimits,
            device::DeviceLimits,
            resources::Format,
            shader::registry::ShaderLimits,
        },
    },
    loader::{ModuleLimits32, ParserLimits32},
    memory::{GuestAddress, PageRange},
    process::{D3D8Config32, EngineInstance, ModuleOperation32},
    windows::user32::User32Limits32,
};

fn graphics_image() -> Vec<u8> {
    let mut bytes = crate::process::windows::process::test_image_for_startup();
    bytes.resize(0x6200, 0);
    bytes[0x86..0x88].copy_from_slice(&2u16.to_le_bytes());
    let word = |bytes: &mut [u8], at: usize, value: u32| {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    };
    word(&mut bytes, 0x98 + 56, 0x8000);
    for (index, rva, raw, length, flags) in [
        (0, 0x1000, 0x200, 0x2000, 0x60000020),
        (1, 0x4000, 0x2200, 0x4000, 0xc0000040),
    ] {
        for (offset, value) in [(8, length), (12, rva), (16, length), (20, raw), (36, flags)] {
            word(&mut bytes, 0x178 + index * 40 + offset, value);
        }
    }
    word(&mut bytes, 0x98 + 104, 0x4000);
    word(&mut bytes, 0x98 + 108, 80);
    let raw = |rva: usize| 0x2200 + rva - 0x4000;
    let mut module_name = 0x4100;
    let mut hint = 0x4400;
    let mut slot = 0;
    for (index, (module, symbols)) in [
        ("kernel32.dll", &["ExitProcess"][..]),
        (
            "user32.dll",
            &["RegisterClassA", "CreateWindowExA", "DestroyWindow"][..],
        ),
        ("d3d8.dll", &["Direct3DCreate8"][..]),
    ]
    .into_iter()
    .enumerate()
    {
        word(&mut bytes, raw(0x4000 + index * 20), 0x4200 + slot * 4);
        word(
            &mut bytes,
            raw(0x4000 + index * 20 + 12),
            module_name as u32,
        );
        word(&mut bytes, raw(0x4000 + index * 20 + 16), 0x4300 + slot * 4);
        bytes[raw(module_name)..raw(module_name) + module.len()].copy_from_slice(module.as_bytes());
        module_name += module.len() + 1;
        for symbol in symbols {
            bytes[raw(hint) + 2..raw(hint) + 2 + symbol.len()].copy_from_slice(symbol.as_bytes());
            word(&mut bytes, raw(0x4200 + slot as usize * 4), hint as u32);
            word(&mut bytes, raw(0x4300 + slot as usize * 4), hint as u32);
            hint = (hint + symbol.len() + 4) & !1;
            slot += 1;
        }
        slot += 1;
    }
    bytes
}

fn start_graphics_image(image: &[u8]) {
    let mut engine = EngineInstance::new(1024, 0x9192939451525354).unwrap();
    engine
        .configure_module_limits(ModuleLimits32 {
            modules: 16,
            source_bytes: 0x100000,
            forwarder_depth: 16,
            parser: ParserLimits32 {
                metadata_entries: 65536,
                string_bytes: 0x100000,
            },
        })
        .unwrap();
    engine.register_kernel32_module(0x70000000).unwrap();
    engine
        .configure_user32(
            User32Limits32 {
                classes: 16,
                windows: 16,
                messages: 64,
                continuations: 16,
            },
            PageRange::new(GuestAddress(0x30000000), 16).unwrap(),
        )
        .unwrap();
    engine.register_user32_module(0x72000000).unwrap();
    let display = DisplayMode32 {
        width: 64,
        height: 64,
        refresh_rate: 60,
        format: Format::X8R8G8B8,
    };
    let mut adapter = AdapterProfile32::new(display, vec![display]).unwrap();
    adapter.max_texture_extent = 4096;
    adapter.max_volume_extent = 256;
    engine
        .configure_d3d8(D3D8Config32 {
            objects: PageRange::new(GuestAddress(0x10000000), 256).unwrap(),
            locks: PageRange::new(GuestAddress(0x20000000), 128).unwrap(),
            max_objects: 128,
            max_devices: 4,
            max_locks: 16,
            adapter,
            device: DeviceLimits {
                graphics: GraphicsLimits {
                    resources: 1024,
                    resource_bytes: 256 * 1024 * 1024,
                    batch_bytes: 256,
                },
                commands: CommandLimits {
                    batch_bytes: 256,
                    pending_bytes: 0x100000,
                },
                shaders: ShaderLimits {
                    objects: 64,
                    bytes: 0x100000,
                },
                state_blocks: 32,
                patches: 256,
                readbacks: 16,
            },
        })
        .unwrap();
    engine.register_d3d8_module(0x71000000).unwrap();
    engine
        .register_guest_module("d3d8-authored.exe", image)
        .unwrap();
    assert_eq!(
        engine
            .start_module_process("d3d8-authored.exe", None, Default::default())
            .unwrap(),
        ModuleOperation32::Loaded { handle: 0x400000 },
    );
}

#[test]
fn authored_graphics_configuration_starts_a_real_windows_process() {
    start_graphics_image(&graphics_image());
}
