use super::super::User32Error32 as E;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DialogName32 {
    Empty,
    Ordinal(u16),
    Text(Vec<u16>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DialogUnits32 {
    pub x: i16,
    pub y: i16,
    pub width: i16,
    pub height: i16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DialogFont32 {
    pub points: u16,
    pub weight: u16,
    pub italic: bool,
    pub charset: u8,
    pub face: Vec<u16>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DialogItem32 {
    pub help: u32,
    pub style: u32,
    pub ex_style: u32,
    pub units: DialogUnits32,
    pub id: u32,
    pub class: DialogName32,
    pub title: DialogName32,
    pub creation: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DialogTemplate32 {
    pub extended: bool,
    pub help: u32,
    pub style: u32,
    pub ex_style: u32,
    pub units: DialogUnits32,
    pub menu: DialogName32,
    pub class: DialogName32,
    pub title: Vec<u16>,
    pub font: Option<DialogFont32>,
    pub items: Vec<DialogItem32>,
    pub length: usize,
}

impl DialogTemplate32 {
    pub(crate) fn read(
        read: impl FnMut(usize, &mut [u8]) -> Result<(), E>,
        byte_limit: usize,
        item_limit: usize,
    ) -> Result<Self, E> {
        let mut input = Input32 {
            read,
            at: 0,
            limit: byte_limit,
        };
        let first = input.word()?;
        let second = input.word()?;
        let extended = second == 0xffff;
        let (help, ex_style, style) = if extended {
            if first != 1 {
                return Err(E::InvalidRequest);
            }
            (input.dword()?, input.dword()?, input.dword()?)
        } else {
            (
                0,
                input.dword()?,
                u32::from(first) | (u32::from(second) << 16),
            )
        };
        let count = usize::from(input.word()?);
        if count > item_limit {
            return Err(E::Capacity);
        }
        let units = input.units()?;
        let menu = input.name()?;
        let class = input.name()?;
        let title = input.string()?;
        let font = if style & 0x40 != 0 {
            let points = input.word()?;
            let (weight, italic, charset) = if extended {
                let weight = input.word()?;
                let attributes = input.word()?;
                (weight, attributes as u8 != 0, (attributes >> 8) as u8)
            } else {
                (0, false, 0)
            };
            Some(DialogFont32 {
                points,
                weight,
                italic,
                charset,
                face: input.string()?,
            })
        } else {
            None
        };
        let mut items = Vec::new();
        items.try_reserve_exact(count).map_err(|_| E::Allocation)?;
        for _ in 0..count {
            input.align4()?;
            let (help, ex_style, style) = if extended {
                (input.dword()?, input.dword()?, input.dword()?)
            } else {
                let style = input.dword()?;
                (0, input.dword()?, style)
            };
            let units = input.units()?;
            let id = if extended {
                input.dword()?
            } else {
                u32::from(input.word()?)
            };
            let class = input.name()?;
            let title = input.name()?;
            let count = input.word()? as usize;
            let mut creation = Vec::new();
            if count != 0 {
                if !extended && count < 2 {
                    return Err(E::InvalidRequest);
                }
                creation
                    .try_reserve_exact(count + if extended { 2 } else { 0 })
                    .map_err(|_| E::Allocation)?;
                creation.extend_from_slice(&(count as u16).to_le_bytes());
                let payload = if extended { count } else { count - 2 };
                let at = creation.len();
                creation.resize(at + payload, 0);
                input.bytes(&mut creation[at..])?;
            }
            items.push(DialogItem32 {
                help,
                style,
                ex_style,
                units,
                id,
                class,
                title,
                creation,
            });
        }
        Ok(Self {
            extended,
            help,
            style,
            ex_style,
            units,
            menu,
            class,
            title,
            font,
            items,
            length: input.at,
        })
    }

    pub(crate) fn parse(bytes: &[u8], item_limit: usize) -> Result<Self, E> {
        Self::read(
            |at, output| {
                let length = output.len();
                output.copy_from_slice(bytes.get(at..at + length).ok_or(E::InvalidRequest)?);
                Ok(())
            },
            bytes.len(),
            item_limit,
        )
    }
}

struct Input32<F> {
    read: F,
    at: usize,
    limit: usize,
}

impl<F: FnMut(usize, &mut [u8]) -> Result<(), E>> Input32<F> {
    fn bytes(&mut self, output: &mut [u8]) -> Result<(), E> {
        let end = self
            .at
            .checked_add(output.len())
            .filter(|&end| end <= self.limit)
            .ok_or(E::InvalidRequest)?;
        (self.read)(self.at, output)?;
        self.at = end;
        Ok(())
    }
    fn word(&mut self) -> Result<u16, E> {
        let mut bytes = [0; 2];
        self.bytes(&mut bytes)?;
        Ok(u16::from_le_bytes(bytes))
    }
    fn dword(&mut self) -> Result<u32, E> {
        let mut bytes = [0; 4];
        self.bytes(&mut bytes)?;
        Ok(u32::from_le_bytes(bytes))
    }
    fn units(&mut self) -> Result<DialogUnits32, E> {
        Ok(DialogUnits32 {
            x: self.word()? as i16,
            y: self.word()? as i16,
            width: self.word()? as i16,
            height: self.word()? as i16,
        })
    }
    fn align4(&mut self) -> Result<(), E> {
        self.at = self
            .at
            .checked_add(3)
            .map(|at| at & !3)
            .filter(|&at| at <= self.limit)
            .ok_or(E::InvalidRequest)?;
        Ok(())
    }
    fn name(&mut self) -> Result<DialogName32, E> {
        Ok(match self.word()? {
            0 => DialogName32::Empty,
            0xffff => DialogName32::Ordinal(self.word()?),
            first => DialogName32::Text(self.string_tail(first)?),
        })
    }
    fn string(&mut self) -> Result<Vec<u16>, E> {
        let first = self.word()?;
        if first == 0 {
            Ok(Vec::new())
        } else {
            self.string_tail(first)
        }
    }
    fn string_tail(&mut self, first: u16) -> Result<Vec<u16>, E> {
        let mut text = Vec::new();
        text.try_reserve(1).map_err(|_| E::Allocation)?;
        text.push(first);
        loop {
            let unit = self.word()?;
            if unit == 0 {
                return Ok(text);
            }
            if text.len() == 32767 {
                return Err(E::InvalidRequest);
            }
            text.try_reserve(1).map_err(|_| E::Allocation)?;
            text.push(unit);
        }
    }
}
