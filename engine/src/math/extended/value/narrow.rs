pub(super) fn divide(a: u64, b: u64, control: u16) -> Option<u128> {
    debug_assert!(a >> 63 != 0 && b >> 63 != 0);
    if control & 0x0300 != 0 || (a | b) & ((1_u64 << 40) - 1) != 0 {
        return None;
    }
    let numerator = (a >> 40) << (26 + u32::from(a < b));
    let divisor = b >> 40;
    let quotient = numerator / divisor;
    let sticky = numerator != quotient * divisor;
    Some(u128::from(quotient | u64::from(sticky)) << 40)
}
