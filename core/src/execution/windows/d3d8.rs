use std::collections::BTreeMap;

use super::super::Access;
use super::{
    API_BASE, GuestMemory, MemoryError, PAGE_SIZE, Permissions,
    desktop::{DESKTOP, Desktop},
    guest,
};

const OBJECT_BASE: u32 = API_BASE + 4096;
const ROOT: u32 = OBJECT_BASE;
const DEVICE: u32 = OBJECT_BASE + 4;
const DEPTH_SURFACE: u32 = OBJECT_BASE + 8;
const ROOT_TABLE: u32 = OBJECT_BASE + 0x100;
const DEVICE_TABLE: u32 = OBJECT_BASE + 0x200;
const DEPTH_SURFACE_TABLE: u32 = OBJECT_BASE + 0x500;
const TEXTURE_SURFACE_TABLE: u32 = OBJECT_BASE + 0x600;
const VERTEX_BUFFER_TABLE: u32 = OBJECT_BASE + 0x700;
const INDEX_BUFFER_TABLE: u32 = OBJECT_BASE + 0x800;
const INVALID_CALL: u32 = 0x8876_086c;
const NOT_AVAILABLE: u32 = 0x8876_086a;
const UNSUPPORTED_COLOR_OPERATION: u32 = 0x8876_0819;
const ADAPTER_IDENTIFIER_SIZE: usize = 1068;
const ADAPTER_DRIVER: &[u8] = b"ring3\0";
const ADAPTER_DESCRIPTION: &[u8] = b"Ring3 Virtual Display Adapter\0";
const DEVICE_CAPS_SIZE: usize = 212;
const CAPS2_CAN_RENDER_WINDOWED: u32 = 0x0008_0000;
const DEVCAPS_DRAWPRIM_TLVERTEX: u32 = 0x0000_0400;
const MAX_PIXELS: u64 = 1_048_576;
const MAX_RECTS: u32 = 64;
const TEXTURE_TABLE: u32 = OBJECT_BASE + 0x400;
const TEXTURE_START: u64 = 0x7200_0000;
const TEXTURE_END: u64 = 0x7f00_0000;
const MIB: u64 = 1024 * 1024;
const OUT_OF_VIDEO_MEMORY: u32 = 0x8876_017c;
const IDENTITY_MATRIX: [u32; 16] = [
    1_f32.to_bits(),
    0,
    0,
    0,
    0,
    1_f32.to_bits(),
    0,
    0,
    0,
    0,
    1_f32.to_bits(),
    0,
    0,
    0,
    0,
    1_f32.to_bits(),
];

mod primitives;

/// an owned top-to-bottom rgba8 snapshot of the most recent presentation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Copy)]
pub(super) enum Call {
    Create,
    AdapterCount,
    AdapterIdentifier,
    AdapterModeCount,
    AdapterMode,
    CurrentDisplayMode,
    CheckDeviceType,
    CheckDeviceFormat,
    CheckDepthStencilMatch,
    CheckMultiSampleType,
    DeviceCaps,
    CreateDevice,
    CreateTexture,
    CreateVertexBuffer,
    CreateIndexBuffer,
    Clear,
    Present,
    RootAddRef,
    RootRelease,
    DeviceAddRef,
    DeviceRelease,
    TextureAddRef,
    TextureRelease,
    VertexBufferAddRef,
    VertexBufferRelease,
    VertexBufferLock,
    VertexBufferUnlock,
    IndexBufferAddRef,
    IndexBufferRelease,
    IndexBufferLock,
    IndexBufferUnlock,
    TexturePreLoad,
    SetTexture,
    GetTextureStageState,
    SetTextureStageState,
    ValidateDevice,
    TestCooperativeLevel,
    BeginScene,
    EndScene,
    TextureLevelCount,
    TextureLevelDesc,
    TextureLockRect,
    TextureUnlockRect,
    TextureGetSurface,
    TextureSurfaceAddRef,
    TextureSurfaceRelease,
    TextureSurfaceDesc,
    TextureSurfaceLockRect,
    TextureSurfaceUnlockRect,
    SetVertexShader,
    SetPixelShader,
    SetIndices,
    GetIndices,
    SetStreamSource,
    GetStreamSource,
    SetViewport,
    SetTransform,
    GetTransform,
    SetRenderState,
    GetDepthStencilSurface,
    DepthSurfaceAddRef,
    DepthSurfaceRelease,
    AvailableTextureMemory,
    DrawPrimitiveUp,
    DrawIndexedPrimitive,
}

impl Call {
    pub(super) fn at(offset: u32) -> Option<Self> {
        Some(match offset {
            12 => Self::Create,
            0x40 => Self::CreateDevice,
            0x44 => Self::Clear,
            0x48 => Self::Present,
            0x50 => Self::RootAddRef,
            0x54 => Self::RootRelease,
            0x58 => Self::DeviceAddRef,
            0x5c => Self::DeviceRelease,
            0x400 => Self::CreateTexture,
            0x4f8 => Self::CreateVertexBuffer,
            0x4fc => Self::VertexBufferAddRef,
            0x500 => Self::VertexBufferRelease,
            0x504 => Self::VertexBufferLock,
            0x508 => Self::VertexBufferUnlock,
            0x50c => Self::CreateIndexBuffer,
            0x510 => Self::IndexBufferAddRef,
            0x514 => Self::IndexBufferRelease,
            0x518 => Self::IndexBufferLock,
            0x51c => Self::IndexBufferUnlock,
            0x404 => Self::TextureAddRef,
            0x408 => Self::TextureRelease,
            0x4d0 => Self::TexturePreLoad,
            0x4d4 => Self::SetTexture,
            0x4d8 => Self::GetTextureStageState,
            0x4dc => Self::SetTextureStageState,
            0x4e0 => Self::ValidateDevice,
            0x4e4 => Self::TestCooperativeLevel,
            0x4e8 => Self::BeginScene,
            0x4ec => Self::EndScene,
            0x40c => Self::TextureLevelCount,
            0x410 => Self::TextureLevelDesc,
            0x414 => Self::TextureLockRect,
            0x418 => Self::TextureUnlockRect,
            0x4b8 => Self::TextureGetSurface,
            0x4bc => Self::TextureSurfaceAddRef,
            0x4c0 => Self::TextureSurfaceRelease,
            0x4c4 => Self::TextureSurfaceDesc,
            0x4c8 => Self::TextureSurfaceLockRect,
            0x4cc => Self::TextureSurfaceUnlockRect,
            0x41c => Self::SetVertexShader,
            0x520 => Self::SetPixelShader,
            0x524 => Self::SetIndices,
            0x528 => Self::GetIndices,
            0x52c => Self::SetStreamSource,
            0x530 => Self::GetStreamSource,
            0x420 => Self::DrawPrimitiveUp,
            0x534 => Self::DrawIndexedPrimitive,
            0x498 => Self::SetViewport,
            0x4f0 => Self::SetTransform,
            0x4f4 => Self::GetTransform,
            0x49c => Self::SetRenderState,
            0x4a0 => Self::GetDepthStencilSurface,
            0x4a4 => Self::DepthSurfaceAddRef,
            0x4a8 => Self::DepthSurfaceRelease,
            0x4ac => Self::AvailableTextureMemory,
            0x2d0 => Self::AdapterCount,
            0x2d4 => Self::AdapterIdentifier,
            0x2d8 => Self::DeviceCaps,
            0x2dc => Self::AdapterModeCount,
            0x2e0 => Self::AdapterMode,
            0x2f0 => Self::CurrentDisplayMode,
            0x2e4 => Self::CheckDeviceType,
            0x2e8 => Self::CheckDeviceFormat,
            0x2f4 => Self::CheckDepthStencilMatch,
            0x2ec => Self::CheckMultiSampleType,
            _ => return None,
        })
    }

