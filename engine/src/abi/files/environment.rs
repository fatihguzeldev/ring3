use super::*;
use crate::process::FileEnvironment32;

fn table(bytes: &[u8], magic: [u8; 4], payload: bool) -> Result<Vec<u32>, AbiError> {
    if bytes.len() < 32 {
        return Err(AbiError::Length);
    }
    header(bytes, magic, bytes.len())?;
    let count = word(bytes, 16) as usize;
    let total = word(bytes, 20) as usize;
    if !(4..=260).contains(&count) || total > 524288 {
        return Err(AbiError::Flags);
    }
    if bytes[24..32].iter().any(|b| *b != 0) {
        return Err(AbiError::Reserved);
    }
    let size = 32 + count * 4;
    if bytes.len() != size + if payload { total * 2 } else { 0 } {
        return Err(AbiError::Length);
    }
    let mut lengths = Vec::new();
    lengths
        .try_reserve_exact(count)
        .map_err(|_| AbiError::Length)?;
    for index in 0..count {
        let length = word(bytes, 32 + index * 4);
        if !(3..=32767).contains(&length) {
            return Err(AbiError::Flags);
        }
        lengths.push(length);
    }
    if lengths.iter().map(|n| *n as usize).sum::<usize>() != total {
        return Err(AbiError::Length);
    }
    Ok(lengths)
}
fn lengths(environment: &FileEnvironment32) -> Result<Vec<u32>, AbiError> {
    if environment.search_path.len() > 256 {
        return Err(AbiError::Flags);
    }
    let mut lengths = Vec::new();
    lengths
        .try_reserve_exact(4 + environment.search_path.len())
        .map_err(|_| AbiError::Length)?;
    for path in paths(environment) {
        if !(3..=32767).contains(&path.len()) || path.contains(&0) {
            return Err(AbiError::Flags);
        }
        lengths.push(path.len() as u32);
    }
    if lengths.iter().map(|n| *n as usize).sum::<usize>() > 524288 {
        return Err(AbiError::Flags);
    }
    Ok(lengths)
}
fn paths(environment: &FileEnvironment32) -> impl Iterator<Item = &[u16]> {
    [
        &environment.application_directory,
        &environment.windows_directory,
        &environment.system_directory,
        &environment.temp_directory,
    ]
    .into_iter()
    .chain(environment.search_path.iter())
    .map(Vec::as_slice)
}
fn write_table(
    lengths: &[u32],
    bytes: &mut [u8],
    magic: [u8; 4],
    payload: bool,
) -> Result<(), AbiError> {
    if !(4..=260).contains(&lengths.len()) || lengths.iter().any(|n| !(3..=32767).contains(n)) {
        return Err(AbiError::Flags);
    }
    let total = lengths.iter().map(|n| *n as usize).sum::<usize>();
    if total > 524288 {
        return Err(AbiError::Flags);
    }
    if bytes.len() != 32 + lengths.len() * 4 + if payload { total * 2 } else { 0 } {
        return Err(AbiError::Length);
    }
    initialize(bytes, magic);
    put(bytes, 16, lengths.len() as u32);
    put(bytes, 20, total as u32);
    for (index, &length) in lengths.iter().enumerate() {
        put(bytes, 32 + index * 4, length);
    }
    Ok(())
}
pub fn decode_file_environment(bytes: &[u8]) -> Result<FileEnvironment32, AbiError> {
    let lengths = table(bytes, *b"R3FE", true)?;
    let mut position = 32 + lengths.len() * 4;
    let mut paths = Vec::new();
    paths
        .try_reserve_exact(lengths.len())
        .map_err(|_| AbiError::Length)?;
    for length in lengths {
        let mut path = Vec::new();
        path.try_reserve_exact(length as usize)
            .map_err(|_| AbiError::Length)?;
        for _ in 0..length {
            let unit = u16::from_le_bytes(bytes[position..position + 2].try_into().unwrap());
            if unit == 0 {
                return Err(AbiError::Flags);
            }
            path.push(unit);
            position += 2;
        }
        paths.push(path);
    }
    let application_directory = paths.remove(0);
    let windows_directory = paths.remove(0);
    let system_directory = paths.remove(0);
    let temp_directory = paths.remove(0);
    Ok(FileEnvironment32 {
        application_directory,
        windows_directory,
        system_directory,
        temp_directory,
        search_path: paths,
    })
}
pub fn encode_file_environment(
    environment: &FileEnvironment32,
    bytes: &mut [u8],
) -> Result<(), AbiError> {
    let lengths = lengths(environment)?;
    write_table(&lengths, bytes, *b"R3FE", true)?;
    let mut offset = 32 + lengths.len() * 4;
    for path in paths(environment) {
        for unit in path {
            bytes[offset..offset + 2].copy_from_slice(&unit.to_le_bytes());
            offset += 2;
        }
    }
    Ok(())
}
pub fn decode_file_environment_start(bytes: &[u8]) -> Result<Vec<u32>, AbiError> {
    table(bytes, *b"R3EB", false)
}
pub fn encode_file_environment_start(lengths: &[u32], bytes: &mut [u8]) -> Result<(), AbiError> {
    write_table(lengths, bytes, *b"R3EB", false)
}
