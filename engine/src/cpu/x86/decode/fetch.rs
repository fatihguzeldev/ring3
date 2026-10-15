use crate::{
    cpu::x86::{
        CpuPagingError32, PagingAccess32, SegmentAccess, SegmentCache, SegmentContext,
        SegmentRegister, inspect_cpu_paging_access32,
    },
    memory::{Access, AddressSpace, CodeSnapshot, GuestAddress, MemoryError, PagingBacking32},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct CodeDecodeKey {
    pub(crate) cs_base: u32,
    pub(crate) default32: bool,
}

impl CodeDecodeKey {
    pub(crate) const FLAT: Self = Self {
        cs_base: 0,
        default32: true,
    };
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct CodeFetchContext {
    cs: SegmentCache,
    base: u32,
}

impl CodeFetchContext {
    pub(crate) fn from_segments(context: &SegmentContext) -> Option<Self> {
        context.is_valid().then_some(Self {
            cs: context.caches[SegmentRegister::Cs.index()],
            base: context.state.bases[SegmentRegister::Cs.index()],
        })
    }

    pub(crate) fn decode_key(self) -> CodeDecodeKey {
        CodeDecodeKey {
            cs_base: self.base,
            default32: self.cs.default32(),
        }
    }

    pub(crate) fn decoder_bitness(self) -> u32 {
        if self.cs.default32() { 32 } else { 16 }
    }

    pub(crate) fn linear_address(self, logical: GuestAddress) -> GuestAddress {
        GuestAddress(self.base.wrapping_add(logical.0))
    }

    pub(crate) fn fetch(
        self,
        memory: &AddressSpace,
        logical: GuestAddress,
        output: &mut [u8],
    ) -> Result<(), CpuPagingError32> {
        let linear = self.linear_address(logical);
        inspect_cpu_paging_access32(
            memory,
            linear,
            u32::try_from(output.len())
                .map_err(|_| CpuPagingError32::Memory(MemoryError::InvalidRange))?,
            PagingAccess32::Execute,
            PagingBacking32::Execute,
        )?;
        memory
            .fetch_code_wrapping(linear, output)
            .map_err(CpuPagingError32::Memory)
    }

    pub(crate) fn snapshot(
        self,
        memory: &AddressSpace,
        logical: GuestAddress,
        length: usize,
    ) -> Result<CodeSnapshot, MemoryError> {
        memory.snapshot_code_wrapping(self.linear_address(logical), length)
    }

    pub(crate) fn permits(self, pc: GuestAddress, length: u8) -> bool {
        self.cs
            .check_span(pc.0, u32::from(length), SegmentAccess::Execute)
            .is_ok()
    }
}

pub(crate) fn fetch_unbound(
    memory: &AddressSpace,
    pc: GuestAddress,
    output: &mut [u8],
) -> Result<(), CpuPagingError32> {
    // the standalone API retains its non-wrapping range priority before paging checks.
    if u64::from(pc.0) + output.len() as u64 > 1_u64 << 32 {
        memory
            .check_span(pc, output.len(), Access::Execute)
            .map_err(CpuPagingError32::Memory)?;
    }
    inspect_cpu_paging_access32(
        memory,
        pc,
        u32::try_from(output.len())
            .map_err(|_| CpuPagingError32::Memory(MemoryError::InvalidRange))?,
        PagingAccess32::Execute,
        PagingBacking32::Execute,
    )?;
    memory.fetch(pc, output).map_err(CpuPagingError32::Memory)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CapturedFetchFaultKind32 {
    Segment,
    Paging,
}

#[derive(Debug)]
pub(crate) struct CapturedCodeFault {
    pub(crate) kind: CapturedFetchFaultKind32,
    pub(crate) pc: GuestAddress,
    pub(crate) attempted_length: u8,
    pub(crate) consumed_prefix_length: u8,
    pub(crate) prefix: Option<CodeSnapshot>,
}

impl CapturedCodeFault {
    pub(crate) fn remains_denied(&self, memory: &AddressSpace, cs: CodeFetchContext) -> bool {
        if !cs.permits(self.pc, self.attempted_length) {
            return true;
        }
        match self.kind {
            CapturedFetchFaultKind32::Segment => false,
            CapturedFetchFaultKind32::Paging => {
                let mut bytes = [0; 15];
                cs.fetch(
                    memory,
                    self.pc,
                    &mut bytes[..usize::from(self.attempted_length)],
                )
                .is_err()
            }
        }
    }
}

#[derive(Debug)]
pub(crate) enum BoundDecode<I = super::DecodedOperation> {
    Instruction(I),
    CodeFault(CapturedCodeFault),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_key_contains_only_code_base_and_default_size() {
        let mut segments = SegmentContext::default();
        let flat = CodeFetchContext::from_segments(&segments).unwrap();
        assert_eq!(flat.decode_key(), CodeDecodeKey::FLAT);
        segments.state.selectors[1] = 0x43;
        segments.state.bases[3] = 0x9000;
        segments.caches[1] = SegmentCache::from_bits(0xf2b0_1234).unwrap();
        let limited = CodeFetchContext::from_segments(&segments).unwrap();
        assert_eq!(limited.decode_key(), flat.decode_key());
        assert!(!limited.permits(GuestAddress(0x1234), 2));
        segments.state.bases[1] = 0xffff_f000;
        let based = CodeFetchContext::from_segments(&segments).unwrap();
        assert_ne!(based.decode_key(), flat.decode_key());
        assert_eq!(
            based.linear_address(GuestAddress(0x2010)),
            GuestAddress(0x1010)
        );
        assert_eq!(
            based.linear_address(GuestAddress(0xfff)),
            GuestAddress(u32::MAX)
        );
        let word = CodeFetchContext {
            cs: SegmentCache::from_bits(0xf0b0_1234).unwrap(),
            base: based.base,
        };
        assert_ne!(word.decode_key(), based.decode_key());
        assert_eq!(word.decoder_bitness(), 16);
        segments.caches[1] = word.cs;
        let current_word = CodeFetchContext::from_segments(&segments).unwrap();
        assert_eq!(current_word.decode_key(), word.decode_key());
        assert_eq!(current_word.decoder_bitness(), 16);
    }
}

#[cfg(test)]
#[path = "fetch/paging.rs"]
mod paging_tests;