    pub(super) fn arguments(self) -> usize {
        match self {
            Self::CreateDevice | Self::Clear | Self::CheckDeviceFormat => 7,
            Self::CreateTexture => 8,
            Self::Present
            | Self::TextureLockRect
            | Self::DrawPrimitiveUp
            | Self::VertexBufferLock
            | Self::IndexBufferLock => 5,
            Self::TextureLevelDesc
            | Self::TextureGetSurface
            | Self::CurrentDisplayMode
            | Self::SetRenderState
            | Self::SetTransform
            | Self::GetTransform
            | Self::SetTexture
            | Self::SetIndices
            | Self::GetIndices => 3,
            Self::AdapterIdentifier
            | Self::AdapterMode
            | Self::DeviceCaps
            | Self::TextureSurfaceLockRect
            | Self::GetTextureStageState
            | Self::SetTextureStageState
            | Self::SetStreamSource
            | Self::GetStreamSource => 4,
            Self::AdapterModeCount
            | Self::TextureUnlockRect
            | Self::SetVertexShader
            | Self::SetPixelShader
            | Self::SetViewport
            | Self::GetDepthStencilSurface
            | Self::TextureSurfaceDesc
            | Self::ValidateDevice => 2,
            Self::CreateVertexBuffer
            | Self::CreateIndexBuffer
            | Self::DrawIndexedPrimitive
            | Self::CheckDeviceType
            | Self::CheckMultiSampleType
            | Self::CheckDepthStencilMatch => 6,
            _ => 1,
        }
    }
}

#[derive(Default)]
pub(super) struct Graphics {
    root_refs: u32,
    device_refs: u32,
    back: Option<Frame>,
    spare: Option<Frame>,
    depth: Option<Vec<u16>>,
    depth_surface_refs: u32,
    z_enabled: bool,
    depth_policy: DepthPolicy,
    scene_open: bool,
    viewport: Option<Viewport>,
    transforms: BTreeMap<u32, [u32; 16]>,
    front: Option<Frame>,
    textures: BTreeMap<u32, Texture>,
    vertex_buffers: BTreeMap<u32, VertexBuffer>,
    index_buffers: BTreeMap<u32, IndexBuffer>,
    indices: Option<(u32, u32)>,
    stream: Option<(u32, u32)>,
    texture_stages: [u32; 8],
    color_arg0: [u32; 8],
    vertex_fvf: u32,
}

#[derive(Clone, Copy)]
struct DepthPolicy {
    write_enabled: bool,
    function: u32,
}

impl Default for DepthPolicy {
    fn default() -> Self {
        Self {
            write_enabled: true,
            function: 4,
        }
    }
}

impl DepthPolicy {
    fn passes(self, incoming: u16, stored: u16) -> bool {
        match self.function {
            1 => false,
            2 => incoming < stored,
            3 => incoming == stored,
            4 => incoming <= stored,
            5 => incoming > stored,
            6 => incoming != stored,
            7 => incoming >= stored,
            8 => true,
            _ => unreachable!("validated depth comparison"),
        }
    }
}

struct Texture {
    refs: u32,
    length: u64,
    format: u32,
    pool: u32,
    levels: Vec<TextureLevel>,
}

struct VertexBuffer {
    refs: u32,
    length: u64,
    lock: BufferLock,
}

struct IndexBuffer {
    refs: u32,
    length: u64,
    format: u32,
    lock: BufferLock,
}

struct BufferLock {
    byte_length: u32,
    usage: u32,
    count: u32,
}

impl BufferLock {
    fn lock(
        &mut self,
        address: u32,
        args: &[u32],
        memory: &mut GuestMemory,
    ) -> Result<u32, MemoryError> {
        let [_, offset, size, output, flags] = <[u32; 5]>::try_from(args).expect("d3d8 call arity");
        let discard = flags & 0x2000 != 0;
        let no_overwrite = flags & 0x1000 != 0;
        let read_only = flags & 0x0010 != 0;
        let size = if size == 0 && offset == 0 {
            self.byte_length
        } else {
            size
        };
        if offset >= self.byte_length
            || size == 0
            || offset
                .checked_add(size)
                .is_none_or(|end| end > self.byte_length)
            || flags & !(0x0010 | 0x0800 | 0x1000 | 0x2000) != 0
            || (discard && (no_overwrite || read_only || offset != 0 || size != self.byte_length))
            || ((discard || no_overwrite) && self.usage & 0x0200 == 0)
            || (read_only && self.usage & 0x0008 != 0)
            || self.count == u32::MAX
        {
            return Ok(INVALID_CALL);
        }
        guest::check(memory, output, 4, Access::Write)?;
        let data = address + u32::try_from(PAGE_SIZE).expect("guest page fits u32") + offset;
        guest::write_word(memory, output, data)?;
        self.count += 1;
        Ok(0)
    }

    fn unlock(&mut self) -> u32 {
        if self.count == 0 {
            return INVALID_CALL;
        }
        self.count -= 1;
        0
    }
}

struct TextureLevel {
    width: u32,
    height: u32,
    offset: u32,
    locked: bool,
    surface_refs: u32,
}

impl Texture {
    fn total_refs(&self) -> u32 {
        self.levels.iter().fold(self.refs, |refs, level| {
            refs.saturating_add(level.surface_refs)
        })
    }
}

fn valid_transform_state(state: u32) -> bool {
    matches!(state, 2 | 3 | 16..=23 | 256..=511)
}

#[derive(Clone, Copy)]
pub(super) struct Viewport {
    left: usize,
    top: usize,
    right: usize,
    bottom: usize,
}

impl Graphics {
    pub(super) fn initialize(memory: &mut GuestMemory) -> Result<(), MemoryError> {
        memory.map_zeroed(u64::from(OBJECT_BASE), PAGE_SIZE, Permissions::READ_WRITE)?;
        guest::write_word(memory, ROOT, ROOT_TABLE)?;
        guest::write_word(memory, DEVICE, DEVICE_TABLE)?;
        guest::write_word(memory, DEPTH_SURFACE, DEPTH_SURFACE_TABLE)?;
        for (table, count) in [
            (ROOT_TABLE, 16),
            (DEVICE_TABLE, 97),
            (DEPTH_SURFACE_TABLE, 13),
        ] {
            for index in 0..count {
                guest::write_word(memory, table + index * 4, API_BASE + 0xffc)?;
            }
        }
        for index in 0..19 {
            guest::write_word(memory, TEXTURE_TABLE + index * 4, API_BASE + 0xffc)?;
        }
        for index in 0..13 {
            guest::write_word(memory, TEXTURE_SURFACE_TABLE + index * 4, API_BASE + 0xffc)?;
        }
        for index in 0..14 {
            guest::write_word(memory, VERTEX_BUFFER_TABLE + index * 4, API_BASE + 0xffc)?;
            guest::write_word(memory, INDEX_BUFFER_TABLE + index * 4, API_BASE + 0xffc)?;
        }
        for (table, index, offset) in [
            (ROOT_TABLE, 1, 0x50),
            (ROOT_TABLE, 2, 0x54),
            (ROOT_TABLE, 4, 0x2d0),
            (ROOT_TABLE, 5, 0x2d4),
            (ROOT_TABLE, 6, 0x2dc),
            (ROOT_TABLE, 7, 0x2e0),
            (ROOT_TABLE, 8, 0x2f0),
            (ROOT_TABLE, 9, 0x2e4),
            (ROOT_TABLE, 10, 0x2e8),
            (ROOT_TABLE, 11, 0x2ec),
            (ROOT_TABLE, 12, 0x2f4),
            (ROOT_TABLE, 13, 0x2d8),
            (ROOT_TABLE, 15, 0x40),
            (DEVICE_TABLE, 1, 0x58),
            (DEVICE_TABLE, 2, 0x5c),
            (DEVICE_TABLE, 15, 0x48),
            (DEVICE_TABLE, 36, 0x44),
            (DEVICE_TABLE, 40, 0x498),
            (DEVICE_TABLE, 37, 0x4f0),
            (DEVICE_TABLE, 38, 0x4f4),
            (DEVICE_TABLE, 50, 0x49c),
            (DEVICE_TABLE, 33, 0x4a0),
            (DEVICE_TABLE, 4, 0x4ac),
            (DEVICE_TABLE, 61, 0x4d4),
            (DEVICE_TABLE, 62, 0x4d8),
            (DEVICE_TABLE, 63, 0x4dc),
            (DEVICE_TABLE, 64, 0x4e0),
            (DEVICE_TABLE, 3, 0x4e4),
            (DEVICE_TABLE, 34, 0x4e8),
            (DEVICE_TABLE, 35, 0x4ec),
            (DEPTH_SURFACE_TABLE, 1, 0x4a4),
            (DEPTH_SURFACE_TABLE, 2, 0x4a8),
            (DEVICE_TABLE, 20, 0x400),
            (DEVICE_TABLE, 23, 0x4f8),
            (DEVICE_TABLE, 24, 0x50c),
            (DEVICE_TABLE, 72, 0x420),
            (DEVICE_TABLE, 71, 0x534),
            (DEVICE_TABLE, 76, 0x41c),
            (DEVICE_TABLE, 88, 0x520),
            (DEVICE_TABLE, 85, 0x524),
            (DEVICE_TABLE, 86, 0x528),
            (DEVICE_TABLE, 83, 0x52c),
            (DEVICE_TABLE, 84, 0x530),
            (TEXTURE_TABLE, 1, 0x404),
            (TEXTURE_TABLE, 2, 0x408),
            (TEXTURE_TABLE, 9, 0x4d0),
            (TEXTURE_TABLE, 13, 0x40c),
            (TEXTURE_TABLE, 14, 0x410),
            (TEXTURE_TABLE, 16, 0x414),
            (TEXTURE_TABLE, 17, 0x418),
            (TEXTURE_TABLE, 15, 0x4b8),
            (TEXTURE_SURFACE_TABLE, 1, 0x4bc),
            (TEXTURE_SURFACE_TABLE, 2, 0x4c0),
            (TEXTURE_SURFACE_TABLE, 8, 0x4c4),
            (TEXTURE_SURFACE_TABLE, 9, 0x4c8),
            (TEXTURE_SURFACE_TABLE, 10, 0x4cc),
            (VERTEX_BUFFER_TABLE, 1, 0x4fc),
            (VERTEX_BUFFER_TABLE, 2, 0x500),
            (VERTEX_BUFFER_TABLE, 11, 0x504),
            (VERTEX_BUFFER_TABLE, 12, 0x508),
            (INDEX_BUFFER_TABLE, 1, 0x510),
            (INDEX_BUFFER_TABLE, 2, 0x514),
            (INDEX_BUFFER_TABLE, 11, 0x518),
            (INDEX_BUFFER_TABLE, 12, 0x51c),
        ] {
            guest::write_word(memory, table + index * 4, API_BASE + offset)?;
        }
        memory.protect(u64::from(OBJECT_BASE), PAGE_SIZE, Permissions::READ)
    }

