use crate::windows::user32::User32Error32 as E;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Item32 {
    pub parent: Option<usize>,
    pub id: u32,
    pub kind: u32,
    pub state: u32,
    pub text: Vec<u16>,
    pub popup: bool,
    pub help: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Template32 {
    pub extended: bool,
    pub help: u32,
    pub items: Vec<Item32>,
    pub length: usize,
}

impl Template32 {
    pub(crate) fn read(
        read: impl FnMut(usize, &mut [u8]) -> Result<(), E>,
        byte_limit: usize,
        item_limit: usize,
        depth_limit: usize,
    ) -> Result<Self, E> {
        if item_limit == 0 || depth_limit == 0 {
            return Err(E::Capacity);
        }
        let mut input = Reader32 {
            read,
            at: 0,
            limit: byte_limit,
        };
        let version = input.word()?;
        if version > 1 {
            return Err(E::InvalidRequest);
        }
        let offset = usize::from(input.word()?);
        let help = if version == 1 { input.dword()? } else { 0 };
        let first = 4usize
            .checked_add(offset)
            .filter(|&first| first >= input.at && first <= byte_limit)
            .ok_or(E::InvalidRequest)?;
        input.at = first;
        let mut items = Vec::new();
        let mut parents = Vec::new();
        parents
            .try_reserve_exact(depth_limit)
            .map_err(|_| E::Allocation)?;
        parents.push((None, true));
        loop {
            if items.len() == item_limit {
                return Err(E::Capacity);
            }
            if version == 1 {
                input.align4()?;
            }
            let (id, kind, state, popup, last) = if version == 1 {
                let kind = input.dword()?;
                let state = input.dword()?;
                let id = input.dword()?;
                let flags = input.word()?;
                if flags & !0x81 != 0 {
                    return Err(E::InvalidRequest);
                }
                (id, kind, state, flags & 1 != 0, flags & 0x80 != 0)
            } else {
                let flags = input.word()?;
                let popup = flags & 0x10 != 0;
                let id = if popup { 0 } else { u32::from(input.word()?) };
                (
                    id,
                    u32::from(flags & !0x9b),
                    u32::from(flags & 0xb),
                    popup,
                    flags & 0x80 != 0,
                )
            };
            let text = input.text()?;
            let help = if version == 1 && popup {
                input.align4()?;
                input.dword()?
            } else {
                0
            };
            let index = items.len();
            let parent = parents.last().ok_or(E::InvalidRequest)?.0;
            items.try_reserve(1).map_err(|_| E::Allocation)?;
            items.push(Item32 {
                parent,
                id,
                kind,
                state,
                text,
                popup,
                help,
            });
            if popup {
                if parents.len() == depth_limit {
                    return Err(E::Capacity);
                }
                parents.push((Some(index), last));
            } else if last {
                let mut closed = parents.pop().ok_or(E::InvalidRequest)?;
                while closed.1 && !parents.is_empty() {
                    closed = parents.pop().unwrap();
                }
                if parents.is_empty() {
                    break;
                }
            }
        }
        Ok(Self {
            extended: version == 1,
            help,
            items,
            length: input.at,
        })
    }

    #[cfg(test)]
    pub(crate) fn parse(bytes: &[u8], item_limit: usize, depth_limit: usize) -> Result<Self, E> {
        Self::read(
            |offset, output| {
                let end = offset.checked_add(output.len()).ok_or(E::InvalidRequest)?;
                output.copy_from_slice(bytes.get(offset..end).ok_or(E::InvalidRequest)?);
                Ok(())
            },
            bytes.len(),
            item_limit,
            depth_limit,
        )
    }
}

struct Reader32<F> {
    read: F,
    at: usize,
    limit: usize,
}

impl<F: FnMut(usize, &mut [u8]) -> Result<(), E>> Reader32<F> {
    fn bytes<const N: usize>(&mut self) -> Result<[u8; N], E> {
        let end = self
            .at
            .checked_add(N)
            .filter(|&end| end <= self.limit)
            .ok_or(E::InvalidRequest)?;
        let mut bytes = [0; N];
        (self.read)(self.at, &mut bytes)?;
        self.at = end;
        Ok(bytes)
    }
    fn word(&mut self) -> Result<u16, E> {
        Ok(u16::from_le_bytes(self.bytes()?))
    }
    fn dword(&mut self) -> Result<u32, E> {
        Ok(u32::from_le_bytes(self.bytes()?))
    }
    fn align4(&mut self) -> Result<(), E> {
        self.at = self
            .at
            .checked_add(3)
            .map(|value| value & !3)
            .filter(|&at| at <= self.limit)
            .ok_or(E::InvalidRequest)?;
        Ok(())
    }
    fn text(&mut self) -> Result<Vec<u16>, E> {
        let mut text = Vec::new();
        loop {
            let word = self.word()?;
            if word == 0 {
                return Ok(text);
            }
            if text.len() == 32767 {
                return Err(E::Capacity);
            }
            text.try_reserve(1).map_err(|_| E::Allocation)?;
            text.push(word);
        }
    }
}

#[cfg(test)]
mod tests;
