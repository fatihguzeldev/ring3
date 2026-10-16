use crate::{
    memory::{Access, AddressSpace, GuestAddress},
    windows::{nls::NlsError32, text::CodePage32},
};
pub(super) fn bytes(
    memory: &AddressSpace,
    address: u32,
    count: i32,
    width: usize,
    limit: usize,
) -> Result<Vec<u8>, NlsError32> {
    if address == 0 || count == 0 || count < -1 {
        return Err(NlsError32::InvalidParameter);
    }
    let mut output = Vec::new();
    if count > 0 {
        let length = (count as usize)
            .checked_mul(width)
            .ok_or(NlsError32::InvalidParameter)?;
        if count as usize > limit {
            return Err(NlsError32::Capacity);
        }
        memory.check_span(GuestAddress(address), length, Access::Read)?;
        output
            .try_reserve_exact(length)
            .map_err(|_| NlsError32::Allocation)?;
        output.resize(length, 0);
        memory.read(GuestAddress(address), &mut output)?;
        return Ok(output);
    }
    output
        .try_reserve(limit.min(256) * width)
        .map_err(|_| NlsError32::Allocation)?;
    for at in 0..limit {
        let address = address
            .checked_add((at * width) as u32)
            .ok_or(NlsError32::InvalidParameter)?;
        let mut unit = [0; 2];
        memory.read(GuestAddress(address), &mut unit[..width])?;
        if output.len() + width > output.capacity() {
            output
                .try_reserve(width.max(output.capacity()))
                .map_err(|_| NlsError32::Allocation)?;
        }
        output.extend_from_slice(&unit[..width]);
        if unit[..width].iter().all(|&byte| byte == 0) {
            return Ok(output);
        }
    }
    Err(NlsError32::Capacity)
}
pub(super) fn prefix(
    memory: &AddressSpace,
    address: u32,
    width: usize,
    limit: usize,
) -> Result<Vec<u8>, NlsError32> {
    if limit != 0 {
        memory.check_span(GuestAddress(address), width, Access::Read)?;
    }
    let mut output = Vec::new();
    output
        .try_reserve_exact(limit.saturating_add(1).saturating_mul(width))
        .map_err(|_| NlsError32::Allocation)?;
    for index in 0..limit {
        let at = address
            .checked_add((index * width) as u32)
            .ok_or(NlsError32::InvalidParameter)?;
        let mut unit = [0; 2];
        memory.read(GuestAddress(at), &mut unit[..width])?;
        output.extend_from_slice(&unit[..width]);
        if unit[..width].iter().all(|&value| value == 0) {
            return Ok(output);
        }
    }
    output.extend_from_slice(&[0; 2][..width]);
    Ok(output)
}
pub(super) fn units(
    memory: &AddressSpace,
    address: u32,
    count: i32,
    wide: bool,
    cp: CodePage32,
    limit: usize,
) -> Result<Vec<u16>, NlsError32> {
    let bytes = bytes(memory, address, count, if wide { 2 } else { 1 }, limit)?;
    if wide {
        Ok(bytes
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes(b.try_into().unwrap()))
            .collect())
    } else {
        cp.decode(&bytes, false)
            .map_err(|_| NlsError32::NoTranslation)
    }
}
pub(super) fn encoded(units: &[u16], wide: bool, cp: CodePage32) -> Result<Vec<u8>, NlsError32> {
    if wide {
        let mut out = Vec::new();
        out.try_reserve_exact(units.len() * 2)
            .map_err(|_| NlsError32::Allocation)?;
        for unit in units {
            out.extend_from_slice(&unit.to_le_bytes());
        }
        Ok(out)
    } else {
        cp.encode_windows(units, 0, b'?')
            .map(|value| value.0)
            .map_err(conversion_error)
    }
}
pub(super) fn conversion_error(error: crate::windows::text::ConversionError32) -> NlsError32 {
    match error {
        crate::windows::text::ConversionError32::InvalidFlags => NlsError32::InvalidFlags,
        crate::windows::text::ConversionError32::NoTranslation => NlsError32::NoTranslation,
        crate::windows::text::ConversionError32::Allocation => NlsError32::Allocation,
    }
}
pub(super) fn output(
    memory: &mut AddressSpace,
    address: u32,
    capacity: i32,
    bytes: &[u8],
    width: usize,
) -> Result<u32, NlsError32> {
    if capacity < 0 {
        return Err(NlsError32::InvalidParameter);
    }
    let count = bytes.len() / width;
    if capacity == 0 {
        return Ok(count as u32);
    }
    if count > capacity as usize {
        return Err(NlsError32::InsufficientBuffer);
    }
    if address == 0 {
        return Err(NlsError32::InvalidParameter);
    }
    memory.write(GuestAddress(address), bytes)?;
    Ok(count as u32)
}
pub(super) fn fixed<const N: usize>(
    memory: &AddressSpace,
    address: u32,
) -> Result<[u8; N], NlsError32> {
    if address == 0 {
        return Err(NlsError32::InvalidParameter);
    }
    let mut bytes = [0; N];
    memory.read(GuestAddress(address), &mut bytes)?;
    Ok(bytes)
}