    pub(super) fn take_frame(&mut self) -> Option<Frame> {
        self.front.take()
    }

    pub(super) fn dispatch(
        &mut self,
        call: Call,
        args: &[u32],
        memory: &mut GuestMemory,
        desktop: &Desktop,
    ) -> Result<u32, MemoryError> {
        Ok(match call {
            Call::Create => {
                if !matches!(args[0], 120 | 220) || self.root_refs != 0 {
                    0
                } else {
                    self.root_refs = 1;
                    ROOT
                }
            }
            Call::AdapterCount => u32::from(args[0] == ROOT && self.root_refs != 0),
            Call::AdapterIdentifier => return self.adapter_identifier(args, memory),
            Call::AdapterModeCount => {
                u32::from(args[0] == ROOT && self.root_refs != 0 && args[1] == 0)
            }
            Call::AdapterMode => return self.adapter_mode(args, memory),
            Call::CurrentDisplayMode => {
                return self.adapter_mode(&[args[0], args[1], 0, args[2]], memory);
            }
            Call::CheckDeviceType => self.check_device_type(args),
            Call::CheckDeviceFormat => self.check_device_format(args),
            Call::CheckDepthStencilMatch => self.check_depth_stencil_match(args),
            Call::CheckMultiSampleType => self.check_multisample_type(args),
            Call::DeviceCaps => return self.device_caps(args, memory),
            Call::CreateDevice => return self.create_device(args, memory, desktop),
            Call::CreateTexture => return self.create_texture(args, memory),
            Call::CreateVertexBuffer => return self.create_vertex_buffer(args, memory),
            Call::CreateIndexBuffer => return self.create_index_buffer(args, memory),
            Call::TextureLevelDesc => return self.texture_level_desc(args, memory),
            Call::TextureLockRect => return self.texture_lock_rect(args, memory),
            Call::TextureUnlockRect => self.texture_unlock_rect(args),
            Call::TextureGetSurface => return self.texture_get_surface(args, memory),
            Call::TextureSurfaceAddRef | Call::TextureSurfaceRelease => {
                return self.texture_surface_ref(call, args[0], memory);
            }
            Call::TextureSurfaceDesc => return self.texture_surface_desc(args, memory),
            Call::TextureSurfaceLockRect => return self.texture_surface_lock_rect(args, memory),
            Call::TextureSurfaceUnlockRect => self.texture_surface_unlock_rect(args[0]),
            Call::SetVertexShader | Call::SetPixelShader => self.set_shader(call, args),
            Call::SetIndices => return self.set_indices(args, memory),
            Call::GetIndices => return self.get_indices(args, memory),
            Call::SetStreamSource => return self.set_stream_source(args, memory),
            Call::GetStreamSource => return self.get_stream_source(args, memory),
            Call::SetViewport => return self.set_viewport(args, memory),
            Call::SetTransform => return self.set_transform(args, memory),
            Call::GetTransform => return self.get_transform(args, memory),
            Call::SetRenderState => self.set_render_state(args),
            Call::SetTexture => return self.set_texture(args, memory),
            Call::GetTextureStageState => return self.get_texture_stage_state(args, memory),
            Call::SetTextureStageState => self.set_texture_stage_state(args),
            Call::ValidateDevice => return self.validate_device(args, memory),
            Call::TestCooperativeLevel => self.test_cooperative_level(args[0]),
            Call::BeginScene | Call::EndScene => self.scene(call, args[0]),
            Call::GetDepthStencilSurface => return self.get_depth_surface(args, memory),
            Call::DepthSurfaceAddRef | Call::DepthSurfaceRelease => {
                self.depth_surface_ref(call, args)
            }
            Call::AvailableTextureMemory => self.available_texture_memory(args[0]),
            Call::DrawPrimitiveUp => return self.draw_primitive_up(args, memory),
            Call::DrawIndexedPrimitive => return primitives::draw_indexed(self, args, memory),
            Call::TextureLevelCount => self.texture_level_count(args[0]),
            Call::TextureAddRef => self.texture_add_ref(args[0]),
            Call::TextureRelease => return self.texture_release(args[0], memory),
            Call::VertexBufferAddRef => self.vertex_buffer_add_ref(args[0]),
            Call::VertexBufferRelease => return self.vertex_buffer_release(args[0], memory),
            Call::VertexBufferLock | Call::IndexBufferLock => {
                return self.buffer_lock(call, args, memory);
            }
            Call::VertexBufferUnlock | Call::IndexBufferUnlock => self.buffer_unlock(call, args[0]),
            Call::IndexBufferAddRef => self.index_buffer_add_ref(args[0]),
            Call::IndexBufferRelease => return self.index_buffer_release(args[0], memory),
            Call::TexturePreLoad => self.textures.get(&args[0]).map_or(INVALID_CALL, |texture| {
                if texture.refs == 0 { INVALID_CALL } else { 0 }
            }),
            Call::Clear => return self.clear(args, memory),
            Call::Present => self.present(args),
            Call::RootAddRef | Call::RootRelease if args[0] == ROOT && self.root_refs != 0 => {
                if matches!(call, Call::RootAddRef) {
                    self.root_refs = self.root_refs.saturating_add(1);
                } else {
                    self.root_refs -= 1;
                }
                self.root_refs
            }
            Call::DeviceAddRef | Call::DeviceRelease
                if args[0] == DEVICE && self.device_refs != 0 =>
            {
                if matches!(call, Call::DeviceAddRef) {
                    self.device_refs = self.device_refs.saturating_add(1);
                } else {
                    self.device_refs -= 1;
                    if self.device_refs == 0 {
                        self.finish_device();
                    }
                }
                self.device_refs
            }
            _ => INVALID_CALL,
        })
    }

