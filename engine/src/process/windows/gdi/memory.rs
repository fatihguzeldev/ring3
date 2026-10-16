use crate::{
    memory::{AddressSpace, GuestAddress},
    windows::{
        gdi::GdiError32,
        user32::{Point32, Rect32},
    },
};
pub(super) fn zeroes(length: usize) -> Result<Vec<u8>, GdiError32> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| GdiError32::Allocation)?;
    bytes.resize(length, 0);
    Ok(bytes)
}
pub(super) fn words<const N: usize>(
    memory: &AddressSpace,
    address: u32,
) -> Result<[u32; N], GdiError32> {
    let mut bytes = zeroes(N * 4)?;
    memory.read(GuestAddress(address), &mut bytes)?;
    Ok(core::array::from_fn(|i| {
        u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
    }))
}
pub(super) fn rect(memory: &AddressSpace, address: u32) -> Result<Rect32, GdiError32> {
    let [left, top, right, bottom] = words(memory, address)?;
    Ok(Rect32 {
        left: left as i32,
        top: top as i32,
        right: right as i32,
        bottom: bottom as i32,
    })
}
pub(super) fn write_words(
    memory: &mut AddressSpace,
    address: u32,
    values: &[u32],
) -> Result<(), GdiError32> {
    let mut bytes = zeroes(values.len() * 4)?;
    for (part, value) in bytes.chunks_exact_mut(4).zip(values) {
        part.copy_from_slice(&value.to_le_bytes());
    }
    memory.write(GuestAddress(address), &bytes)?;
    Ok(())
}
pub(super) fn write_point(
    memory: &mut AddressSpace,
    address: u32,
    point: Point32,
) -> Result<(), GdiError32> {
    write_words(memory, address, &[point.x as u32, point.y as u32])
}
pub(super) fn write_rect(
    memory: &mut AddressSpace,
    address: u32,
    rect: Rect32,
) -> Result<(), GdiError32> {
    write_words(
        memory,
        address,
        &[
            rect.left as u32,
            rect.top as u32,
            rect.right as u32,
            rect.bottom as u32,
        ],
    )
}
