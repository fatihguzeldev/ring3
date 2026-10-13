pub(crate) const XCR0: u64 = 7;

/// baseline avx is admitted only with its complete architectural state enabled.
pub(crate) const AVX_ENABLED: bool = XCR0 & 7 == 7;

// activation follows complete guest-path verification independently of baseline avx.
pub(crate) const F16C_ENABLED: bool = AVX_ENABLED;

pub(crate) struct CpuIdLeaf {
    pub function: u32,
    pub subfunction: Option<u32>,
    pub registers: [u32; 4],
}

pub(crate) const CPU_ID_LEAVES: [CpuIdLeaf; 8] = [
    CpuIdLeaf {
        function: 0,
        subfunction: None,
        registers: [
            13,
            u32::from_le_bytes(*b"Ring"),
            u32::from_le_bytes(*b"ine "),
            u32::from_le_bytes(*b"3Eng"),
        ],
    },
    CpuIdLeaf {
        function: 1,
        subfunction: None,
        registers: [
            0x600,
            8 << 8,
            1 | 1 << 1
                | 1 << 9
                | 1 << 19
                | 1 << 20
                | 1 << 22
                | 1 << 23
                | 1 << 25
                | 1 << 26
                | 1 << 27
                | ((AVX_ENABLED as u32) << 28)
                | ((F16C_ENABLED as u32) << 29),
            1 | 1 << 4 | 1 << 8 | 1 << 15 | 1 << 19 | 1 << 23 | 1 << 24 | 1 << 25 | 1 << 26,
        ],
    },
    CpuIdLeaf {
        function: 7,
        subfunction: Some(0),
        registers: [0, 1 << 3 | 1 << 8, 0, 0],
    },
    CpuIdLeaf {
        function: 13,
        subfunction: Some(0),
        registers: [XCR0 as u32, 832, 832, (XCR0 >> 32) as u32],
    },
    CpuIdLeaf {
        function: 13,
        subfunction: Some(1),
        registers: [0; 4],
    },
    CpuIdLeaf {
        function: 13,
        subfunction: Some(2),
        // the architectural component includes ymm8..15 slots even outside 64-bit mode.
        registers: [256, 576, 0, 0],
    },
    CpuIdLeaf {
        function: 0x8000_0000,
        subfunction: None,
        registers: [0x8000_0001, 0, 0, 0],
    },
    CpuIdLeaf {
        function: 0x8000_0001,
        subfunction: None,
        registers: [0, 0, 1 << 5, 1 << 27],
    },
];

#[cfg(test)]
#[path = "capabilities/native.rs"]
mod native;
