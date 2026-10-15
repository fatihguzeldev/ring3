use super::*;
use crate::graphics::GraphicsLimits;

fn store() -> ResourceStore {
    ResourceStore::new(17, GraphicsLimits::default()).unwrap()
}

fn texture(kind: TextureKind, format: Format, pool: Pool) -> ResourceDesc {
    ResourceDesc::Texture(TextureDesc {
        kind,
        width: 8,
        height: 8,
        depth: if kind == TextureKind::Volume { 4 } else { 1 },
        levels: 0,
        format,
        usage: Usage(0),
        pool,
    })
}

mod copy;
mod layout;
mod lifetime;
mod locks;
mod preparation;
