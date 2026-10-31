use crate::cpu::x86::{SegmentContext, State32};

pub const ABI_VERSION: u32 = 1;
pub const ABI_SIZE: u32 = 160;
pub const NUMERIC_VIEW_VERSION: u32 = 1;
pub const NUMERIC_VIEW_SIZE: u32 = 36;

#[derive(Clone, Debug)]
#[repr(C)]
pub struct NumericView {
    pub version: u32,
    pub size: u32,
    pub x87_state: u32,
    pub selectors: [u32; 6],
}

impl Default for NumericView {
    fn default() -> Self {
        Self {
            version: NUMERIC_VIEW_VERSION,
            size: NUMERIC_VIEW_SIZE,
            x87_state: 0,
            selectors: [0; 6],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Status {
    Continue = 0,
    NeedCode = 1,
    Budget = 2,
    MemoryFault = 3,
    Unsupported = 4,
    Gate = 5,
    CodeInvalidated = 6,
    Instruction = 7,
    Infrastructure = 8,
    CpuFault = 9,
    SystemEntry = 10,
}

#[derive(Clone, Debug)]
#[repr(C, align(8))]
pub struct CpuState {
    pub version: u32,
    pub size: u32,
    pub registers: [u32; 8],
    pub eip: u32,
    pub eflags: u32,
    pub remaining: u32,
    pub status: u32,
    pub retired: u32,
    pub detail: u32,
    pub auxiliary: u32,
    pub vector: u32,
    pub read_result: u32,
    pub mappings: u32,
    pub permissions: u32,
    pub backing: u32,
    pub backing_pages: u32,
    pub executable_views: u32,
    pub private_slots: u32,
    pub dirty_bits: u32,
    pub dirty_slots: u32,
    pub dirty_len: u32,
    pub dirty_capacity: u32,
    pub fs_base: u32,
    pub gs_base: u32,
    pub numeric: u32,
    pub mapping_epoch: u64,
    pub code_epoch: u64,
    pub directory: u32,
    pub directory_mask: u32,
    pub segment_mask: u32,
    pub transaction_version: u32,
}

impl Default for CpuState {
    fn default() -> Self {
        Self {
            version: ABI_VERSION,
            size: ABI_SIZE,
            registers: [0; 8],
            eip: 0,
            eflags: 2,
            remaining: 0,
            status: 0,
            retired: 0,
            detail: 0,
            auxiliary: 0,
            vector: 0,
            read_result: 0,
            mappings: 0,
            permissions: 0,
            backing: 0,
            backing_pages: 0,
            executable_views: 0,
            private_slots: 0,
            dirty_bits: 0,
            dirty_slots: 0,
            dirty_len: 0,
            dirty_capacity: 0,
            fs_base: 0,
            gs_base: 0,
            numeric: 0,
            mapping_epoch: 0,
            code_epoch: 0,
            directory: 0,
            directory_mask: 0,
            segment_mask: 0x1f,
            transaction_version: 0,
        }
    }
}

pub fn supports_segments(segments: &SegmentContext) -> bool {
    segments.is_valid()
        && segments.is_legacy()
        && segments.caches[..4].iter().all(|cache| cache.usable())
}

impl CpuState {
    pub fn state(&self) -> State32 {
        State32 {
            registers: self.registers,
            eip: self.eip,
            eflags: self.eflags,
        }
    }

    pub fn set_state(&mut self, state: State32) {
        self.registers = state.registers;
        self.eip = state.eip;
        self.eflags = state.eflags | 2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cpu_layout_matches_generated_and_host_consumers() {
        assert_eq!(std::mem::size_of::<CpuState>(), ABI_SIZE as usize);
        assert_eq!(std::mem::offset_of!(CpuState, registers), 8);
        assert_eq!(std::mem::offset_of!(CpuState, eip), 40);
        assert_eq!(std::mem::offset_of!(CpuState, mappings), 76);
        assert_eq!(std::mem::offset_of!(CpuState, numeric), 124);
        assert_eq!(std::mem::offset_of!(CpuState, mapping_epoch), 128);
        assert_eq!(std::mem::offset_of!(CpuState, directory), 144);
        assert_eq!(std::mem::offset_of!(CpuState, segment_mask), 152);
        assert_eq!(std::mem::offset_of!(CpuState, transaction_version), 156);
    }
    #[test]
    fn numeric_view_binds_the_owned_x87_layout_without_exposing_numeric_state() {
        use crate::cpu::x86::X87State;

        assert_eq!(
            std::mem::size_of::<NumericView>(),
            NUMERIC_VIEW_SIZE as usize
        );
        assert_eq!(std::mem::offset_of!(NumericView, version), 0);
        assert_eq!(std::mem::offset_of!(NumericView, size), 4);
        assert_eq!(std::mem::offset_of!(NumericView, x87_state), 8);
        assert_eq!(std::mem::offset_of!(NumericView, selectors), 12);
        assert_eq!(std::mem::size_of::<X87State>(), 100);
        assert_eq!(std::mem::offset_of!(X87State, control), 0);
        assert_eq!(std::mem::offset_of!(X87State, status), 2);
        assert_eq!(std::mem::offset_of!(X87State, tag), 4);
        assert_eq!(std::mem::offset_of!(X87State, opcode), 6);
        assert_eq!(std::mem::offset_of!(X87State, instruction_pointer), 8);
        assert_eq!(std::mem::offset_of!(X87State, data_pointer), 12);
        assert_eq!(std::mem::offset_of!(X87State, code_selector), 16);
        assert_eq!(std::mem::offset_of!(X87State, data_selector), 18);
        assert_eq!(std::mem::offset_of!(X87State, registers), 20);
    }
    #[test]
    fn compiled_segment_profile_preserves_full_limits_and_tls_bases() {
        let mut segments = SegmentContext::default();
        assert!(supports_segments(&segments));
        segments.state.bases[4] = 0x7ff0_0000;
        assert!(supports_segments(&segments));
        segments.state.bases[3] = 0x1000;
        assert!(!supports_segments(&segments));
        segments.state.bases[3] = 0;
        segments.caches[4] = crate::cpu::x86::SegmentCache::UNUSABLE;
        assert!(!supports_segments(&segments));
    }
}
