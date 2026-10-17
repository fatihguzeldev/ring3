use super::{ComdlgError32 as E, DialogError32};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OpenFileName32 {
    pub size: u32,
    pub owner: u32,
    pub instance: u32,
    pub filter: u32,
    pub custom_filter: u32,
    pub custom_capacity: u32,
    pub filter_index: u32,
    pub file: u32,
    pub file_capacity: u32,
    pub file_title: u32,
    pub title_capacity: u32,
    pub initial_directory: u32,
    pub title: u32,
    pub flags: u32,
    pub file_offset: u16,
    pub extension_offset: u16,
    pub default_extension: u32,
    pub data: u32,
    pub hook: u32,
    pub template: u32,
    pub reserved: u32,
    pub reserved_word: u32,
    pub flags_ex: u32,
}

impl OpenFileName32 {
    pub(crate) fn parse(bytes: &[u8]) -> Result<Self, E> {
        if bytes.len() < 4 {
            return Err(E::InvalidRequest);
        }
        let size = u32::from_le_bytes(bytes[..4].try_into().unwrap());
        if !matches!(size, 76 | 88) {
            return Err(E::Dialog(DialogError32::StructSize));
        }
        if bytes.len() != size as usize {
            return Err(E::InvalidRequest);
        }
        let word = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        Ok(Self {
            size,
            owner: word(4),
            instance: word(8),
            filter: word(12),
            custom_filter: word(16),
            custom_capacity: word(20),
            filter_index: word(24),
            file: word(28),
            file_capacity: word(32),
            file_title: word(36),
            title_capacity: word(40),
            initial_directory: word(44),
            title: word(48),
            flags: word(52),
            file_offset: u16::from_le_bytes(bytes[56..58].try_into().unwrap()),
            extension_offset: u16::from_le_bytes(bytes[58..60].try_into().unwrap()),
            default_extension: word(60),
            data: word(64),
            hook: word(68),
            template: word(72),
            reserved: if size == 88 { word(76) } else { 0 },
            reserved_word: if size == 88 { word(80) } else { 0 },
            flags_ex: if size == 88 { word(84) } else { 0 },
        })
    }

    pub(crate) fn flag(self, flag: u32) -> bool {
        self.flags & flag != 0
    }
}