    fn adapter_identifier(
        &self,
        args: &[u32],
        memory: &mut GuestMemory,
    ) -> Result<u32, MemoryError> {
        if args[0] != ROOT || self.root_refs == 0 || args[1] != 0 || !matches!(args[2], 0 | 2) {
            return Ok(INVALID_CALL);
        }
        guest::check(memory, args[3], ADAPTER_IDENTIFIER_SIZE, Access::Write)?;
        let mut identifier = [0; ADAPTER_IDENTIFIER_SIZE];
        identifier[..ADAPTER_DRIVER.len()].copy_from_slice(ADAPTER_DRIVER);
        let description = &mut identifier[512..];
        description[..ADAPTER_DESCRIPTION.len()].copy_from_slice(ADAPTER_DESCRIPTION);
        memory.write(u64::from(args[3]), &identifier)?;
        Ok(0)
    }

    fn device_caps(&self, args: &[u32], memory: &mut GuestMemory) -> Result<u32, MemoryError> {
        if args[0] != ROOT || self.root_refs == 0 || args[1] != 0 || !matches!(args[2], 1..=3) {
            return Ok(INVALID_CALL);
        }
        if args[2] != 1 {
            return Ok(NOT_AVAILABLE);
        }
        guest::check(memory, args[3], DEVICE_CAPS_SIZE, Access::Write)?;
        let mut caps = [0; DEVICE_CAPS_SIZE];
        caps[..4].copy_from_slice(&1_u32.to_le_bytes());
        caps[12..16].copy_from_slice(&CAPS2_CAN_RENDER_WINDOWED.to_le_bytes());
        caps[28..32].copy_from_slice(&DEVCAPS_DRAWPRIM_TLVERTEX.to_le_bytes());
        caps[32..36].copy_from_slice(&2_u32.to_le_bytes());
        caps[40..44].copy_from_slice(&0xff_u32.to_le_bytes());
        caps[180..184].copy_from_slice(&primitives::MAX_PRIMITIVES.to_le_bytes());
        caps[188..192].copy_from_slice(&1_u32.to_le_bytes());
        caps[192..196].copy_from_slice(&primitives::MAX_STRIDE.to_le_bytes());
        memory.write(u64::from(args[3]), &caps)?;
        Ok(0)
    }

    fn adapter_mode(&self, args: &[u32], memory: &mut GuestMemory) -> Result<u32, MemoryError> {
        if args[0] != ROOT || self.root_refs == 0 || args[1] != 0 || args[2] != 0 {
            return Ok(INVALID_CALL);
        }
        guest::check(memory, args[3], 16, Access::Write)?;
        let mut mode = [0; 16];
        for (field, value) in mode.chunks_exact_mut(4).zip([640_u32, 480, 0, 22]) {
            field.copy_from_slice(&value.to_le_bytes());
        }
        memory.write(u64::from(args[3]), &mode)?;
        Ok(0)
    }

    fn check_device_type(&self, args: &[u32]) -> u32 {
        if args[0] != ROOT || self.root_refs == 0 || args[1] != 0 || !matches!(args[2], 1..=3) {
            return INVALID_CALL;
        }
        if args[2] == 1 && args[3] == 22 && args[4] == 22 {
            0
        } else {
            NOT_AVAILABLE
        }
    }

    fn check_device_format(&self, args: &[u32]) -> u32 {
        if args[0] != ROOT || self.root_refs == 0 || args[1] != 0 || !matches!(args[2], 1..=3) {
            return INVALID_CALL;
        }
        if matches!(
            args[2..],
            [1, 22, 1, 1, 22] | [1, 22, 0, 3, 21 | 22] | [1, 22, 2, 1, 80]
        ) {
            0
        } else {
            NOT_AVAILABLE
        }
    }

    fn check_depth_stencil_match(&self, args: &[u32]) -> u32 {
        if args[0] != ROOT || self.root_refs == 0 || args[1] != 0 || !matches!(args[2], 1..=3) {
            return INVALID_CALL;
        }
        if args[2..] == [1, 22, 22, 80] {
            0
        } else {
            NOT_AVAILABLE
        }
    }

    fn check_multisample_type(&self, args: &[u32]) -> u32 {
        if args[0] != ROOT
            || self.root_refs == 0
            || args[1] != 0
            || !matches!(args[2], 1..=3)
            || args[5] > 16
        {
            return INVALID_CALL;
        }
        if args[2] == 1 && args[3] == 22 && args[5] == 0 {
            0
        } else {
            NOT_AVAILABLE
        }
    }

    fn create_device(
        &mut self,
        args: &[u32],
        memory: &mut GuestMemory,
        desktop: &Desktop,
    ) -> Result<u32, MemoryError> {
        let valid_focus = matches!(args[3], 0 | DESKTOP)
            || desktop
                .window(args[3])
                .is_some_and(|window| window.parent == 0);
        if args[0] != ROOT
            || self.root_refs == 0
            || args[1] != 0
            || args[2] != 1
            || !valid_focus
            || args[4] != 0x20
        {
            return Ok(INVALID_CALL);
        }
        if self.device_refs != 0 {
            return Ok(NOT_AVAILABLE);
        }
        let mut params = [0; 13];
        guest::read_words(memory, args[5], &mut params)?;
        let [
            width,
            height,
            format,
            count,
            samples,
            swap,
            window,
            windowed,
            enable_depth,
            depth_format,
            flags,
            refresh,
            interval,
        ] = params;
        if width == 0
            || height == 0
            || u64::from(width) * u64::from(height) > MAX_PIXELS
            || format != 22
            || count > 2
            || samples != 0
            || swap != 1
            || !(matches!(window, 0 | DESKTOP)
                || desktop
                    .window(window)
                    .is_some_and(|owned| owned.parent == 0))
            || (window == 0 && args[3] == 0)
            || !matches!(windowed, 0 | 1)
            || (windowed == 0 && (width != 640 || height != 480))
            || (windowed == 0 && (matches!(args[3], 0 | DESKTOP) || matches!(window, 0 | DESKTOP)))
            || !matches!((enable_depth, depth_format), (0, 0) | (1, 80))
            || flags != 0
            || refresh != 0
            || interval != 0
        {
            return Ok(INVALID_CALL);
        }
        guest::check(memory, args[5], 52, Access::Write)?;
        guest::check(memory, args[6], 4, Access::Write)?;
        let mut frame = Frame {
            width,
            height,
            rgba: vec![
                0;
                usize::try_from(u64::from(width) * u64::from(height) * 4)
                    .expect("admitted frame fits a 32-bit host")
            ],
        };
        for pixel in frame.rgba.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
        guest::write_word(memory, args[5] + 12, count.max(1))?;
        guest::write_word(memory, args[6], DEVICE)?;
        self.root_refs = self.root_refs.saturating_add(1);
        self.device_refs = 1;
        self.scene_open = false;
        self.texture_stages = [0; 8];
        self.indices = None;
        self.stream = None;
        self.color_arg0 = [1; 8];
        self.depth = (enable_depth == 1).then(|| vec![u16::MAX; (width * height) as usize]);
        self.depth_surface_refs = 0;
        self.z_enabled = enable_depth == 1;
        self.depth_policy = DepthPolicy::default();
        self.spare = (count == 2).then(|| frame.clone());
        self.viewport = Some(Viewport {
            left: 0,
            top: 0,
            right: width as usize,
            bottom: height as usize,
        });
        self.back = Some(frame);
        Ok(0)
    }

    fn finish_device(&mut self) {
        self.back = None;
        self.spare = None;
        self.depth = None;
        self.depth_surface_refs = 0;
        self.z_enabled = false;
        self.depth_policy = DepthPolicy::default();
        self.scene_open = false;
        self.viewport = None;
        self.transforms.clear();
        self.vertex_fvf = 0;
        self.texture_stages = [0; 8];
        self.indices = None;
        self.stream = None;
        self.color_arg0 = [1; 8];
        self.root_refs = self.root_refs.saturating_sub(1);
    }

