use super::ComdlgError32 as E;
use crate::windows::text::CodePage32;

pub(crate) struct FileSelection32 {
    pub bytes: Vec<u8>,
    pub units: u32,
    pub file_offset: u16,
    pub extension_offset: u16,
}

impl FileSelection32 {
    pub(crate) fn single(
        path: &[u16],
        wide: bool,
        ansi: CodePage32,
        limit: usize,
    ) -> Result<Self, E> {
        if path.is_empty() || path.contains(&0) {
            return Err(E::InvalidRequest);
        }
        let file = path
            .iter()
            .rposition(|unit| matches!(unit, 47 | 92))
            .map_or(0, |at| at + 1);
        let extension = path[file..]
            .iter()
            .rposition(|&unit| unit == 46)
            .map(|at| file + at + 1);
        let file_offset = offset(&path[..file], wide, ansi)?;
        let extension_offset = extension.map_or(Ok(0), |at| offset(&path[..at], wide, ansi))?;
        let mut bytes = Vec::new();
        append(&mut bytes, path, wide, ansi, limit)?;
        append(&mut bytes, &[0], wide, ansi, limit)?;
        Ok(Self {
            units: u32::try_from(bytes.len() / if wide { 2 } else { 1 })
                .map_err(|_| E::Capacity)?,
            bytes,
            file_offset,
            extension_offset,
        })
    }

    #[cfg(test)]
    pub(crate) fn multiple(
        directory: &[u16],
        names: &[Vec<u16>],
        wide: bool,
        ansi: CodePage32,
        limit: usize,
    ) -> Result<Self, E> {
        if directory.is_empty() || directory.contains(&0) || names.len() < 2 {
            return Err(E::InvalidRequest);
        }
        let file_offset = offset(directory, wide, ansi)?
            .checked_add(1)
            .ok_or(E::Capacity)?;
        let mut bytes = Vec::new();
        append(&mut bytes, directory, wide, ansi, limit)?;
        append(&mut bytes, &[0], wide, ansi, limit)?;
        for name in names {
            if name.is_empty() || name.iter().any(|unit| matches!(unit, 0 | 47 | 92)) {
                return Err(E::InvalidRequest);
            }
            append(&mut bytes, name, wide, ansi, limit)?;
            append(&mut bytes, &[0], wide, ansi, limit)?;
        }
        append(&mut bytes, &[0], wide, ansi, limit)?;
        Ok(Self {
            units: u32::try_from(bytes.len() / if wide { 2 } else { 1 })
                .map_err(|_| E::Capacity)?,
            bytes,
            file_offset,
            extension_offset: 0,
        })
    }
}

fn offset(units: &[u16], wide: bool, ansi: CodePage32) -> Result<u16, E> {
    let length = if wide {
        units.len()
    } else {
        ansi.encode(units, false)
            .map_err(|_| E::InvalidRequest)?
            .0
            .len()
    };
    u16::try_from(length).map_err(|_| E::Capacity)
}

fn append(
    bytes: &mut Vec<u8>,
    units: &[u16],
    wide: bool,
    ansi: CodePage32,
    limit: usize,
) -> Result<(), E> {
    if wide {
        let length = units.len().checked_mul(2).ok_or(E::Capacity)?;
        reserve(bytes, length, limit)?;
        bytes.extend(units.iter().flat_map(|unit| unit.to_le_bytes()));
    } else {
        let encoded = ansi.encode(units, false).map_err(|_| E::InvalidRequest)?.0;
        reserve(bytes, encoded.len(), limit)?;
        bytes.extend_from_slice(&encoded);
    }
    Ok(())
}

fn reserve(bytes: &mut Vec<u8>, length: usize, limit: usize) -> Result<(), E> {
    bytes
        .len()
        .checked_add(length)
        .filter(|&end| end <= limit)
        .ok_or(E::Capacity)?;
    bytes.try_reserve_exact(length).map_err(|_| E::Allocation)
}
