use super::CodePage32;
impl CodePage32 {
    pub const fn maximum_char_size(self) -> u32 {
        match self {
            Self::Utf8 | Self::Legacy(54936) => 4,
            Self::Legacy(51932) => 3,
            Self::Legacy(932 | 936 | 949 | 950) => 2,
            _ => 1,
        }
    }
    pub const fn lead_bytes(self) -> [u8; 12] {
        match self {
            Self::Legacy(932) => [0x81, 0x9f, 0xe0, 0xfc, 0, 0, 0, 0, 0, 0, 0, 0],
            Self::Legacy(936 | 949 | 950) => [0x81, 0xfe, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            _ => [0; 12],
        }
    }
    pub fn is_lead_byte(self, byte: u8) -> bool {
        self.lead_bytes()
            .chunks_exact(2)
            .any(|range| range[0] != 0 && (range[0]..=range[1]).contains(&byte))
    }
    pub fn name(self) -> String {
        match self {
            Self::Windows1252 => "1252 (ANSI - Latin I)".into(),
            Self::Oem437 => "437 (OEM - United States)".into(),
            Self::Utf8 => "65001 (Unicode - UTF-8)".into(),
            Self::Legacy(id) => format!("{id} (Windows code page)"),
        }
    }
    pub fn encode_windows_default(
        self,
        units: &[u16],
        flags: u32,
        default: &[u8],
    ) -> Result<(Vec<u8>, bool), super::super::ConversionError32> {
        if let Self::Legacy(id) = self {
            super::legacy::encode_windows(id, units, flags, default)
        } else {
            self.encode_windows(units, flags, default.first().copied().unwrap_or(b'?'))
        }
    }
}
