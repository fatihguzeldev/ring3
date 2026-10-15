use super::{
    super::{
        D3D8Runtime32, DispatchError32,
        memory::check_output,
        objects::{Object32, ObjectKind32, Resource32},
    },
    error,
};
use crate::{
    graphics::d3d8::{
        api::{HResult, InterfaceKind},
        resources::{
            BufferDesc, BufferKind, Format, Pool, ResourceDesc, SurfaceDesc, TextureDesc,
            TextureKind, Usage,
        },
    },
    memory::{AddressSpace, GuestAddress},
    process::windows::objects::{GuestComCreation32, InterfaceLayout32},
    windows::com::ReferenceState32,
};

impl D3D8Runtime32 {
    pub(in crate::process::windows::d3d8) fn device_resource_call(
        &mut self,
        memory: &mut AddressSpace,
        pointer: GuestAddress,
        slot: u8,
        args: &[u32],
    ) -> Result<Option<u32>, DispatchError32> {
        if !(20..=27).contains(&slot) {
            return Ok(None);
        }
        let (desc, interface, output) = match slot {
            20 => (
                ResourceDesc::Texture(TextureDesc {
                    kind: TextureKind::Texture2D,
                    width: args[1],
                    height: args[2],
                    depth: 1,
                    levels: args[3],
                    usage: Usage(args[4]),
                    format: format(args[5])?,
                    pool: pool(args[6])?,
                }),
                InterfaceKind::Texture,
                args[7],
            ),
            21 => (
                ResourceDesc::Texture(TextureDesc {
                    kind: TextureKind::Volume,
                    width: args[1],
                    height: args[2],
                    depth: args[3],
                    levels: args[4],
                    usage: Usage(args[5]),
                    format: format(args[6])?,
                    pool: pool(args[7])?,
                }),
                InterfaceKind::VolumeTexture,
                args[8],
            ),
            22 => (
                ResourceDesc::Texture(TextureDesc {
                    kind: TextureKind::Cube,
                    width: args[1],
                    height: args[1],
                    depth: 1,
                    levels: args[2],
                    usage: Usage(args[3]),
                    format: format(args[4])?,
                    pool: pool(args[5])?,
                }),
                InterfaceKind::CubeTexture,
                args[6],
            ),
            23 => (
                ResourceDesc::Buffer(BufferDesc {
                    length: args[1],
                    usage: Usage(args[2]),
                    kind: BufferKind::Vertex { fvf: args[3] },
                    pool: pool(args[4])?,
                }),
                InterfaceKind::VertexBuffer,
                args[5],
            ),
            24 => (
                ResourceDesc::Buffer(BufferDesc {
                    length: args[1],
                    usage: Usage(args[2]),
                    kind: BufferKind::Index {
                        format: format(args[3])?,
                    },
                    pool: pool(args[4])?,
                }),
                InterfaceKind::IndexBuffer,
                args[5],
            ),
            25 => (
                ResourceDesc::Surface(SurfaceDesc {
                    width: args[1],
                    height: args[2],
                    format: format(args[3])?,
                    multisample: args[4],
                    lockable: args[5] != 0,
                    usage: Usage(Usage::RENDER_TARGET),
                    pool: Pool::Default,
                }),
                InterfaceKind::Surface,
                args[6],
            ),
            26 => {
                let format = format(args[3])?;
                (
                    ResourceDesc::Surface(SurfaceDesc {
                        width: args[1],
                        height: args[2],
                        format,
                        multisample: args[4],
                        lockable: format == Format::D16Lockable,
                        usage: Usage(Usage::DEPTH_STENCIL),
                        pool: Pool::Default,
                    }),
                    InterfaceKind::Surface,
                    args[5],
                )
            }
            27 => (
                ResourceDesc::Surface(SurfaceDesc {
                    width: args[1],
                    height: args[2],
                    format: format(args[3])?,
                    multisample: 0,
                    lockable: true,
                    usage: Usage(0),
                    pool: Pool::SystemMemory,
                }),
                InterfaceKind::Surface,
                args[4],
            ),
            _ => unreachable!(),
        };
        check_output(memory, output, 4)?;
        match desc {
            ResourceDesc::Texture(texture) if texture.pool != Pool::Scratch => {
                self.config.adapter.check_format(
                    texture.usage,
                    desc.resource_type(),
                    texture.format,
                )?;
                let maximum = if texture.kind == TextureKind::Volume {
                    self.config.adapter.max_volume_extent
                } else {
                    self.config.adapter.max_texture_extent
                };
                if texture.width > maximum || texture.height > maximum || texture.depth > maximum {
                    return Err(HResult::INVALID_CALL.into());
                }
            }
            ResourceDesc::Surface(surface) if surface.usage.0 != 0 => {
                self.config
                    .adapter
                    .check_format(surface.usage, 1, surface.format)?;
                self.config
                    .adapter
                    .check_multisample(surface.format, surface.multisample)?;
            }
            ResourceDesc::Buffer(buffer)
                if buffer.usage.0 & (Usage::RT_PATCHES | Usage::NP_PATCHES) != 0 =>
            {
                return Err(HResult::NOT_AVAILABLE.into());
            }
            _ => {}
        }
        let record = self.vacant_record()?;
        let prepared = self.device(pointer)?.prepare_resource(desc)?;
        self.device(pointer)?.can_commit_resource(&prepared)?;
        let id = prepared.id();
        let identity = self.com.create_with_lifetime(
            memory,
            record as u64,
            &[InterfaceLayout32 {
                iid: interface.iid(),
                methods: &self.methods[interface as usize - 1],
            }],
            GuestComCreation32 {
                output: Some((GuestAddress(output), interface.iid())),
                writes: &[],
                initial: ReferenceState32 {
                    references: 1,
                    holds: 0,
                },
                retain: &[pointer],
            },
        )?;
        self.device_mut(pointer)?.commit_resource(prepared)?;
        self.records[record] = Some(Object32 {
            pointer: identity.identity_pointer,
            interface,
            private_unknowns: Vec::new(),
            kind: ObjectKind32::Resource(Resource32 {
                device: pointer,
                resource: id,
            }),
        });
        Ok(Some(0))
    }
}

fn format(value: u32) -> Result<Format, DispatchError32> {
    Format::try_from(value).map_err(error)
}
fn pool(value: u32) -> Result<Pool, DispatchError32> {
    Pool::try_from(value).map_err(error)
}
