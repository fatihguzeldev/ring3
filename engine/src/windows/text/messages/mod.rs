mod inserts;
mod layout;
mod resources;
mod system;

pub(crate) use resources::message_table_entry;
pub(crate) use system::system_message;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MessageError32 {
    InvalidParameter,
    MoreData,
    Allocation,
    Memory(crate::memory::MemoryError),
}
impl MessageError32 {
    pub(crate) fn win32(self) -> u32 {
        match self {
            Self::InvalidParameter => 87,
            Self::MoreData => 234,
            Self::Allocation => 8,
            Self::Memory(_) => 998,
        }
    }
}
pub(crate) trait MessageArguments32 {
    fn word(&self, index: usize) -> Result<u32, MessageError32>;
    fn character(&self, value: u16, wide: bool) -> Result<Vec<u16>, MessageError32>;
    fn string(
        &self,
        pointer: u32,
        wide: bool,
        precision: Option<usize>,
    ) -> Result<Vec<u16>, MessageError32>;
}
const MAX_UNITS: usize = 65535;
fn append(output: &mut Vec<u16>, units: &[u16]) -> Result<(), MessageError32> {
    if units.len() > MAX_UNITS.saturating_sub(output.len()) {
        return Err(MessageError32::MoreData);
    }
    output
        .try_reserve(units.len())
        .map_err(|_| MessageError32::Allocation)?;
    output.extend_from_slice(units);
    Ok(())
}

pub(crate) fn format_message(
    source: &[u16],
    flags: u32,
    wide: bool,
    arguments: &impl MessageArguments32,
) -> Result<Vec<u16>, MessageError32> {
    let mut output = Vec::new();
    let mut position = 0;
    let mut argument_bias = 0;
    while position < source.len() {
        let unit = source[position];
        position += 1;
        if unit == 13 || unit == 10 {
            if unit == 13 && source.get(position) == Some(&10) {
                position += 1;
            }
            append(
                &mut output,
                if flags & 255 == 0 { &[13, 10] } else { &[32] },
            )?;
        } else if unit == 37 {
            let next = *source
                .get(position)
                .ok_or(MessageError32::InvalidParameter)?;
            position += 1;
            match next {
                48 => break,
                110 => append(&mut output, &[13, 10])?,
                114 => append(&mut output, &[13])?,
                116 => append(&mut output, &[9])?,
                98 => append(&mut output, &[32])?,
                49..=57 => {
                    let begin = position - 2;
                    let mut index = usize::from(next - 48);
                    if let Some(&digit @ 48..=57) = source.get(position) {
                        index = index * 10 + usize::from(digit - 48);
                        position += 1;
                    }
                    let mut specification = &[][..];
                    if source.get(position) == Some(&33) {
                        let start = position + 1;
                        position = start;
                        while source.get(position).is_some_and(|&unit| unit != 33) {
                            position += 1;
                        }
                        if position == source.len() {
                            return Err(MessageError32::InvalidParameter);
                        }
                        specification = &source[start..position];
                        position += 1;
                    }
                    if flags & 0x200 != 0 {
                        append(&mut output, &source[begin..position])?;
                    } else {
                        let rendered = inserts::render(
                            specification,
                            index - 1 + argument_bias,
                            wide,
                            flags & 0x2000 != 0,
                            arguments,
                        )?;
                        append(&mut output, &rendered.units)?;
                        argument_bias += rendered.va_bias;
                    }
                }
                _ => {
                    if flags & 0x200 != 0 {
                        append(&mut output, &[37])?;
                    }
                    append(&mut output, &[next])?;
                }
            }
        } else {
            append(&mut output, &[unit])?;
        }
    }
    layout::wrap(&output, flags as u8)
}

#[cfg(test)]
mod tests;