    fn set_viewport(&mut self, args: &[u32], memory: &GuestMemory) -> Result<u32, MemoryError> {
        if args[0] != DEVICE || self.device_refs == 0 || args[1] == 0 {
            return Ok(INVALID_CALL);
        }
        let Some(frame) = self.back.as_ref() else {
            return Ok(INVALID_CALL);
        };
        let mut fields = [0; 6];
        guest::read_words(memory, args[1], &mut fields)?;
        let [x, y, width, height, min_z, max_z] = fields;
        if width == 0
            || height == 0
            || x.checked_add(width).is_none_or(|right| right > frame.width)
            || y.checked_add(height)
                .is_none_or(|bottom| bottom > frame.height)
            || min_z != 0_f32.to_bits()
            || max_z != 1_f32.to_bits()
        {
            return Ok(INVALID_CALL);
        }
        self.viewport = Some(Viewport {
            left: x as usize,
            top: y as usize,
            right: (x + width) as usize,
            bottom: (y + height) as usize,
        });
        Ok(0)
    }

    fn set_transform(&mut self, args: &[u32], memory: &GuestMemory) -> Result<u32, MemoryError> {
        if args[0] != DEVICE
            || self.device_refs == 0
            || !valid_transform_state(args[1])
            || args[2] == 0
        {
            return Ok(INVALID_CALL);
        }
        let mut matrix = [0; 16];
        guest::read_words(memory, args[2], &mut matrix)?;
        if matrix.iter().any(|word| !f32::from_bits(*word).is_finite()) {
            return Ok(INVALID_CALL);
        }
        self.transforms.insert(args[1], matrix);
        Ok(0)
    }

    fn get_transform(&self, args: &[u32], memory: &mut GuestMemory) -> Result<u32, MemoryError> {
        if args[0] != DEVICE
            || self.device_refs == 0
            || !valid_transform_state(args[1])
            || args[2] == 0
        {
            return Ok(INVALID_CALL);
        }
        guest::check(memory, args[2], 64, Access::Write)?;
        let matrix = self
            .transforms
            .get(&args[1])
            .copied()
            .unwrap_or(IDENTITY_MATRIX);
        let mut bytes = [0; 64];
        for (word, chunk) in matrix.iter().zip(bytes.chunks_exact_mut(4)) {
            chunk.copy_from_slice(&word.to_le_bytes());
        }
        memory.write(u64::from(args[2]), &bytes)?;
        Ok(0)
    }

    fn set_render_state(&mut self, args: &[u32]) -> u32 {
        if args[0] != DEVICE || self.device_refs == 0 {
            return INVALID_CALL;
        }
        match args[1] {
            7 if args[2] <= 1 => self.z_enabled = args[2] == 1,
            14 if args[2] <= 1 => self.depth_policy.write_enabled = args[2] == 1,
            23 if (1..=8).contains(&args[2]) => self.depth_policy.function = args[2],
            _ => return INVALID_CALL,
        }
        0
    }

    fn set_texture(&mut self, args: &[u32], memory: &mut GuestMemory) -> Result<u32, MemoryError> {
        let [device, stage, next] = <[u32; 3]>::try_from(args).expect("d3d8 call arity");
        let Ok(index) = usize::try_from(stage) else {
            return Ok(INVALID_CALL);
        };
        if device != DEVICE || self.device_refs == 0 || index >= self.texture_stages.len() {
            return Ok(INVALID_CALL);
        }
        let previous = self.texture_stages[index];
        if previous == next {
            return Ok(0);
        }
        if next != 0 {
            let Some(texture) = self.textures.get(&next) else {
                return Ok(INVALID_CALL);
            };
            if texture.refs == 0 || texture.total_refs() == u32::MAX {
                return Ok(INVALID_CALL);
            }
        }
        if previous != 0 {
            self.texture_release(previous, memory)?;
        }
        if next != 0 {
            self.texture_add_ref(next);
        }
        self.texture_stages[index] = next;
        Ok(0)
    }

    fn set_texture_stage_state(&mut self, args: &[u32]) -> u32 {
        let [device, stage, kind, value] = <[u32; 4]>::try_from(args).expect("d3d8 call arity");
        let Ok(index) = usize::try_from(stage) else {
            return INVALID_CALL;
        };
        if device != DEVICE
            || self.device_refs == 0
            || index >= self.color_arg0.len()
            || kind != 26
            || value > 2
        {
            return INVALID_CALL;
        }
        self.color_arg0[index] = value;
        0
    }

    fn get_texture_stage_state(
        &self,
        args: &[u32],
        memory: &mut GuestMemory,
    ) -> Result<u32, MemoryError> {
        let [device, stage, kind, output] = <[u32; 4]>::try_from(args).expect("d3d8 call arity");
        let Ok(index) = usize::try_from(stage) else {
            return Ok(INVALID_CALL);
        };
        if device != DEVICE || self.device_refs == 0 || index >= self.color_arg0.len() || kind != 26
        {
            return Ok(INVALID_CALL);
        }
        guest::check(memory, output, 4, Access::Write)?;
        guest::write_word(memory, output, self.color_arg0[index])?;
        Ok(0)
    }

    fn validate_device(&self, args: &[u32], memory: &mut GuestMemory) -> Result<u32, MemoryError> {
        let [device, output] = <[u32; 2]>::try_from(args).expect("d3d8 call arity");
        if device != DEVICE || self.device_refs == 0 {
            return Ok(INVALID_CALL);
        }
        if self.texture_stages.iter().any(|texture| *texture != 0) {
            return Ok(UNSUPPORTED_COLOR_OPERATION);
        }
        guest::check(memory, output, 4, Access::Write)?;
        guest::write_word(memory, output, 1)?;
        Ok(0)
    }

    fn test_cooperative_level(&self, device: u32) -> u32 {
        if device == DEVICE && self.device_refs != 0 {
            0
        } else {
            INVALID_CALL
        }
    }

    fn scene(&mut self, call: Call, device: u32) -> u32 {
        let opening = matches!(call, Call::BeginScene);
        if device != DEVICE || self.device_refs == 0 || self.scene_open == opening {
            return INVALID_CALL;
        }
        self.scene_open = opening;
        0
    }

    fn get_depth_surface(
        &mut self,
        args: &[u32],
        memory: &mut GuestMemory,
    ) -> Result<u32, MemoryError> {
        if args[0] != DEVICE || self.device_refs == 0 || self.depth.is_none() {
            return Ok(INVALID_CALL);
        }
        if self.depth_surface_refs >= u32::MAX - 1 {
            return Ok(INVALID_CALL);
        }
        let refs = self.depth_surface_refs + 1;
        let retained_device = if self.depth_surface_refs == 0 {
            let Some(device_refs) = self.device_refs.checked_add(1) else {
                return Ok(INVALID_CALL);
            };
            device_refs
        } else {
            self.device_refs
        };
        guest::check(memory, args[1], 4, Access::Write)?;
        guest::write_word(memory, args[1], DEPTH_SURFACE)?;
        self.depth_surface_refs = refs;
        self.device_refs = retained_device;
        Ok(0)
    }

    fn depth_surface_ref(&mut self, call: Call, args: &[u32]) -> u32 {
        if args[0] != DEPTH_SURFACE || self.depth_surface_refs == 0 {
            return INVALID_CALL;
        }
        if matches!(call, Call::DepthSurfaceAddRef) {
            if self.depth_surface_refs >= u32::MAX - 1 {
                return INVALID_CALL;
            }
            let refs = self.depth_surface_refs + 1;
            self.depth_surface_refs = refs;
            return refs + 1;
        }
        self.depth_surface_refs -= 1;
        if self.depth_surface_refs != 0 {
            return self.depth_surface_refs + 1;
        }
        self.device_refs -= 1;
        if self.device_refs == 0 {
            self.finish_device();
            0
        } else {
            1
        }
    }

    fn available_texture_memory(&self, device: u32) -> u32 {
        if device != DEVICE || self.device_refs == 0 {
            return 0;
        }
        let used: u64 = self
            .textures
            .values()
            .map(|texture| texture.length)
            .sum::<u64>()
            + self
                .vertex_buffers
                .values()
                .map(|buffer| buffer.length)
                .sum::<u64>()
            + self
                .index_buffers
                .values()
                .map(|buffer| buffer.length)
                .sum::<u64>();
        let free = (TEXTURE_END - TEXTURE_START).saturating_sub(used);
        u32::try_from(((free + MIB / 2) / MIB) * MIB).expect("bounded texture aperture")
    }

    fn present(&mut self, args: &[u32]) -> u32 {
        if args[0] != DEVICE || self.device_refs == 0 || args[1..5] != [0; 4] {
            return INVALID_CALL;
        }
        self.front.clone_from(&self.back);
        if let (Some(back), Some(spare)) = (&mut self.back, &mut self.spare) {
            std::mem::swap(back, spare);
        }
        0
    }

