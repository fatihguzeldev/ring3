use super::*;
use crate::cpu::x86::{SegmentContext, X87State};

#[derive(Clone, Copy)]
pub(super) struct X87Request {
    pub(super) operation: X87Operation,
    pub(super) address: u32,
    pub(super) pc: u32,
    pub(super) opcode: u16,
    pub(super) segment: SegmentRegister,
    pub(super) waiting: bool,
    pub(super) read_size: usize,
    pub(super) write_size: usize,
    pub(super) length: u32,
}

impl X87Request {
    pub(super) fn new(
        operation: X87Operation,
        address: u32,
        pc: u32,
        opcode: u32,
        control: u32,
    ) -> Result<Self, HostError> {
        let segment = (control >> 8) & 7;
        if opcode > 0x7ff || control & !0x701 != 0 || segment > 6 {
            return Err(HostError::InvalidRequest);
        }
        let read_size = operation.read_size();
        let write_size = operation.write_size();
        Ok(Self {
            operation,
            address,
            pc,
            opcode: opcode as u16,
            segment: if segment == 0 {
                SegmentRegister::Ds
            } else {
                [
                    SegmentRegister::Es,
                    SegmentRegister::Cs,
                    SegmentRegister::Ss,
                    SegmentRegister::Ds,
                    SegmentRegister::Fs,
                    SegmentRegister::Gs,
                ][(segment - 1) as usize]
            },
            waiting: control & 1 != 0,
            read_size,
            write_size,
            length: read_size.max(write_size) as u32,
        })
    }
}

pub(super) struct X87Step {
    pub(super) value: u32,
    pub(super) memory_len: usize,
    pub(super) result: Result<(), CpuMemoryError>,
    pub(super) publish_state: bool,
}

impl X87Step {
    fn fault(error: CpuMemoryError, publish_state: bool) -> Self {
        Self {
            value: 0,
            memory_len: 0,
            result: Err(error),
            publish_state,
        }
    }
}

impl EngineInstance {
    #[inline(always)]
    pub(super) fn x87_step(
        &mut self,
        state: &mut X87State,
        context: &mut Option<SegmentContext>,
        request: X87Request,
        flags: u32,
        output: &mut Option<[u8; 108]>,
    ) -> Result<X87Step, HostError> {
        if request.waiting && state.status & 0x3f & !state.control != 0 {
            state.status |= 0x8080;
            return Ok(X87Step::fault(
                CpuMemoryError::FloatingPoint(state.status),
                true,
            ));
        }
        // waiting x87 exceptions precede operand authority, including null selectors.
        if context.is_none() {
            *context = Some(self.operand_segment_context()?);
        }
        let context = context.as_ref().unwrap();
        let segments = context.state;
        let offset = request
            .address
            .wrapping_sub(segments.bases[request.segment.index()]);
        for (width, access) in [
            (request.read_size, SegmentAccess::Read),
            (request.write_size, SegmentAccess::Write),
        ] {
            if width != 0
                && let Err(error) = super::super::segments::check_cached_span(
                    context,
                    request.segment,
                    offset,
                    width as u32,
                    access,
                )
            {
                return Ok(X87Step::fault(error, false));
            }
        }
        let mut input = (request.read_size != 0).then(|| [0; 108]);
        if let Some(input) = &mut input
            && let Err(error) = self.cpu_memory_port32().read(
                GuestAddress(request.address),
                &mut input[..request.read_size],
            )
        {
            return Ok(X87Step::fault(CpuMemoryError::from(error), false));
        }
        if request.write_size != 0
            && let Err(error) = self.cpu_memory_port32().check(
                GuestAddress(request.address),
                request.write_size,
                PagingAccess32::Write,
            )
        {
            return Ok(X87Step::fault(CpuMemoryError::from(error), false));
        }
        let metadata = (
            state.instruction_pointer,
            state.opcode,
            state.code_selector,
            state.data_pointer,
            state.data_selector,
        );
        if request.operation.is_computation() {
            state.instruction_pointer = request.pc;
            state.opcode = request.opcode;
            state.code_selector = segments.selectors[SegmentRegister::Cs.index()];
            if request.length != 0 {
                state.data_pointer = offset;
                state.data_selector = segments.selectors[request.segment.index()];
            }
        }
        let output = if request.write_size != 0 {
            Some(output.get_or_insert_with(|| [0; 108]))
        } else {
            None
        };
        let outcome = match state.evaluate_into(
            request.operation,
            input
                .as_ref()
                .map_or(&[], |input| &input[..request.read_size]),
            flags,
            output,
            &mut self.numerical_workspace,
        ) {
            Ok(outcome) => outcome,
            Err(error) => {
                // a failed second step must retain the first committed fp prefix.
                (
                    state.instruction_pointer,
                    state.opcode,
                    state.code_selector,
                    state.data_pointer,
                    state.data_selector,
                ) = metadata;
                return Err(if error == X87Error::NumericalFailure {
                    HostError::Infrastructure
                } else {
                    HostError::InvalidRequest
                });
            }
        };
        *state = outcome.state;
        Ok(X87Step {
            value: outcome.eflags.unwrap_or(flags),
            memory_len: outcome.memory_len,
            result: Ok(()),
            publish_state: true,
        })
    }
}
