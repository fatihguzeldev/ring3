use super::input;
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
    memory::{GuestAddress, PageRange},
    process::{D3D8Config32, HostError, wasm::mutate},
};

pub(crate) fn configure_d3d8(length: u32) -> u32 {
    mutate(|instance| {
        let config = decode(input(instance, length)?)?;
        instance.configure_d3d8(config)
    })
}

fn decode(bytes: &[u8]) -> Result<D3D8Config32, HostError> {
    if bytes.len() < 128
        || &bytes[..4] != b"R3C8"
        || u16::from_le_bytes(bytes[4..6].try_into().unwrap()) != 1
        || u16::from_le_bytes(bytes[6..8].try_into().unwrap()) != 8
    {
        return Err(HostError::InvalidRequest);
    }
    let words: [u32; 32] = std::array::from_fn(|index| {
        u32::from_le_bytes(bytes[index * 4..index * 4 + 4].try_into().unwrap())
    });
    if words[2] as usize != bytes.len()
        || words[3] != 0
        || words[29..].iter().any(|word| *word != 0)
        || !(1..=256).contains(&words[4])
        || bytes.len() != 128 + words[4] as usize * 16
    {
        return Err(HostError::InvalidRequest);
    }
    let mut modes = Vec::new();
    modes
        .try_reserve_exact(words[4] as usize)
        .map_err(|_| HostError::Infrastructure)?;
    for chunk in bytes[128..].chunks_exact(16) {
        let mode: [u32; 4] = std::array::from_fn(|index| {
            u32::from_le_bytes(chunk[index * 4..index * 4 + 4].try_into().unwrap())
        });
        modes.push(DisplayMode32 {
            width: mode[0],
            height: mode[1],
            refresh_rate: mode[2],
            format: format(mode[3])?,
        });
    }
    let mut adapter = AdapterProfile32::new(
        DisplayMode32 {
            width: words[20],
            height: words[21],
            format: format(words[22])?,
            refresh_rate: words[23],
        },
        modes,
    )
    .map_err(|_| HostError::InvalidRequest)?;
    adapter.max_texture_extent = words[24];
    adapter.max_volume_extent = words[25];
    adapter.max_anisotropy = words[26];
    adapter.multisamples = words[27];
    adapter.presentation_intervals = words[28];
    adapter.validate().map_err(|_| HostError::InvalidRequest)?;
    Ok(D3D8Config32 {
        objects: PageRange::new(GuestAddress(words[5]), words[6]).map_err(HostError::Memory)?,
        locks: PageRange::new(GuestAddress(words[7]), words[8]).map_err(HostError::Memory)?,
        max_objects: words[9],
        max_devices: words[10],
        max_locks: words[11],
        adapter,
        device: DeviceLimits {
            graphics: GraphicsLimits {
                resources: words[12],
                resource_bytes: words[13],
                batch_bytes: words[14],
            },
            commands: CommandLimits {
                batch_bytes: words[14],
                pending_bytes: words[19],
            },
            shaders: ShaderLimits {
                objects: words[15],
                bytes: words[16],
            },
            state_blocks: words[17],
            readbacks: words[18],
        },
    })
}

fn format(value: u32) -> Result<Format, HostError> {
    Format::try_from(value).map_err(|_| HostError::InvalidRequest)
}
