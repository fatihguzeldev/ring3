use crate::{
    cpu::x86::{SegmentAccess, SegmentCache, SegmentContext, SegmentRegister},
    memory::{CodeSnapshot, GuestAddress},
};

#[derive(Clone, Copy, Debug)]
pub(crate) struct CodeFetchContext {
    cs: SegmentCache,
}

impl CodeFetchContext {
    pub(crate) fn from_segments(context: &SegmentContext) -> Option<Self> {
        context.is_valid().then_some(Self {
            cs: context.caches[SegmentRegister::Cs.index()],
        })
    }

    pub(crate) fn permits(self, pc: GuestAddress, length: u8) -> bool {
        self.cs
            .check_span(pc.0, u32::from(length), SegmentAccess::Execute)
            .is_ok()
    }
}

#[derive(Debug)]
pub(crate) struct CapturedCodeFault {
    pub(crate) pc: GuestAddress,
    pub(crate) attempted_length: u8,
    pub(crate) consumed_prefix_length: u8,
    pub(crate) prefix: Option<CodeSnapshot>,
}

#[derive(Debug)]
pub(crate) enum BoundDecode {
    Instruction(super::DecodedInstruction),
    CodeFault(CapturedCodeFault),
}