    fn set_shader(&mut self, call: Call, args: &[u32]) -> u32 {
        if args[0] != DEVICE || self.device_refs == 0 {
            return INVALID_CALL;
        }
        match call {
            Call::SetVertexShader if matches!(args[1], 0x44 | 0x142 | 0x152) => {
                self.vertex_fvf = args[1];
                0
            }
            Call::SetPixelShader if args[1] == 0 => 0,
            _ => INVALID_CALL,
        }
    }

    fn set_indices(&mut self, args: &[u32], memory: &mut GuestMemory) -> Result<u32, MemoryError> {
        let [device, next, base] = <[u32; 3]>::try_from(args).expect("d3d8 call arity");
        if device != DEVICE || self.device_refs == 0 {
            return Ok(INVALID_CALL);
        }
        let previous = self.indices.map_or(0, |(buffer, _)| buffer);
        if next == previous {
            self.indices = (next != 0).then_some((next, base));
            return Ok(0);
        }
        if next != 0
            && self
                .index_buffers
                .get(&next)
                .is_none_or(|buffer| buffer.refs == u32::MAX)
        {
            return Ok(INVALID_CALL);
        }
        if previous != 0 {
            self.index_buffer_release(previous, memory)?;
        }
        if next != 0 {
            self.index_buffer_add_ref(next);
        }
        self.indices = (next != 0).then_some((next, base));
        Ok(0)
    }

    fn get_indices(&mut self, args: &[u32], memory: &mut GuestMemory) -> Result<u32, MemoryError> {
        let [device, output_buffer, output_base] =
            <[u32; 3]>::try_from(args).expect("d3d8 call arity");
        if device != DEVICE || self.device_refs == 0 || output_buffer.abs_diff(output_base) < 4 {
            return Ok(INVALID_CALL);
        }
        let (buffer, base) = self.indices.unwrap_or((0, 0));
        if buffer != 0
            && self
                .index_buffers
                .get(&buffer)
                .is_none_or(|buffer| buffer.refs == u32::MAX)
        {
            return Ok(INVALID_CALL);
        }
        guest::check(memory, output_buffer, 4, Access::Write)?;
        guest::check(memory, output_base, 4, Access::Write)?;
        guest::write_word(memory, output_buffer, buffer)?;
        guest::write_word(memory, output_base, base)?;
        if buffer != 0 {
            self.index_buffer_add_ref(buffer);
        }
        Ok(0)
    }

    fn set_stream_source(
        &mut self,
        args: &[u32],
        memory: &mut GuestMemory,
    ) -> Result<u32, MemoryError> {
        let [device, stream, next, stride] = <[u32; 4]>::try_from(args).expect("d3d8 call arity");
        if device != DEVICE
            || self.device_refs == 0
            || stream != 0
            || (next == 0 && stride != 0)
            || (next != 0 && !(1..=primitives::MAX_STRIDE).contains(&stride))
        {
            return Ok(INVALID_CALL);
        }
        let previous = self.stream.map_or(0, |(buffer, _)| buffer);
        if previous == next {
            self.stream = (next != 0).then_some((next, stride));
            return Ok(0);
        }
        if next != 0
            && self
                .vertex_buffers
                .get(&next)
                .is_none_or(|buffer| buffer.refs == u32::MAX)
        {
            return Ok(INVALID_CALL);
        }
        if previous != 0 {
            self.vertex_buffer_release(previous, memory)?;
        }
        if next != 0 {
            self.vertex_buffer_add_ref(next);
        }
        self.stream = (next != 0).then_some((next, stride));
        Ok(0)
    }

    fn get_stream_source(
        &mut self,
        args: &[u32],
        memory: &mut GuestMemory,
    ) -> Result<u32, MemoryError> {
        let [device, stream, output_buffer, output_stride] =
            <[u32; 4]>::try_from(args).expect("d3d8 call arity");
        if device != DEVICE
            || self.device_refs == 0
            || stream != 0
            || output_buffer.abs_diff(output_stride) < 4
        {
            return Ok(INVALID_CALL);
        }
        let (buffer, stride) = self.stream.unwrap_or((0, 0));
        if buffer != 0
            && self
                .vertex_buffers
                .get(&buffer)
                .is_none_or(|buffer| buffer.refs == u32::MAX)
        {
            return Ok(INVALID_CALL);
        }
        guest::check(memory, output_buffer, 4, Access::Write)?;
        guest::check(memory, output_stride, 4, Access::Write)?;
        guest::write_word(memory, output_buffer, buffer)?;
        guest::write_word(memory, output_stride, stride)?;
        if buffer != 0 {
            self.vertex_buffer_add_ref(buffer);
        }
        Ok(0)
    }

    fn draw_primitive_up(
        &mut self,
        args: &[u32],
        memory: &GuestMemory,
    ) -> Result<u32, MemoryError> {
        if args[0] != DEVICE || self.device_refs == 0 {
            return Ok(INVALID_CALL);
        }
        primitives::draw_up(
            self.back.as_mut(),
            self.depth.as_deref_mut().filter(|_| self.z_enabled),
            self.depth_policy,
            self.viewport,
            self.vertex_fvf,
            args,
            memory,
        )
    }

    fn create_vertex_buffer(
        &mut self,
        args: &[u32],
        memory: &mut GuestMemory,
    ) -> Result<u32, MemoryError> {
        let [device, byte_length, usage, fvf, pool, output] =
            <[u32; 6]>::try_from(args).expect("d3d8 call arity");
        let position = fvf & 0x000e;
        let supported_fvf = matches!(position, 0x0002 | 0x0004)
            && fvf & !(0x000e | 0x0010 | 0x0040 | 0x0080 | 0x0100) == 0
            && (position != 0x0004 || fvf & 0x0010 == 0);
        if device != DEVICE
            || self.device_refs == 0
            || byte_length == 0
            || usage & !(0x0008 | 0x0010 | 0x0200) != 0
            || !supported_fvf
            || !matches!(pool, 0..=2)
        {
            return Ok(INVALID_CALL);
        }
        guest::check(memory, output, 4, Access::Write)?;
        let length = PAGE_SIZE + u64::from(byte_length).div_ceil(PAGE_SIZE) * PAGE_SIZE;
        let Some(address) = memory.first_free_span(TEXTURE_START, TEXTURE_END, length)? else {
            return Ok(OUT_OF_VIDEO_MEMORY);
        };
        if memory
            .map_zeroed(address, length, Permissions::READ_WRITE)
            .is_err()
        {
            return Ok(OUT_OF_VIDEO_MEMORY);
        }
        let address = u32::try_from(address).expect("vertex buffer guest range is 32-bit");
        guest::write_word(memory, address, VERTEX_BUFFER_TABLE)?;
        memory.protect(u64::from(address), PAGE_SIZE, Permissions::READ)?;
        guest::write_word(memory, output, address)?;
        self.vertex_buffers.insert(
            address,
            VertexBuffer {
                refs: 1,
                length,
                lock: BufferLock {
                    byte_length,
                    usage,
                    count: 0,
                },
            },
        );
        self.device_refs = self.device_refs.saturating_add(1);
        Ok(0)
    }

    fn vertex_buffer_add_ref(&mut self, address: u32) -> u32 {
        let Some(buffer) = self.vertex_buffers.get_mut(&address) else {
            return INVALID_CALL;
        };
        buffer.refs = buffer.refs.saturating_add(1);
        buffer.refs
    }

    fn buffer_lock(
        &mut self,
        call: Call,
        args: &[u32],
        memory: &mut GuestMemory,
    ) -> Result<u32, MemoryError> {
        let address = args[0];
        let lock = match call {
            Call::VertexBufferLock => self
                .vertex_buffers
                .get_mut(&address)
                .map(|buffer| &mut buffer.lock),
            Call::IndexBufferLock => self
                .index_buffers
                .get_mut(&address)
                .map(|buffer| &mut buffer.lock),
            _ => unreachable!("buffer lock dispatch"),
        };
        lock.map_or(Ok(INVALID_CALL), |lock| lock.lock(address, args, memory))
    }

