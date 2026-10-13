use super::{DecodeError, invalid, unsupported};
#[derive(Clone, Copy)]
pub(super) struct Encoding {
    w: bool,
    l: bool,
    vvvv: u8,
}
impl Encoding {
    pub(super) fn parse(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut offset = 0;
        while let Some(&byte) = bytes.get(offset) {
            match byte {
                0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65 | 0x67 => offset += 1,
                0x66 | 0xf0 | 0xf2 | 0xf3 => return Err(invalid()),
                _ => break,
            }
        }
        let (w, control) = match bytes.get(offset) {
            Some(0xc5) => (false, *bytes.get(offset + 1).ok_or_else(invalid)?),
            Some(0xc4) => {
                let map = *bytes.get(offset + 1).ok_or_else(invalid)?;
                if map & 0xc0 != 0xc0 || !matches!(map & 0x1f, 1..=3) {
                    return Err(invalid());
                }
                let control = *bytes.get(offset + 2).ok_or_else(invalid)?;
                (control & 0x80 != 0, control)
            }
            _ => return Err(invalid()),
        };
        if bytes[offset] == 0xc5 && control & 0xc0 != 0xc0 {
            return Err(invalid());
        }
        Ok(Self {
            w,
            l: control & 4 != 0,
            vvvv: (control >> 3) & 15,
        })
    }
    pub(super) fn validate_w(self, expected: bool) -> Result<(), DecodeError> {
        if self.w == expected {
            Ok(())
        } else {
            Err(invalid())
        }
    }

    pub(super) fn validate(
        self,
        strict_w: bool,
        l_policy: u8,
        unused_vvvv: bool,
    ) -> Result<(), DecodeError> {
        if strict_w && self.w || unused_vvvv && self.vvvv != 15 {
            return Err(invalid());
        }
        match l_policy {
            0 if self.l => Err(invalid()),
            1 if !self.l => Err(invalid()),
            3 if self.l => Err(unsupported()),
            _ => Ok(()),
        }
    }
}
