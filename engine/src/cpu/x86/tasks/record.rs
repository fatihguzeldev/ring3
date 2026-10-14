use super::TaskReturnError;

/// the tss descriptor, not iret operand size, selects the record format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskFormat {
    Word,
    Dword,
}

impl TaskFormat {
    pub const fn record_bytes(self) -> usize {
        match self {
            Self::Word => 44,
            Self::Dword => 104,
        }
    }

    /// specialized intel task rules with cr4.cet=0; limits are inclusive.
    pub const fn minimum_limit(self) -> u32 {
        match self {
            Self::Word => 0x2c,
            Self::Dword => 0x67,
        }
    }
}

/// narrow task state stays narrow: no invented upper halves, fs/gs or cr3.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskState16 {
    pub ip: u16,
    pub flags: u16,
    /// ax, cx, dx, bx, sp, bp, si, di.
    pub general: [u16; 8],
    /// es, cs, ss, ds.
    pub selectors: [u16; 4],
    pub ldtr: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskState32 {
    pub eip: u32,
    pub flags: u32,
    /// eax, ecx, edx, ebx, esp, ebp, esi, edi.
    pub general: [u32; 8],
    /// es, cs, ss, ds, fs, gs.
    pub selectors: [u16; 6],
    pub ldtr: u16,
    pub cr3: u32,
    pub task_trap: bool,
    pub io_map_base: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskState {
    Word(TaskState16),
    Dword(TaskState32),
}

/// outgoing dynamic values; cr3, ldtr and all other static fields are not save inputs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskSave {
    pub next_eip: u32,
    pub flags: u32,
    /// eax, ecx, edx, ebx, esp, ebp, esi, edi.
    pub general: [u32; 8],
    /// es, cs, ss, ds, fs, gs.
    pub selectors: [u16; 6],
}

/// a checked borrowed record. extra caller-owned bytes remain part of the source image.
#[derive(Debug)]
pub struct TaskRecord<'a> {
    format: TaskFormat,
    bytes: &'a [u8],
}

impl<'a> TaskRecord<'a> {
    pub fn new(format: TaskFormat, bytes: &'a [u8]) -> Result<Self, TaskReturnError> {
        if bytes.len() < format.record_bytes() {
            return Err(TaskReturnError::MissingAuthority);
        }
        Ok(Self { format, bytes })
    }

    pub fn backlink(&self) -> u16 {
        read16(self.bytes, 0)
    }

    pub fn decode(&self) -> TaskState {
        match self.format {
            TaskFormat::Word => TaskState::Word(TaskState16 {
                ip: read16(self.bytes, 14),
                flags: read16(self.bytes, 16),
                general: core::array::from_fn(|index| read16(self.bytes, 18 + 2 * index)),
                selectors: core::array::from_fn(|index| read16(self.bytes, 34 + 2 * index)),
                ldtr: read16(self.bytes, 42),
            }),
            TaskFormat::Dword => TaskState::Dword(TaskState32 {
                eip: read32(self.bytes, 32),
                flags: read32(self.bytes, 36),
                general: core::array::from_fn(|index| read32(self.bytes, 40 + 4 * index)),
                selectors: core::array::from_fn(|index| read16(self.bytes, 72 + 4 * index)),
                ldtr: read16(self.bytes, 96),
                cr3: read32(self.bytes, 28),
                task_trap: read16(self.bytes, 100) & 1 != 0,
                io_map_base: read16(self.bytes, 102),
            }),
        }
    }
}

/// patch only outgoing dynamic fields, preserving static, reserved and extended bytes.
/// the caller supplies the original post-iret pc and instruction-start flags image.
/// this encodes a save image; it does not publish ram or commit a task switch.
pub fn encode_return(
    format: TaskFormat,
    save: &TaskSave,
    bytes: &mut [u8],
) -> Result<(), TaskReturnError> {
    if bytes.len() < format.record_bytes() {
        return Err(TaskReturnError::MissingAuthority);
    }
    let flags = save.flags & !0x4000;
    match format {
        TaskFormat::Word => {
            write16(bytes, 14, save.next_eip as u16);
            write16(bytes, 16, flags as u16);
            for (index, value) in save.general.iter().enumerate() {
                write16(bytes, 18 + 2 * index, *value as u16);
            }
            for (index, value) in save.selectors[..4].iter().enumerate() {
                write16(bytes, 34 + 2 * index, *value);
            }
        }
        TaskFormat::Dword => {
            write32(bytes, 32, save.next_eip);
            write32(bytes, 36, flags);
            for (index, value) in save.general.iter().enumerate() {
                write32(bytes, 40 + 4 * index, *value);
            }
            for (index, value) in save.selectors.iter().enumerate() {
                write16(bytes, 72 + 4 * index, *value);
            }
        }
    }
    Ok(())
}

fn read16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}
fn read32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}
fn write16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}
fn write32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests;
