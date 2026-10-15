use crate::{
    files::{FileAccess32, FileError32},
    memory::Permissions,
};

#[derive(Clone, Copy, Debug)]
pub(super) struct MappingProtection32 {
    pub read: bool,
    pub write: bool,
    pub copy: bool,
    pub execute: bool,
    pub reserve: bool,
}
impl MappingProtection32 {
    pub(super) fn parse(value: u32, pagefile: bool) -> Result<Self, FileError32> {
        let attributes = value & !0xff;
        if attributes & !(0x0800_0000 | 0x0400_0000) != 0 {
            return Err(FileError32::Unsupported);
        }
        if attributes & 0x0800_0000 != 0 && attributes & 0x0400_0000 != 0 {
            return Err(FileError32::InvalidParameter);
        }
        let reserve = pagefile && attributes & 0x0400_0000 != 0;
        let (read, write, copy, execute) = match value & 0xff {
            2 => (true, false, false, false),
            4 => (true, true, false, false),
            8 => (true, false, true, false),
            0x20 => (true, false, false, true),
            0x40 => (true, true, false, true),
            0x80 => (true, false, true, true),
            _ => return Err(FileError32::InvalidParameter),
        };
        Ok(Self {
            read,
            write,
            copy,
            execute,
            reserve,
        })
    }
    pub(super) fn file_access(self) -> FileAccess32 {
        FileAccess32(
            FileAccess32::READ.0
                | if self.write { FileAccess32::WRITE.0 } else { 0 }
                | if self.execute {
                    FileAccess32::EXECUTE.0
                } else {
                    0
                },
        )
    }
    pub(super) fn view(self, access: u32) -> Result<(Permissions, bool), FileError32> {
        let execute = access & 0x20 != 0;
        let access = access & !0x20;
        let (write, copy) = match access {
            1 => (false, true),
            2 | 4 | 6 | 0x000f_001f => (access != 4, false),
            _ => return Err(FileError32::InvalidParameter),
        };
        if !self.read
            || (write && !self.write)
            || (copy && !(self.read || self.copy))
            || (execute && !self.execute)
        {
            return Err(FileError32::AccessDenied);
        }
        let bits = 1 | if write || copy { 2 } else { 0 } | if execute { 4 } else { 0 };
        Ok((Permissions::from_bits(bits).unwrap(), copy))
    }
}
