use super::super::{
    patch::{
        domain::DomainMesh,
        program::{InputPlan, SurfaceProgram},
    },
    resources::{Format, Rect, ResourceDesc, SubresourceInfo, SurfaceRef},
    shader::{Declaration, Program},
    state::{PipelineState, TEXTURE_STAGES, VERTEX_STREAMS},
};
use crate::graphics::{GraphicsLimits, ResourceId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandError {
    InvalidLimits,
    InvalidEpoch,
    InvalidCommand,
    Backpressure,
    Capacity,
    Allocation,
    IdentityExhausted,
    StaleBatch,
    EmptyBatch,
    OutputTooSmall,
    InvalidRange,
    StalePreparation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandLimits {
    pub batch_bytes: u32,
    pub pending_bytes: u32,
}

impl From<GraphicsLimits> for CommandLimits {
    fn from(limits: GraphicsLimits) -> Self {
        Self {
            batch_bytes: limits.batch_bytes,
            pending_bytes: limits.resource_bytes.saturating_add(4 * 1024 * 1024),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandPlan {
    pub(super) payload_bytes: usize,
    pub(super) opcode: u16,
    pub(super) draining: bool,
}

impl CommandPlan {
    pub fn payload_bytes(self) -> usize {
        self.payload_bytes
    }
    pub fn requires_drain(self) -> bool {
        self.draining
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    Queued,
    Draining,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceVersion {
    pub id: ResourceId,
    pub version: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfaceVersion {
    pub surface: SurfaceRef,
    pub version: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Targets {
    pub color: SurfaceVersion,
    pub depth: Option<SurfaceVersion>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamResource {
    pub resource: ResourceVersion,
    pub stride: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IndexBinding {
    pub resource: ResourceVersion,
    pub format: Format,
    pub base_vertex: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct DrawBindings {
    pub textures: [Option<TextureBinding>; TEXTURE_STAGES],
    pub streams: [Option<StreamResource>; VERTEX_STREAMS],
    pub indices: Option<IndexBinding>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PipelineId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextureBinding {
    pub resource: ResourceVersion,
    pub lod: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Primitive {
    PointList = 1,
    LineList = 2,
    LineStrip = 3,
    TriangleList = 4,
    TriangleStrip = 5,
    TriangleFan = 6,
}

impl Primitive {
    pub fn element_count(self, primitives: u32) -> Option<u32> {
        if primitives == 0 {
            return Some(0);
        }
        match self {
            Self::PointList => Some(primitives),
            Self::LineList => primitives.checked_mul(2),
            Self::LineStrip => primitives.checked_add(1),
            Self::TriangleList => primitives.checked_mul(3),
            Self::TriangleStrip | Self::TriangleFan => primitives.checked_add(2),
        }
    }
}

impl TryFrom<u32> for Primitive {
    type Error = CommandError;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::PointList),
            2 => Ok(Self::LineList),
            3 => Ok(Self::LineStrip),
            4 => Ok(Self::TriangleList),
            5 => Ok(Self::TriangleStrip),
            6 => Ok(Self::TriangleFan),
            _ => Err(CommandError::InvalidCommand),
        }
    }
}

pub enum DrawData<'a> {
    Vertices {
        first_vertex: u32,
    },
    Indexed {
        min_vertex: u32,
        vertex_count: u32,
        start_index: u32,
    },
    UserVertices {
        stride: u32,
        vertices: &'a [u8],
    },
    UserIndexed {
        min_vertex: u32,
        vertex_count: u32,
        stride: u32,
        vertices: &'a [u8],
        format: Format,
        indices: &'a [u8],
    },
}

pub struct Draw<'a> {
    pub pipeline: PipelineId,
    pub primitive: Primitive,
    pub primitive_count: u32,
    pub bindings: DrawBindings,
    pub targets: Targets,
    pub data: DrawData<'a>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatchSource<'a> {
    Cached {
        identity: u64,
    },
    Inline {
        program: &'a SurfaceProgram,
        inputs: &'a InputPlan,
        streams: &'a [Option<StreamResource>; VERTEX_STREAMS],
    },
}

pub struct PatchDraw<'a> {
    pub pipeline: PipelineId,
    pub bindings: DrawBindings,
    pub targets: Targets,
    pub domain: u64,
    pub source: PatchSource<'a>,
}

pub struct Upload<'a> {
    pub resource: ResourceVersion,
    pub subresource: u32,
    pub offset: u32,
    pub row_pitch: u32,
    pub slice_pitch: u32,
    pub row_bytes: u32,
    pub rows: u32,
    pub slices: u32,
    pub bytes: &'a [u8],
}

pub enum ShaderDefinition<'a> {
    Vertex {
        declaration: &'a Declaration,
        program: Option<&'a Program>,
    },
    Pixel {
        program: &'a Program,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CopyRect {
    pub source: Rect,
    pub destination: (u32, u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SignedRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

pub struct Present<'a> {
    pub swap_chain: u64,
    pub swap_effect: u32,
    pub window: u32,
    pub source: SurfaceVersion,
    pub source_rect: Option<Rect>,
    pub destination_rect: Option<SignedRect>,
    pub dirty_rects: &'a [Rect],
    pub dirty_region: bool,
    pub interval: u32,
}

pub struct Reset<'a> {
    pub generation: u64,
    pub width: u32,
    pub height: u32,
    pub format: Format,
    pub multisample: u32,
    pub back_buffers: &'a [SurfaceVersion],
    pub depth: Option<SurfaceVersion>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum DeviceEvent {
    Lost = 1,
    Resettable = 2,
    Removed = 3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadbackId {
    pub epoch: u64,
    pub sequence: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum ReadbackKind {
    Lock = 1,
    FrontBuffer = 2,
    Copy = 3,
    ProcessVertices = 4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadbackRequest {
    pub id: ReadbackId,
    pub source: SurfaceVersion,
    pub kind: ReadbackKind,
    pub expected: ReadbackLayout,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadbackLayout {
    pub format: Format,
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub row_pitch: u32,
    pub slice_pitch: u32,
    pub length: u32,
}

pub enum Command<'a> {
    PatchKernel,
    DefinePatch {
        identity: u64,
        vertex_shader: u32,
        program: &'a SurfaceProgram,
        inputs: &'a InputPlan,
        streams: &'a [Option<StreamResource>; VERTEX_STREAMS],
    },
    DefinePatchDomain {
        identity: u64,
        mesh: &'a DomainMesh,
    },
    DrawPatch(&'a PatchDraw<'a>),
    DeletePatch {
        identity: u64,
    },
    DeletePatchDomain {
        identity: u64,
    },
    CreateResource {
        id: ResourceId,
        desc: ResourceDesc,
        subresources: &'a [SubresourceInfo],
    },
    DestroyResource(ResourceVersion),
    Upload(Upload<'a>),
    CreateShader {
        handle: u32,
        definition: ShaderDefinition<'a>,
    },
    DeleteShader {
        handle: u32,
    },
    Pipeline {
        id: PipelineId,
        state: &'a PipelineState,
    },
    Targets(Targets),
    Draw(Draw<'a>),
    Clear {
        targets: Targets,
        flags: u32,
        color: u32,
        depth: u32,
        stencil: u32,
        rectangles: &'a [Rect],
    },
    Copy {
        source: SurfaceVersion,
        destination: SurfaceVersion,
        rectangles: &'a [CopyRect],
    },
    Present(Present<'a>),
    Reset(Reset<'a>),
    DeviceEvent {
        generation: u64,
        event: DeviceEvent,
    },
    Readback(ReadbackRequest),
    Palette {
        index: u32,
        entries: &'a [[u8; 4]; 256],
    },
    Gamma {
        flags: u32,
        ramp: &'a [[u16; 256]; 3],
    },
    CursorShape {
        source: SurfaceVersion,
        hotspot: (u32, u32),
    },
    CursorPosition {
        x: i32,
        y: i32,
        flags: u32,
    },
    CursorVisible(bool),
    ProcessVertices {
        pipeline: PipelineId,
        bindings: DrawBindings,
        source_start: u32,
        destination: ResourceVersion,
        destination_index: u32,
        vertex_count: u32,
        flags: u32,
        destination_fvf: u32,
    },
}