    fn buffer_unlock(&mut self, call: Call, address: u32) -> u32 {
        let lock = match call {
            Call::VertexBufferUnlock => self
                .vertex_buffers
                .get_mut(&address)
                .map(|buffer| &mut buffer.lock),
            Call::IndexBufferUnlock => self
                .index_buffers
                .get_mut(&address)
                .map(|buffer| &mut buffer.lock),
            _ => unreachable!("buffer unlock dispatch"),
        };
        lock.map_or(INVALID_CALL, BufferLock::unlock)
    }

    fn vertex_buffer_release(
        &mut self,
        address: u32,
        memory: &mut GuestMemory,
    ) -> Result<u32, MemoryError> {
        let Some(buffer) = self.vertex_buffers.get_mut(&address) else {
            return Ok(INVALID_CALL);
        };
        if buffer.refs > 1 {
            buffer.refs -= 1;
            return Ok(buffer.refs);
        }
        memory.unmap(u64::from(address), buffer.length)?;
        self.vertex_buffers.remove(&address);
        self.device_refs -= 1;
        if self.device_refs == 0 {
            self.finish_device();
        }
        Ok(0)
    }

    fn create_index_buffer(
        &mut self,
        args: &[u32],
        memory: &mut GuestMemory,
    ) -> Result<u32, MemoryError> {
        let [device, byte_length, usage, format, pool, output] =
            <[u32; 6]>::try_from(args).expect("d3d8 call arity");
        let index_size = match format {
            101 => 2,
            102 => 4,
            _ => return Ok(INVALID_CALL),
        };
        if device != DEVICE
            || self.device_refs == 0
            || byte_length == 0
            || byte_length % index_size != 0
            || usage & !(0x0008 | 0x0010 | 0x0200) != 0
            || !matches!(pool, 0..=2)
        {
            return Ok(INVALID_CALL);
        }
        guest::check(memory, output, 4, Access::Write)?;
        let length = PAGE_SIZE + u64::from(byte_length).div_ceil(PAGE_SIZE) * PAGE_SIZE;
        let Some(address) = memory.first_free_span(TEXTURE_START, TEXTURE_END, length)? else {
            return Ok(OUT_OF_VIDEO_MEMORY);
        };
        if memory
            .map_zeroed(address, length, Permissions::READ_WRITE)
            .is_err()
        {
            return Ok(OUT_OF_VIDEO_MEMORY);
        }
        let address = u32::try_from(address).expect("index buffer guest range is 32-bit");
        guest::write_word(memory, address, INDEX_BUFFER_TABLE)?;
        memory.protect(u64::from(address), PAGE_SIZE, Permissions::READ)?;
        guest::write_word(memory, output, address)?;
        self.index_buffers.insert(
            address,
            IndexBuffer {
                refs: 1,
                length,
                format,
                lock: BufferLock {
                    byte_length,
                    usage,
                    count: 0,
                },
            },
        );
        self.device_refs = self.device_refs.saturating_add(1);
        Ok(0)
    }

    fn index_buffer_add_ref(&mut self, address: u32) -> u32 {
        let Some(buffer) = self.index_buffers.get_mut(&address) else {
            return INVALID_CALL;
        };
        buffer.refs = buffer.refs.saturating_add(1);
        buffer.refs
    }

    fn index_buffer_release(
        &mut self,
        address: u32,
        memory: &mut GuestMemory,
    ) -> Result<u32, MemoryError> {
        let Some(buffer) = self.index_buffers.get_mut(&address) else {
            return Ok(INVALID_CALL);
        };
        if buffer.refs > 1 {
            buffer.refs -= 1;
            return Ok(buffer.refs);
        }
        memory.unmap(u64::from(address), buffer.length)?;
        self.index_buffers.remove(&address);
        self.device_refs -= 1;
        if self.device_refs == 0 {
            self.finish_device();
        }
        Ok(0)
    }

    fn create_texture(
        &mut self,
        args: &[u32],
        memory: &mut GuestMemory,
    ) -> Result<u32, MemoryError> {
        let [
            device,
            width,
            height,
            requested_levels,
            usage,
            format,
            pool,
            output,
        ] = <[u32; 8]>::try_from(args).expect("d3d8 call arity");
        if device != DEVICE
            || self.device_refs == 0
            || width == 0
            || height == 0
            || u64::from(width) * u64::from(height) > MAX_PIXELS
            || usage != 0
            || !matches!(format, 21 | 22)
            || !matches!(pool, 0..=2)
        {
            return Ok(INVALID_CALL);
        }

        let mut levels = Vec::new();
        let (mut level_width, mut level_height) = (width, height);
        let mut pixel_bytes = 0_u64;
        loop {
            levels.push(TextureLevel {
                width: level_width,
                height: level_height,
                offset: u32::try_from(PAGE_SIZE + pixel_bytes)
                    .expect("bounded texture address offset"),
                locked: false,
                surface_refs: 0,
            });
            pixel_bytes += u64::from(level_width) * u64::from(level_height) * 4;
            if (requested_levels != 0 && levels.len() == requested_levels as usize)
                || (level_width == 1 && level_height == 1)
            {
                break;
            }
            level_width = (level_width / 2).max(1);
            level_height = (level_height / 2).max(1);
        }
        if requested_levels != 0 && levels.len() != requested_levels as usize {
            return Ok(INVALID_CALL);
        }
        guest::check(memory, output, 4, Access::Write)?;
        let length = PAGE_SIZE + pixel_bytes.div_ceil(PAGE_SIZE) * PAGE_SIZE;
        let Some(address) = memory.first_free_span(TEXTURE_START, TEXTURE_END, length)? else {
            return Ok(OUT_OF_VIDEO_MEMORY);
        };
        if memory
            .map_zeroed(address, length, Permissions::READ_WRITE)
            .is_err()
        {
            return Ok(OUT_OF_VIDEO_MEMORY);
        }
        let address = u32::try_from(address).expect("texture guest range is 32-bit");
        guest::write_word(memory, address, TEXTURE_TABLE)?;
        for index in 0..levels.len() {
            let offset = u32::try_from(index).expect("bounded mip count") * 4;
            guest::write_word(memory, address + 4 + offset, TEXTURE_SURFACE_TABLE)?;
        }
        memory.protect(u64::from(address), PAGE_SIZE, Permissions::READ)?;
        guest::write_word(memory, output, address)?;
        self.textures.insert(
            address,
            Texture {
                refs: 1,
                length,
                format,
                pool,
                levels,
            },
        );
        self.device_refs = self.device_refs.saturating_add(1);
        Ok(0)
    }

    fn texture_release(
        &mut self,
        address: u32,
        memory: &mut GuestMemory,
    ) -> Result<u32, MemoryError> {
        let Some(texture) = self.textures.get_mut(&address) else {
            return Ok(INVALID_CALL);
        };
        if texture.refs == 0 {
            return Ok(INVALID_CALL);
        }
        if texture.total_refs() > 1 {
            texture.refs -= 1;
            return Ok(texture.total_refs());
        }
        memory.unmap(u64::from(address), texture.length)?;
        self.textures.remove(&address);
        self.device_refs -= 1;
        if self.device_refs == 0 {
            self.finish_device();
        }
        Ok(0)
    }

    fn texture_level_count(&self, address: u32) -> u32 {
        self.textures.get(&address).map_or(INVALID_CALL, |texture| {
            u32::try_from(texture.levels.len()).expect("bounded mip count")
        })
    }

    fn texture_add_ref(&mut self, address: u32) -> u32 {
        let Some(texture) = self.textures.get_mut(&address) else {
            return INVALID_CALL;
        };
        if texture.refs == 0 {
            return INVALID_CALL;
        }
        texture.refs = texture.refs.saturating_add(1);
        texture.total_refs()
    }

    fn texture_get_surface(
        &mut self,
        args: &[u32],
        memory: &mut GuestMemory,
    ) -> Result<u32, MemoryError> {
        let Some(texture) = self.textures.get_mut(&args[0]) else {
            return Ok(INVALID_CALL);
        };
        if texture.refs == 0 || texture.total_refs() == u32::MAX {
            return Ok(INVALID_CALL);
        }
        let Some(level) = texture.levels.get_mut(args[1] as usize) else {
            return Ok(INVALID_CALL);
        };
        if level.surface_refs == u32::MAX {
            return Ok(INVALID_CALL);
        }
        guest::check(memory, args[2], 4, Access::Write)?;
        let surface = args[0] + 4 + args[1] * 4;
        guest::write_word(memory, args[2], surface)?;
        level.surface_refs += 1;
        Ok(0)
    }

