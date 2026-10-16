#[derive(Clone, Copy, Default)]
pub(crate) struct SystemColors32;

impl SystemColors32 {
    pub(crate) fn supports(index: u32) -> bool {
        matches!(index, 0..=24 | 26..=30)
    }
    pub(crate) fn get(self, index: u32) -> Option<u32> {
        const CLASSIC: [u32; 31] = [
            0xc0c0c0, 0x808000, 0x800000, 0x808080, 0xc0c0c0, 0xffffff, 0, 0, 0, 0xffffff,
            0xc0c0c0, 0xc0c0c0, 0x808080, 0x800000, 0xffffff, 0xc0c0c0, 0x808080, 0x808080, 0,
            0xc0c0c0, 0xffffff, 0x404040, 0xdfdfdf, 0, 0xe1ffff, 0, 0xff0000, 0xd08080, 0xc0c0c0,
            0x800000, 0xc0c0c0,
        ];
        Self::supports(index).then(|| CLASSIC[index as usize])
    }
}

#[cfg(test)]
mod tests {
    use super::SystemColors32;
    #[test]
    fn classic_profile_exposes_colorrefs_and_rejects_reserved_or_out_of_range_indices() {
        let colors = SystemColors32;
        assert_eq!(colors.get(5), Some(0xffffff));
        assert_eq!(colors.get(15), Some(0xc0c0c0));
        assert_eq!(colors.get(18), Some(0));
        assert_eq!(colors.get(20), Some(0xffffff));
        for index in [25, 31, u32::MAX] {
            assert_eq!(colors.get(index), None);
        }
    }
}
