use crate::{
    abi::header,
    process::{CacheRequest32, CacheUsage32},
};

pub const CACHE_REQUEST_SIZE: usize = 64;
pub const CACHE_USAGE_SIZE: usize = 80;
pub fn encode_cache_request(
    request: Option<CacheRequest32>,
    output: &mut [u8; CACHE_REQUEST_SIZE],
) {
    header::write(output, *b"R3CQ");
    if let Some(request) = request {
        header::write_u32(output, 12, 1);
        u64_at(output, 16, request.serial);
        header::write_u32(output, 24, request.kind as u32);
        header::write_u32(output, 28, request.pc);
        u64_at(output, 32, request.unit_id);
        u64_at(output, 40, request.victim_id);
        header::write_u32(output, 48, request.slot);
        header::write_u32(output, 52, request.victim_slot);
        header::write_u32(output, 56, request.tier as u32);
    }
}
pub fn encode_cache_usage(usage: CacheUsage32, output: &mut [u8; CACHE_USAGE_SIZE]) {
    header::write(output, *b"R3CU");
    header::write_u32(output, 16, usage.units);
    header::write_u32(output, 20, usage.wasm_bytes);
    header::write_u32(output, 24, usage.installed);
    for (offset, value) in [
        (32, usage.hits),
        (40, usage.misses),
        (48, usage.evictions),
        (56, usage.promotions),
        (64, usage.pending_serial),
    ] {
        u64_at(output, offset, value);
    }
}
fn u64_at(output: &mut [u8], offset: usize, value: u64) {
    output[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn request_has_literal_offsets_and_absence_clears_every_field() {
        let request = CacheRequest32 {
            serial: 0x1122334455667788,
            kind: crate::process::CacheRequestKind32::Evict,
            pc: 0xaabbccdd,
            unit_id: 3,
            victim_id: 4,
            slot: 5,
            victim_slot: 6,
            tier: crate::cpu::dbt::CompilationTier32::Optimized,
        };
        let mut bytes = [0xff; 64];
        encode_cache_request(Some(request), &mut bytes);
        assert_eq!(
            &bytes[..16],
            &[82, 51, 67, 81, 1, 0, 1, 0, 64, 0, 0, 0, 1, 0, 0, 0]
        );
        assert_eq!(
            &bytes[16..32],
            &[
                0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11, 2, 0, 0, 0, 0xdd, 0xcc, 0xbb, 0xaa
            ]
        );
        assert_eq!(
            &bytes[32..],
            &[
                3, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 5, 0, 0, 0, 6, 0, 0, 0, 1, 0, 0, 0,
                0, 0, 0, 0
            ]
        );
        encode_cache_request(None, &mut bytes);
        assert_eq!(&bytes[12..], &[0; 52]);
    }
    #[test]
    fn usage_keeps_full_width_counters_and_reserved_words_zero() {
        let mut bytes = [0xff; 80];
        encode_cache_usage(
            CacheUsage32 {
                units: 7,
                wasm_bytes: 8,
                installed: 9,
                hits: 1 << 40,
                misses: 2 << 40,
                evictions: 3 << 40,
                promotions: 4 << 40,
                pending_serial: 5 << 40,
            },
            &mut bytes,
        );
        assert_eq!(
            &bytes[..16],
            &[82, 51, 67, 85, 1, 0, 1, 0, 80, 0, 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(&bytes[28..32], &[0; 4]);
        assert_eq!(&bytes[32..40], &(1u64 << 40).to_le_bytes());
        assert_eq!(&bytes[64..72], &(5u64 << 40).to_le_bytes());
        assert_eq!(&bytes[72..], &[0; 8]);
    }
}
