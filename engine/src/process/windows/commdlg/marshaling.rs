use super::output::OutputPages32;
use crate::{
    memory::{AddressSpace, GuestAddress},
    process::windows::strings::{Input32, WindowsStringError32},
    windows::{
        commdlg::{ComdlgError32 as E, DialogError32, FileFilters32, OpenFileName32},
        text::CodePage32,
    },
};

pub(in crate::process::windows) struct CapturedFileDialog32 {
    pub pointer: u32,
    pub input: OpenFileName32,
    pub wide: bool,
    pub ansi: CodePage32,
    pub initial_file: Vec<u16>,
    pub initial_directory: Vec<u16>,
    pub title: Vec<u16>,
    pub default_extension: Vec<u16>,
    pub filters: FileFilters32,
    pub(super) outputs: OutputPages32,
}

impl CapturedFileDialog32 {
    pub(in crate::process::windows) fn check(&self, memory: &AddressSpace) -> Result<(), E> {
        self.outputs.check(memory)
    }

    pub(super) fn read(
        memory: &mut AddressSpace,
        pointer: u32,
        wide: bool,
        ansi: CodePage32,
        budget: usize,
    ) -> Result<Self, E> {
        let mut header = [0; 88];
        memory.read(GuestAddress(pointer), &mut header[..4])?;
        let size = u32::from_le_bytes(header[..4].try_into().unwrap()) as usize;
        if !matches!(size, 76 | 88) {
            return Err(E::Dialog(DialogError32::StructSize));
        }
        memory.read(GuestAddress(pointer), &mut header[..size])?;
        let input = OpenFileName32::parse(&header[..size])?;
        if input.file == 0 || input.file_capacity == 0 {
            return Err(E::InvalidRequest);
        }
        let mut remaining = budget;
        let initial_file = text(
            memory,
            input.file,
            wide,
            ansi,
            &mut remaining,
            input.file_capacity as usize,
        )?;
        let initial_directory =
            optional_text(memory, input.initial_directory, wide, ansi, &mut remaining)?;
        let title = optional_text(memory, input.title, wide, ansi, &mut remaining)?;
        let default_extension =
            optional_text(memory, input.default_extension, wide, ansi, &mut remaining)?;
        let units = if input.filter == 0 {
            vec![0]
        } else {
            multistring(memory, input.filter, wide, ansi, &mut remaining, None)?
        };
        let entries = FileFilters32::parse(&units, budget / 4)?;
        let custom = if input.custom_filter == 0 || input.custom_capacity == 0 {
            None
        } else {
            let mut units = multistring(
                memory,
                input.custom_filter,
                wide,
                ansi,
                &mut remaining,
                Some(input.custom_capacity as usize),
            )?;
            if units.first() == Some(&0) {
                None
            } else {
                units.try_reserve(1).map_err(|_| E::Allocation)?;
                units.push(0);
                let mut custom = FileFilters32::parse(&units, 1)?;
                custom.pop()
            }
        };
        let outputs = OutputPages32::capture(memory, pointer, input, wide)?;
        Ok(Self {
            pointer,
            input,
            wide,
            ansi,
            initial_file,
            initial_directory,
            title,
            default_extension,
            filters: FileFilters32 { entries, custom },
            outputs,
        })
    }
}

fn optional_text(
    memory: &AddressSpace,
    pointer: u32,
    wide: bool,
    ansi: CodePage32,
    remaining: &mut usize,
) -> Result<Vec<u16>, E> {
    if pointer == 0 {
        return Ok(Vec::new());
    }
    text(memory, pointer, wide, ansi, remaining, usize::MAX)
}

fn text(
    memory: &AddressSpace,
    pointer: u32,
    wide: bool,
    ansi: CodePage32,
    remaining: &mut usize,
    capacity: usize,
) -> Result<Vec<u16>, E> {
    let input = Input32::read(memory, pointer, wide, ansi, (*remaining / 2).min(capacity))
        .map_err(string_error)?;
    charge(remaining, input.units.len())?;
    Ok(input.units)
}

fn multistring(
    memory: &AddressSpace,
    pointer: u32,
    wide: bool,
    ansi: CodePage32,
    remaining: &mut usize,
    custom_capacity: Option<usize>,
) -> Result<Vec<u16>, E> {
    let width = if wide { 2 } else { 1 };
    let mut bytes = Vec::new();
    let mut zero = true;
    let mut terminators = 0;
    let limit = (*remaining / 2).min(custom_capacity.unwrap_or(usize::MAX));
    for index in 0..limit {
        let offset = index
            .checked_mul(width)
            .and_then(|value| u32::try_from(value).ok())
            .ok_or(E::Capacity)?;
        let address = pointer.checked_add(offset).ok_or(E::InvalidRequest)?;
        let mut unit = [0; 2];
        memory.read(GuestAddress(address), &mut unit[..width])?;
        bytes.try_reserve(width).map_err(|_| E::Allocation)?;
        bytes.extend_from_slice(&unit[..width]);
        let current_zero = unit[..width].iter().all(|&byte| byte == 0);
        if current_zero {
            terminators += 1;
        }
        if current_zero && (zero || custom_capacity.is_some() && terminators == 2) {
            let units = if wide {
                bytes
                    .chunks_exact(2)
                    .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                    .collect()
            } else {
                ansi.decode(&bytes, false).map_err(|_| E::InvalidRequest)?
            };
            charge(remaining, if wide { bytes.len() / 2 } else { bytes.len() })?;
            return Ok(units);
        }
        zero = current_zero;
    }
    Err(E::Capacity)
}

fn charge(remaining: &mut usize, units: usize) -> Result<(), E> {
    *remaining = remaining
        .checked_sub(units.checked_mul(2).ok_or(E::Capacity)?)
        .ok_or(E::Capacity)?;
    Ok(())
}

fn string_error(error: WindowsStringError32) -> E {
    match error {
        WindowsStringError32::Memory(error) => E::Memory(error),
        WindowsStringError32::Allocation => E::Allocation,
        WindowsStringError32::FilenameTooLong => E::Capacity,
        WindowsStringError32::InvalidParameter => E::InvalidRequest,
    }
}

#[cfg(test)]
mod tests;
