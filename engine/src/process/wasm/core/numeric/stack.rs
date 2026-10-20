use super::*;
use crate::{
    cpu::x86::{CpuMemoryPort32, SegmentRegister, StackFrameKind, StackPointerSize},
    process::cpu::stack,
};

impl NumericBank {
    pub(super) fn stack(
        &mut self,
        instance: &mut EngineInstance,
        context: &mut Context32,
        kind: StackFrameKind,
        allocation: u16,
        nesting: u8,
    ) -> NumericResult<()> {
        if nesting > 31
            || !matches!(kind, StackFrameKind::Enter16 | StackFrameKind::Enter32)
                && (allocation != 0 || nesting != 0)
        {
            return Err(Failure::Infrastructure);
        }
        let mut state = context.state();
        let width = kind.width() as usize;
        stack::check_segments(&self.segments, kind, allocation, nesting, state.registers)
            .map_err(|error| Failure::Memory(error, width))?;
        let mut port = memory::Port(instance.memory.as_mut().ok_or(Failure::Infrastructure)?);
        let prepared = stack::prepare(
            &mut port,
            self.segments.state.bases[SegmentRegister::Ss.index()],
            StackPointerSize::new(self.segments.caches[SegmentRegister::Ss.index()].default32()),
            kind,
            allocation,
            nesting,
            state.registers,
        )
        .map_err(|error| Failure::paging(error, width))?;
        if prepared.count != 0 {
            port.write_stack(
                &prepared.addresses[..prepared.count],
                &prepared.values[..prepared.count],
                width,
            )
            .map_err(|error| Failure::paging(error, width))?;
        }
        state.registers = prepared.registers;
        context.set_state(state);
        Ok(())
    }
}
