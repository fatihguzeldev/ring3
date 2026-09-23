use super::super::Access;
use super::{API_BASE, DispatchError, GuestMemory, MemoryError, PAGE_SIZE, Permissions, guest};

pub(super) const BASE: u32 = 0x7001_5000;
const TABLE_BASE: u32 = BASE + 0x100;
const OBJECT_BASE: u32 = BASE + 0x800;
const INTERFACES: u32 = 6;
const TABLE_SLOTS: u32 = 64;
const OBJECT_STRIDE: u32 = 0x20;
const MAX_GRAPHS: usize = 64;
const CLASS_NOT_REGISTERED: u32 = 0x8004_0154;
const CLASS_NO_AGGREGATION: u32 = 0x8004_0110;
const NOT_INITIALIZED: u32 = 0x8004_01f0;
const NO_INTERFACE: u32 = 0x8000_4002;
const NULL_POINTER: u32 = 0x8000_4003;
const OUT_OF_MEMORY: u32 = 0x8007_000e;

const fn guid(data1: u32, data2: u16, data3: u16, data4: [u8; 8]) -> [u8; 16] {
    let a = data1.to_le_bytes();
    let b = data2.to_le_bytes();
    let c = data3.to_le_bytes();
    [
        a[0], a[1], a[2], a[3], b[0], b[1], c[0], c[1], data4[0], data4[1], data4[2], data4[3],
        data4[4], data4[5], data4[6], data4[7],
    ]
}

const fn directshow_iid(data1: u32) -> [u8; 16] {
    guid(
        data1,
        0x0ad4,
        0x11ce,
        [0xb0, 0x3a, 0, 0x20, 0xaf, 0x0b, 0xa7, 0x70],
    )
}

const FILTER_GRAPH: [u8; 16] = guid(
    0xe436_ebb3,
    0x524f,
    0x11ce,
    [0x9f, 0x53, 0, 0x20, 0xaf, 0x0b, 0xa7, 0x70],
);
const GRAPH_BUILDER: [u8; 16] = directshow_iid(0x56a8_68a9);
const MEDIA_CONTROL: [u8; 16] = directshow_iid(0x56a8_68b1);
const MEDIA_SEEKING: [u8; 16] = guid(
    0x36b7_3880,
    0xc2c8,
    0x11cf,
    [0x8b, 0x46, 0, 0x80, 0x5f, 0x6c, 0xef, 0x60],
);
const MEDIA_POSITION: [u8; 16] = directshow_iid(0x56a8_68b2);
const VIDEO_WINDOW: [u8; 16] = directshow_iid(0x56a8_68b4);
const BASIC_AUDIO: [u8; 16] = directshow_iid(0x56a8_68b5);
const UNKNOWN: [u8; 16] = guid(0, 0, 0, [0xc0, 0, 0, 0, 0, 0, 0, 0x46]);

fn interface(iid: [u8; 16]) -> Option<u32> {
    match iid {
        GRAPH_BUILDER | UNKNOWN => Some(0),
        MEDIA_CONTROL => Some(1),
        MEDIA_SEEKING => Some(2),
        MEDIA_POSITION => Some(3),
        VIDEO_WINDOW => Some(4),
        BASIC_AUDIO => Some(5),
        _ => None,
    }
}

fn read_guid(memory: &GuestMemory, address: u32) -> Result<[u8; 16], MemoryError> {
    guest::check(memory, address, 16, Access::Read)?;
    let mut value = [0; 16];
    memory.read(u64::from(address), &mut value)?;
    Ok(value)
}

#[derive(Clone, Copy)]
pub(super) enum Call {
    Initialize,
    Uninitialize,
    CreateInstance,
    QueryInterface,
    AddRef,
    Release,
    RenderFile,
}

impl Call {
    pub(super) fn at(offset: u32) -> Option<Self> {
        match offset {
            0x474 => Some(Self::Initialize),
            0x478 => Some(Self::Uninitialize),
            0x47c => Some(Self::CreateInstance),
            0x480 => Some(Self::QueryInterface),
            0x484 => Some(Self::AddRef),
            0x488 => Some(Self::Release),
            0x48c => Some(Self::RenderFile),
            _ => None,
        }
    }

    pub(super) fn resolve(name: &str) -> Option<u32> {
        match name {
            "CoInitialize" => Some(0x474),
            "CoUninitialize" => Some(0x478),
            "CoCreateInstance" => Some(0x47c),
            _ => None,
        }
    }

    pub(super) fn arguments(self) -> usize {
        match self {
            Self::Initialize | Self::AddRef | Self::Release => 1,
            Self::Uninitialize => 0,
            Self::CreateInstance => 5,
            Self::QueryInterface | Self::RenderFile => 3,
        }
    }
}

#[derive(Default)]
pub(super) struct Com {
    references: u32,
    graphs: Vec<u32>,
    mapped: bool,
}