    fn texture_surface_ref(
        &mut self,
        call: Call,
        surface: u32,
        memory: &mut GuestMemory,
    ) -> Result<u32, MemoryError> {
        let Some((texture_address, index)) = self.texture_surface_identity(surface) else {
            return Ok(INVALID_CALL);
        };
        let texture = self
            .textures
            .get_mut(&texture_address)
            .expect("validated surface");
        let total_refs = texture.total_refs();
        let level = &mut texture.levels[index];
        if matches!(call, Call::TextureSurfaceAddRef) {
            if total_refs == u32::MAX {
                return Ok(INVALID_CALL);
            }
            level.surface_refs += 1;
            return Ok(level.surface_refs);
        }
        if total_refs == 1 {
            memory.unmap(u64::from(texture_address), texture.length)?;
            self.textures.remove(&texture_address);
            self.device_refs -= 1;
            if self.device_refs == 0 {
                self.finish_device();
            }
            return Ok(0);
        }
        level.surface_refs -= 1;
        Ok(level.surface_refs)
    }

    fn texture_surface_identity(&self, surface: u32) -> Option<(u32, usize)> {
        let page_size = u32::try_from(PAGE_SIZE).expect("guest page fits u32");
        let texture_address = surface & !(page_size - 1);
        let offset = surface.checked_sub(texture_address + 4)?;
        if offset % 4 != 0 {
            return None;
        }
        let index = (offset / 4) as usize;
        let texture = self.textures.get(&texture_address)?;
        (texture.levels.get(index)?.surface_refs != 0).then_some((texture_address, index))
    }

    fn texture_surface_desc(
        &self,
        args: &[u32],
        memory: &mut GuestMemory,
    ) -> Result<u32, MemoryError> {
        let Some((texture, level)) = self.texture_surface_identity(args[0]) else {
            return Ok(INVALID_CALL);
        };
        self.texture_level_desc(
            &[
                texture,
                u32::try_from(level).expect("bounded mip count"),
                args[1],
            ],
            memory,
        )
    }

    fn texture_surface_lock_rect(
        &mut self,
        args: &[u32],
        memory: &mut GuestMemory,
    ) -> Result<u32, MemoryError> {
        let Some((texture, level)) = self.texture_surface_identity(args[0]) else {
            return Ok(INVALID_CALL);
        };
        self.texture_lock_rect(
            &[
                texture,
                u32::try_from(level).expect("bounded mip count"),
                args[1],
                args[2],
                args[3],
            ],
            memory,
        )
    }

    fn texture_surface_unlock_rect(&mut self, surface: u32) -> u32 {
        let Some((texture, level)) = self.texture_surface_identity(surface) else {
            return INVALID_CALL;
        };
        self.texture_unlock_rect(&[texture, u32::try_from(level).expect("bounded mip count")])
    }

    fn texture_level_desc(
        &self,
        args: &[u32],
        memory: &mut GuestMemory,
    ) -> Result<u32, MemoryError> {
        let Some(texture) = self.textures.get(&args[0]) else {
            return Ok(INVALID_CALL);
        };
        let Some(level) = texture.levels.get(args[1] as usize) else {
            return Ok(INVALID_CALL);
        };
        guest::check(memory, args[2], 32, Access::Write)?;
        let values = [
            texture.format,
            3,
            0,
            texture.pool,
            level.width * level.height * 4,
            0,
            level.width,
            level.height,
        ];
        let bytes: Vec<_> = values.into_iter().flat_map(u32::to_le_bytes).collect();
        memory.write(u64::from(args[2]), &bytes)?;
        Ok(0)
    }

    fn texture_lock_rect(
        &mut self,
        args: &[u32],
        memory: &mut GuestMemory,
    ) -> Result<u32, MemoryError> {
        let Some(texture) = self.textures.get(&args[0]) else {
            return Ok(INVALID_CALL);
        };
        let Some(level) = texture.levels.get(args[1] as usize) else {
            return Ok(INVALID_CALL);
        };
        if level.locked || !matches!(args[4], 0 | 0x10) {
            return Ok(INVALID_CALL);
        }
        let (left, top) = if args[3] == 0 {
            (0, 0)
        } else {
            let mut rect = [0; 4];
            guest::read_words(memory, args[3], &mut rect)?;
            if rect[0] >= rect[2]
                || rect[1] >= rect[3]
                || rect[2] > level.width
                || rect[3] > level.height
            {
                return Ok(INVALID_CALL);
            }
            (rect[0], rect[1])
        };
        guest::check(memory, args[2], 8, Access::Write)?;
        let pitch = level.width * 4;
        let pixels = args[0] + level.offset + top * pitch + left * 4;
        let bytes: Vec<_> = [pitch, pixels]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        memory.write(u64::from(args[2]), &bytes)?;
        self.textures.get_mut(&args[0]).unwrap().levels[args[1] as usize].locked = true;
        Ok(0)
    }

    fn texture_unlock_rect(&mut self, args: &[u32]) -> u32 {
        let Some(level) = self
            .textures
            .get_mut(&args[0])
            .and_then(|texture| texture.levels.get_mut(args[1] as usize))
        else {
            return INVALID_CALL;
        };
        if !level.locked {
            return INVALID_CALL;
        }
        level.locked = false;
        0
    }

    fn clear(&mut self, args: &[u32], memory: &GuestMemory) -> Result<u32, MemoryError> {
        if args[0] != DEVICE
            || self.device_refs == 0
            || !matches!(args[3], 1..=3)
            || args[1] > MAX_RECTS
            || (args[1] == 0) != (args[2] == 0)
        {
            return Ok(INVALID_CALL);
        }
        let Some(back) = self.back.as_mut() else {
            return Ok(INVALID_CALL);
        };
        let clear_color = args[3] & 1 != 0;
        let clear_depth = args[3] & 2 != 0;
        if clear_depth && self.depth.is_none() {
            return Ok(INVALID_CALL);
        }
        let depth_value = f32::from_bits(args[5]);
        if clear_depth && (!depth_value.is_finite() || !(0.0..=1.0).contains(&depth_value)) {
            return Ok(INVALID_CALL);
        }
        let mut rects = vec![[0; 4]; args[1] as usize];
        guest::check(memory, args[2], rects.len() * 16, Access::Read)?;
        for (index, rect) in (0_u32..).zip(&mut rects) {
            guest::read_words(memory, args[2] + index * 16, rect)?;
            if rect[0].cast_signed() > rect[2].cast_signed()
                || rect[1].cast_signed() > rect[3].cast_signed()
            {
                return Ok(INVALID_CALL);
            }
        }
        if rects.is_empty() {
            rects.push([0, 0, back.width, back.height]);
        }
        let [_, red, green, blue] = args[4].to_be_bytes();
        for rect in rects {
            let x1 = rect[0].cast_signed().max(0).cast_unsigned().min(back.width);
            let y1 = rect[1]
                .cast_signed()
                .max(0)
                .cast_unsigned()
                .min(back.height);
            let x2 = rect[2].cast_signed().max(0).cast_unsigned().min(back.width);
            let y2 = rect[3]
                .cast_signed()
                .max(0)
                .cast_unsigned()
                .min(back.height);
            for y in y1..y2 {
                let start = ((y * back.width + x1) * 4) as usize;
                let end = ((y * back.width + x2) * 4) as usize;
                if clear_color {
                    for pixel in back.rgba[start..end].chunks_exact_mut(4) {
                        pixel.copy_from_slice(&[red, green, blue, 255]);
                    }
                }
                if clear_depth {
                    let start = (y * back.width + x1) as usize;
                    let end = (y * back.width + x2) as usize;
                    self.depth.as_mut().expect("validated depth surface")[start..end]
                        .fill(quantize_d16(f64::from(depth_value)));
                }
            }
        }
        Ok(0)
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn quantize_d16(value: f64) -> u16 {
    (value * f64::from(u16::MAX)).round() as u16
}
