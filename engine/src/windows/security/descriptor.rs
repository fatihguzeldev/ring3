mod inheritance;

use super::{SecurityError32 as Error, Sid32, Token32};

const SELF_RELATIVE: u16 = 0x8000;
const DACL_PRESENT: u16 = 4;
const SACL_PRESENT: u16 = 16;
pub(crate) const MAX_DESCRIPTOR_BYTES: usize = 65_536;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AccessAce32 {
    pub kind: u8,
    pub flags: u8,
    pub mask: u32,
    pub sid: Sid32,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Acl32 {
    pub revision: u8,
    pub aces: Vec<AccessAce32>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Dacl32 {
    Absent,
    Null,
    Acl(Acl32),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SecurityDescriptor32 {
    pub owner: Option<Sid32>,
    pub group: Option<Sid32>,
    pub dacl: Dacl32,
    pub sacl: Dacl32,
    pub control: u16,
    pub resource_manager: u8,
}
impl Acl32 {
    fn parse(bytes: &[u8], audit: bool) -> Result<Self, Error> {
        if bytes.len() < 8 || !matches!(bytes[0], 2 | 4) {
            return Err(Error::InvalidAcl);
        }
        let length = usize::from(u16::from_le_bytes(bytes[2..4].try_into().unwrap()));
        let count = usize::from(u16::from_le_bytes(bytes[4..6].try_into().unwrap()));
        if length < 8 || length > bytes.len() || count > (length - 8) / 16 {
            return Err(Error::InvalidAcl);
        }
        let mut aces = Vec::new();
        aces.try_reserve_exact(count)
            .map_err(|_| Error::Allocation)?;
        let mut at = 8;
        for _ in 0..count {
            let header = bytes.get(at..at + 8).ok_or(Error::InvalidAcl)?;
            let size = usize::from(u16::from_le_bytes(header[2..4].try_into().unwrap()));
            if size < 16 || size & 3 != 0 || at + size > length {
                return Err(Error::InvalidAcl);
            }
            if !matches!(header[0], 0..=2) {
                return Err(Error::Unsupported);
            }
            if (header[0] == 2) != audit || header[1] & 0x20 != 0 {
                return Err(Error::InvalidAcl);
            }
            let sid = Sid32::parse(&bytes[at + 8..at + size])?;
            if sid.byte_len() + 8 != size {
                return Err(Error::InvalidAcl);
            }
            aces.push(AccessAce32 {
                kind: header[0],
                flags: header[1],
                mask: u32::from_le_bytes(header[4..8].try_into().unwrap()),
                sid,
            });
            at += size;
        }
        Ok(Self {
            revision: bytes[0],
            aces,
        })
    }
    fn byte_len(&self) -> Result<usize, Error> {
        let length = 8 + self
            .aces
            .iter()
            .map(|ace| 8 + ace.sid.byte_len())
            .sum::<usize>();
        if length > usize::from(u16::MAX) || self.aces.len() > usize::from(u16::MAX) {
            return Err(Error::Capacity);
        }
        Ok(length)
    }
    fn write(&self, output: &mut [u8]) -> Result<usize, Error> {
        let length = self.byte_len()?;
        let output = output.get_mut(..length).ok_or(Error::InvalidParameter)?;
        output.fill(0);
        output[0] = self.revision;
        output[2..4].copy_from_slice(&(length as u16).to_le_bytes());
        output[4..6].copy_from_slice(&(self.aces.len() as u16).to_le_bytes());
        let mut at = 8;
        for ace in &self.aces {
            let size = 8 + ace.sid.byte_len();
            output[at] = ace.kind;
            output[at + 1] = ace.flags;
            output[at + 2..at + 4].copy_from_slice(&(size as u16).to_le_bytes());
            output[at + 4..at + 8].copy_from_slice(&ace.mask.to_le_bytes());
            ace.sid.write(&mut output[at + 8..at + size])?;
            at += size;
        }
        Ok(length)
    }
}
impl SecurityDescriptor32 {
    pub(crate) fn snapshot(&self) -> Result<Self, Error> {
        let copy_acl = |source: &Dacl32| -> Result<Dacl32, Error> {
            Ok(match source {
                Dacl32::Absent => Dacl32::Absent,
                Dacl32::Null => Dacl32::Null,
                Dacl32::Acl(source) => {
                    let mut aces = Vec::new();
                    aces.try_reserve_exact(source.aces.len())
                        .map_err(|_| Error::Allocation)?;
                    aces.extend_from_slice(&source.aces);
                    Dacl32::Acl(Acl32 {
                        revision: source.revision,
                        aces,
                    })
                }
            })
        };
        Ok(Self {
            owner: self.owner,
            group: self.group,
            dacl: copy_acl(&self.dacl)?,
            sacl: copy_acl(&self.sacl)?,
            control: self.control,
            resource_manager: self.resource_manager,
        })
    }
    pub(crate) fn from_self_relative(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() < 20 || bytes.len() > MAX_DESCRIPTOR_BYTES || bytes[0] != 1 {
            return Err(Error::InvalidDescriptor);
        }
        let control = u16::from_le_bytes(bytes[2..4].try_into().unwrap());
        if control & SELF_RELATIVE == 0 {
            return Err(Error::InvalidDescriptor);
        }
        let component = |at: usize| -> Result<Option<&[u8]>, Error> {
            let offset = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
            if offset == 0 {
                return Ok(None);
            }
            if offset < 20 || offset & 3 != 0 {
                return Err(Error::InvalidDescriptor);
            }
            Ok(Some(bytes.get(offset..).ok_or(Error::InvalidDescriptor)?))
        };
        let acl = |at: usize, present: u16, audit: bool| -> Result<Dacl32, Error> {
            if control & present == 0 {
                return Ok(Dacl32::Absent);
            }
            Ok(match component(at)? {
                None => Dacl32::Null,
                Some(bytes) => Dacl32::Acl(Acl32::parse(bytes, audit)?),
            })
        };
        Ok(Self {
            owner: component(4)?.map(Sid32::parse).transpose()?,
            group: component(8)?.map(Sid32::parse).transpose()?,
            sacl: acl(12, SACL_PRESENT, true)?,
            dacl: acl(16, DACL_PRESENT, false)?,
            control,
            resource_manager: bytes[1],
        })
    }
    pub(crate) fn to_self_relative(&self) -> Result<Vec<u8>, Error> {
        let mut length = 20;
        for sid in [self.owner, self.group].into_iter().flatten() {
            length += sid.byte_len();
        }
        for acl in [&self.sacl, &self.dacl] {
            if let Dacl32::Acl(acl) = acl {
                length += acl.byte_len()?;
            }
        }
        if length > MAX_DESCRIPTOR_BYTES {
            return Err(Error::Capacity);
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(length)
            .map_err(|_| Error::Allocation)?;
        bytes.resize(length, 0);
        bytes[0] = 1;
        bytes[1] = self.resource_manager;
        let mut control = self.control | SELF_RELATIVE;
        control = (control & !(DACL_PRESENT | SACL_PRESENT))
            | if self.dacl != Dacl32::Absent {
                DACL_PRESENT
            } else {
                0
            }
            | if self.sacl != Dacl32::Absent {
                SACL_PRESENT
            } else {
                0
            };
        bytes[2..4].copy_from_slice(&control.to_le_bytes());
        let mut at = 20;
        for (field, sid) in [(4, self.owner), (8, self.group)] {
            if let Some(sid) = sid {
                bytes[field..field + 4].copy_from_slice(&(at as u32).to_le_bytes());
                at += sid.write(&mut bytes[at..])?;
            }
        }
        for (field, acl) in [(12, &self.sacl), (16, &self.dacl)] {
            if let Dacl32::Acl(acl) = acl {
                bytes[field..field + 4].copy_from_slice(&(at as u32).to_le_bytes());
                at += acl.write(&mut bytes[at..])?;
            }
        }
        Ok(bytes)
    }
    pub(crate) fn for_token(token: &Token32) -> Result<Self, Error> {
        let mut aces = Vec::new();
        aces.try_reserve_exact(2).map_err(|_| Error::Allocation)?;
        for sid in [token.user(), Sid32::system()] {
            aces.push(AccessAce32 {
                kind: 0,
                flags: 3,
                mask: 0x001f_01ff,
                sid,
            });
        }
        Ok(Self {
            owner: Some(token.user()),
            group: Some(token.primary_group()),
            dacl: Dacl32::Acl(Acl32 { revision: 2, aces }),
            sacl: Dacl32::Absent,
            control: SELF_RELATIVE | DACL_PRESENT,
            resource_manager: 0,
        })
    }
}