impl Com {
    fn initialize(memory: &mut GuestMemory) -> Result<(), MemoryError> {
        memory.map_zeroed(u64::from(BASE), PAGE_SIZE, Permissions::READ_WRITE)?;
        for id in 0..INTERFACES {
            let table = TABLE_BASE + id * TABLE_SLOTS * 4;
            for slot in 0..TABLE_SLOTS {
                let address = match (id, slot) {
                    (_, 0) => API_BASE + 0x480,
                    (_, 1) => API_BASE + 0x484,
                    (_, 2) => API_BASE + 0x488,
                    (0, 13) => API_BASE + 0x48c,
                    _ => API_BASE + 0xffc,
                };
                guest::write_word(memory, table + slot * 4, address)?;
            }
        }
        for graph in 0..u32::try_from(MAX_GRAPHS).expect("bounded graph count") {
            for id in 0..INTERFACES {
                guest::write_word(
                    memory,
                    OBJECT_BASE + graph * OBJECT_STRIDE + id * 4,
                    TABLE_BASE + id * TABLE_SLOTS * 4,
                )?;
            }
        }
        memory.protect(u64::from(BASE), PAGE_SIZE, Permissions::READ)
    }

    fn object(&self, pointer: u32) -> Result<usize, DispatchError> {
        let offset = pointer
            .checked_sub(OBJECT_BASE)
            .ok_or(DispatchError::Unsupported)?;
        let graph =
            usize::try_from(offset / OBJECT_STRIDE).map_err(|_| DispatchError::Unsupported)?;
        let interface = offset % OBJECT_STRIDE;
        if interface >= INTERFACES * 4
            || !interface.is_multiple_of(4)
            || self.graphs.get(graph).copied().unwrap_or(0) == 0
        {
            return Err(DispatchError::Unsupported);
        }
        Ok(graph)
    }

    fn create_instance(
        &mut self,
        args: &[u32],
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        let output = args[4];
        if output == 0 {
            return Ok(NULL_POINTER);
        }
        guest::write_word(memory, output, 0)?;
        if self.references == 0 {
            return Ok(NOT_INITIALIZED);
        }
        if args[0] == 0 || args[3] == 0 {
            return Ok(NULL_POINTER);
        }
        let class = read_guid(memory, args[0])?;
        let iid = read_guid(memory, args[3])?;
        if class != FILTER_GRAPH || args[2] != 1 {
            return Ok(CLASS_NOT_REGISTERED);
        }
        if args[1] != 0 {
            return Ok(CLASS_NO_AGGREGATION);
        }
        let Some(interface) = interface(iid) else {
            return Ok(NO_INTERFACE);
        };
        if self.graphs.len() == MAX_GRAPHS {
            return Ok(OUT_OF_MEMORY);
        }
        if !self.mapped {
            Self::initialize(memory)?;
            self.mapped = true;
        }
        let graph = u32::try_from(self.graphs.len()).expect("bounded graph count");
        let pointer = OBJECT_BASE + graph * OBJECT_STRIDE + interface * 4;
        guest::write_word(memory, output, pointer)?;
        self.graphs.push(1);
        Ok(0)
    }

    fn query_interface(
        &mut self,
        args: &[u32],
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        let graph = self.object(args[0])?;
        if args[2] == 0 || args[1] == 0 {
            return Ok(NULL_POINTER);
        }
        guest::write_word(memory, args[2], 0)?;
        let iid = read_guid(memory, args[1])?;
        let Some(interface) = interface(iid) else {
            return Ok(NO_INTERFACE);
        };
        let next = self.graphs[graph]
            .checked_add(1)
            .ok_or(DispatchError::Unsupported)?;
        let pointer = OBJECT_BASE
            + u32::try_from(graph).expect("bounded graph count") * OBJECT_STRIDE
            + interface * 4;
        guest::write_word(memory, args[2], pointer)?;
        self.graphs[graph] = next;
        Ok(0)
    }

    pub(super) fn dispatch(
        &mut self,
        call: Call,
        args: &[u32],
        memory: &mut GuestMemory,
    ) -> Result<Option<u32>, DispatchError> {
        match call {
            Call::Initialize => {
                if args[0] != 0 {
                    return Ok(Some(0x8007_0057));
                }
                let result = u32::from(self.references != 0);
                self.references = self
                    .references
                    .checked_add(1)
                    .ok_or(DispatchError::Unsupported)?;
                Ok(Some(result))
            }
            Call::Uninitialize => {
                self.references = self.references.saturating_sub(1);
                Ok(None)
            }
            Call::CreateInstance => self.create_instance(args, memory).map(Some),
            Call::QueryInterface => self.query_interface(args, memory).map(Some),
            Call::RenderFile => Err(DispatchError::Unsupported),
            Call::AddRef => {
                let graph = self.object(args[0])?;
                let next = self.graphs[graph]
                    .checked_add(1)
                    .ok_or(DispatchError::Unsupported)?;
                self.graphs[graph] = next;
                Ok(Some(next))
            }
            Call::Release => {
                let graph = self.object(args[0])?;
                self.graphs[graph] -= 1;
                Ok(Some(self.graphs[graph]))
            }
        }
    }
}
