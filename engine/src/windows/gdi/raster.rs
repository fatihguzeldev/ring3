pub fn raster_operation32(rop: u32, pattern: u32, source: u32, destination: u32) -> u32 {
    let table = (rop >> 16) as u8;
    let mut result = 0;
    for index in 0..8 {
        if table & (1 << index) != 0 {
            result |= (if index & 4 != 0 { pattern } else { !pattern })
                & (if index & 2 != 0 { source } else { !source })
                & (if index & 1 != 0 {
                    destination
                } else {
                    !destination
                });
        }
    }
    result
}
pub fn raster_uses_source32(rop: u32) -> bool {
    let table = (rop >> 16) as u8;
    (0..8).any(|index| (table >> index) & 1 != (table >> (index ^ 2)) & 1)
}
