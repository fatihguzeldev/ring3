//! fp sources and projection data for the shared recovery executor.
use super::program::{Error, Program, RefId, SourceContext, SourceOp, ValueKey};
use crate::cpu::core::{
    emitter::{Output, mem, numeric::pool},
    fp_pool as p,
    region::values::{
        FrameStateId, RegionPlan,
        executable::{
            ExecValueId,
            fp::{FpInput, FpPayload, FpRecovery, FpValueNode},
        },
    },
};
use wasm_encoder::{BlockType, ValType};

#[cfg(test)]
mod tests;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Payload {
    Native {
        value: RefId,
    },
    Opaque {
        low: RefId,
        high: RefId,
    },
    Tagged {
        valid: RefId,
        native: RefId,
        low: RefId,
        high: RefId,
    },
}

impl Payload {
    pub fn roots(&self) -> Vec<RefId> {
        match *self {
            Self::Native { value } => vec![value],
            Self::Opaque { low, high } => vec![low, high],
            Self::Tagged {
                valid,
                native,
                low,
                high,
            } => vec![valid, native, low, high],
        }
    }
    pub fn selected(&self, native_valid: bool) -> Vec<RefId> {
        match *self {
            Self::Tagged { native, .. } if native_valid => vec![native],
            Self::Tagged { low, high, .. } => vec![low, high],
            _ => self.roots(),
        }
    }
    fn key(&self) -> Vec<u64> {
        match *self {
            Self::Native { value } => vec![0, u64::from(value.0)],
            Self::Opaque { low, high } => vec![1, u64::from(low.0), u64::from(high.0)],
            Self::Tagged {
                valid,
                native,
                low,
                high,
            } => vec![
                2,
                u64::from(valid.0),
                u64::from(native.0),
                u64::from(low.0),
                u64::from(high.0),
            ],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Source {
    Input {
        field: FpInput,
        top: Option<RefId>,
    },
    PrecisionMode {
        control: RefId,
    },
    Certificate {
        logical: u8,
        top: RefId,
        control: RefId,
        valid: RefId,
    },
    Classify(Payload),
}

impl Source {
    pub fn dependencies(&self) -> Vec<RefId> {
        match self {
            Self::Input { top, .. } => top.iter().copied().collect(),
            Self::PrecisionMode { control } => vec![*control],
            Self::Certificate {
                top,
                control,
                valid,
                ..
            } => vec![*top, *control, *valid],
            Self::Classify(payload) => payload.roots(),
        }
    }
    pub fn key(&self) -> Vec<u64> {
        match self {
            Self::Input { field, top } => {
                let (kind, argument) = input_key(*field);
                vec![
                    0,
                    kind,
                    argument,
                    top.map_or(u64::MAX, |id| u64::from(id.0)),
                ]
            }
            Self::PrecisionMode { control } => vec![1, u64::from(control.0)],
            Self::Certificate {
                logical,
                top,
                control,
                valid,
            } => vec![
                2,
                u64::from(*logical),
                u64::from(top.0),
                u64::from(control.0),
                u64::from(valid.0),
            ],
            Self::Classify(payload) => {
                let mut key = vec![3];
                key.extend(payload.key());
                key
            }
        }
    }
}

fn input_key(field: FpInput) -> (u64, u64) {
    match field {
        FpInput::Native(i) => (0, u64::from(i)),
        FpInput::Low(i) => (1, u64::from(i)),
        FpInput::High(i) => (2, u64::from(i)),
        FpInput::Valid(i) => (3, u64::from(i)),
        FpInput::Tag(i) => (4, u64::from(i)),
        FpInput::Top => (5, 0),
        FpInput::Control => (6, 0),
        FpInput::Status => (7, 0),
        FpInput::Pending => (8, 0),
        FpInput::PendingLeft => (9, 0),
        FpInput::PendingRight => (10, 0),
        FpInput::PendingResult => (11, 0),
        FpInput::Fip => (12, 0),
        FpInput::Fdp => (13, 0),
        FpInput::Opcode => (14, 0),
        FpInput::Cs => (15, 0),
        FpInput::Ds => (16, 0),
        FpInput::Selector(segment) => (17, segment.index() as u64),
    }
}

fn payload(
    program: &mut Program,
    value: FpPayload,
    context: &SourceContext<'_>,
) -> Result<Payload, Error> {
    Ok(match value {
        FpPayload::Native { value, .. } => Payload::Native {
            value: program.value(value, context)?,
        },
        FpPayload::Opaque { low, high } => Payload::Opaque {
            low: program.value(low, context)?,
            high: program.value(high, context)?,
        },
        FpPayload::Tagged {
            valid,
            native,
            low,
            high,
            ..
        } => Payload::Tagged {
            valid: program.value(valid, context)?,
            native: program.value(native, context)?,
            low: program.value(low, context)?,
            high: program.value(high, context)?,
        },
    })
}

pub(super) fn source(
    program: &mut Program,
    id: ExecValueId,
    node: &FpValueNode,
    context: &SourceContext<'_>,
) -> Result<RefId, Error> {
    let color = program.color(id, context)?;
    let key = ValueKey::Executable(id);
    let op = match *node {
        FpValueNode::I32(value) => SourceOp::Constant {
            bits: u64::from(value),
        },
        FpValueNode::EffectResult { .. } => return Err(Error::UncapturedEffect(id)),
        FpValueNode::PrecisionMode { control } => SourceOp::Fp(Source::PrecisionMode {
            control: program.value(control, context)?,
        }),
        FpValueNode::InputCertificate {
            point,
            logical,
            control,
            valid,
        } => {
            if point.prefix != 0 {
                SourceOp::Constant { bits: 0 }
            } else {
                let top = context.executable.block(point.block).entry.fp.top;
                SourceOp::Fp(Source::Certificate {
                    logical,
                    top: program.value(top, context)?,
                    control: program.value(control, context)?,
                    valid: program.value(valid, context)?,
                })
            }
        }
        FpValueNode::Classify(value) => {
            SourceOp::Fp(Source::Classify(payload(program, value, context)?))
        }
        FpValueNode::Input { point, field } => {
            let needs_top = matches!(
                field,
                FpInput::Native(_)
                    | FpInput::Low(_)
                    | FpInput::High(_)
                    | FpInput::Valid(_)
                    | FpInput::Tag(_)
            );
            let top = if needs_top && point.prefix == 0 {
                Some(program.value(context.executable.block(point.block).entry.fp.top, context)?)
            } else {
                None
            };
            SourceOp::Fp(Source::Input { field, top })
        }
    };
    program.reference(key, color, op)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Cell {
    pub cell: u8,
    pub aliases: u8,
    pub payload: Payload,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Projection {
    pub frame: FrameStateId,
    pub top_delta: i8,
    pub written: u8,
    pub certificates: i64,
    pub cells: Vec<Cell>,
    pub header: [(u32, RefId); 12],
}

impl Projection {
    pub fn build(
        program: &mut Program,
        context: &SourceContext<'_>,
        recovery: &FpRecovery,
        frame: FrameStateId,
        plan: &RegionPlan,
    ) -> Result<Self, Error> {
        let shape = plan.shape(frame);
        let mut certificates = 0i64;
        let mut seen = 0u8;
        let mut cells = Vec::new();
        for logical in 0..8 {
            let cell = shape.cells[logical].0;
            if let FpPayload::Native { domain, .. } = recovery.payload[logical] {
                certificates |= (pool::FORMAT_TOKEN | i64::from(domain.certificate_bits()))
                    << (u32::from(cell) * pool::TOKEN_BITS);
            }
            if seen & (1 << cell) != 0 {
                continue;
            }
            seen |= 1 << cell;
            let aliases = (0..8)
                .filter(|&i| shape.cells[i].0 == cell)
                .fold(0u8, |mask, i| mask | (1 << i));
            debug_assert!(
                (0..8)
                    .filter(|&i| aliases & (1 << i) != 0)
                    .all(|i| recovery.payload[logical] == recovery.payload[i])
            );
            cells.push(Cell {
                cell,
                aliases,
                payload: payload(program, recovery.payload[logical], context)?,
            });
        }
        let fields = [
            (87, recovery.control),
            (88, recovery.status.bits),
            (90, recovery.top),
            (91, recovery.metadata.fip),
            (92, recovery.metadata.fdp),
            (93, recovery.metadata.opcode),
            (94, recovery.metadata.cs),
            (95, recovery.metadata.ds),
            (100, recovery.status.pending),
            (101, recovery.status.left),
            (102, recovery.status.right),
            (103, recovery.status.result),
        ];
        let mut header = [(0, RefId(0)); 12];
        for (target, (local, value)) in header.iter_mut().zip(fields) {
            *target = (local, program.value(value, context)?);
        }
        let projection = Self {
            frame,
            top_delta: recovery.top_delta,
            written: recovery.written,
            certificates,
            cells,
            header,
        };
        program.validate_roots(&projection.roots())?;
        Ok(projection)
    }
    pub fn roots(&self) -> Vec<RefId> {
        self.cells
            .iter()
            .flat_map(|cell| cell.payload.roots())
            .chain(self.header.iter().map(|&(_, value)| value))
            .collect()
    }
    pub fn dirty(&self, incoming: u8) -> u8 {
        incoming.rotate_right(u32::from(self.top_delta.rem_euclid(8) as u8)) | self.written
    }
    pub fn discriminators(&self, dirty: u8) -> Vec<RefId> {
        self.cells
            .iter()
            .filter(|cell| cell.aliases & dirty != 0)
            .filter_map(|cell| match cell.payload {
                Payload::Tagged { valid, .. } => Some(valid),
                _ => None,
            })
            .collect()
    }
    pub fn selected(&self, dirty: u8, mut valid: impl FnMut(RefId) -> bool) -> Vec<RefId> {
        self.cells
            .iter()
            .filter(|cell| cell.aliases & dirty != 0)
            .flat_map(|cell| {
                let native = match cell.payload {
                    Payload::Tagged { valid: id, .. } => valid(id),
                    _ => false,
                };
                cell.payload.selected(native)
            })
            .chain(self.header.iter().map(|&(_, value)| value))
            .collect()
    }
}

fn get(out: &mut Output<'_>, program: &Program, id: RefId) {
    out.c().local_get(program.get(id).color.local);
}

fn record(out: &mut Output<'_>, program: &Program, top: Option<RefId>, logical: u8) {
    out.c().local_get(pool::POINTER);
    if let Some(top) = top {
        get(out, program, top);
    } else {
        out.c().local_get(pool::POINTER).i32_load(mem(p::TOP, 2));
    }
    out.c()
        .i32_const(i32::from(logical))
        .i32_add()
        .i32_const(7)
        .i32_and()
        .i32_const(5)
        .i32_shl()
        .i32_add()
        .i32_const(p::RECORDS as i32)
        .i32_add()
        .local_set(39);
}

pub(super) fn emit_source(out: &mut Output<'_>, source: &Source, program: &Program) {
    match source {
        Source::PrecisionMode { control } => {
            get(out, program, *control);
            out.c().i32_const(0x300).i32_and().i32_const(0x200).i32_eq();
        }
        Source::Certificate {
            logical,
            top,
            control,
            valid,
        } => {
            get(out, program, *valid);
            get(out, program, *control);
            record(out, program, Some(*top), *logical);
            out.c()
                .local_get(39)
                .i32_load(mem(p::PROOF_CONTROL, 2))
                .i32_eq()
                .i32_and()
                .if_(BlockType::Result(ValType::I32))
                .local_get(39)
                .i32_load(mem(p::PROOFS, 2))
                .i32_const(15)
                .i32_and()
                .else_()
                .i32_const(0)
                .end();
        }
        Source::Classify(payload) => emit_classify(out, payload, program),
        Source::Input { field, top } => match *field {
            FpInput::Native(logical)
            | FpInput::Low(logical)
            | FpInput::High(logical)
            | FpInput::Valid(logical) => {
                record(out, program, *top, logical);
                out.c().local_get(39);
                match field {
                    FpInput::Native(_) => {
                        out.c().f64_load(mem(p::NATIVE, 3));
                    }
                    FpInput::Low(_) => {
                        out.c().i64_load(mem(p::LOW, 3));
                    }
                    FpInput::High(_) => {
                        out.c().i32_load(mem(p::HIGH, 2));
                    }
                    FpInput::Valid(_) => {
                        out.c().i32_load(mem(p::VALID, 2));
                    }
                    _ => unreachable!(),
                }
            }
            FpInput::Tag(logical) => {
                out.c().local_get(pool::POINTER).i32_load(mem(p::TAG, 2));
                if let Some(top) = top {
                    get(out, program, *top);
                } else {
                    out.c().local_get(pool::POINTER).i32_load(mem(p::TOP, 2));
                }
                out.c()
                    .i32_const(i32::from(logical))
                    .i32_add()
                    .i32_const(7)
                    .i32_and()
                    .i32_const(1)
                    .i32_shl()
                    .i32_shr_u()
                    .i32_const(3)
                    .i32_and();
            }
            FpInput::Selector(segment) => {
                out.c()
                    .local_get(96)
                    .i32_load16_u(mem(100 + segment.index() as u64 * 2, 1));
            }
            field => {
                let (offset, wide) = match field {
                    FpInput::Top => (p::TOP, false),
                    FpInput::Control => (p::CONTROL, false),
                    FpInput::Status => (p::STATUS, false),
                    FpInput::Pending => (p::PENDING, false),
                    FpInput::PendingLeft => (p::PENDING_LEFT, true),
                    FpInput::PendingRight => (p::PENDING_RIGHT, true),
                    FpInput::PendingResult => (p::PENDING_RESULT, true),
                    FpInput::Fip => (p::FIP, false),
                    FpInput::Fdp => (p::FDP, false),
                    FpInput::Opcode => (p::OPCODE, false),
                    FpInput::Cs => (p::CS, false),
                    FpInput::Ds => (p::DS, false),
                    _ => unreachable!(),
                };
                out.c().local_get(pool::POINTER);
                if wide {
                    out.c().f64_load(mem(offset, 3));
                } else {
                    out.c().i32_load(mem(offset, 2));
                }
            }
        },
    }
}

fn emit_raw_class(out: &mut Output<'_>, low: RefId, high: RefId, program: &Program) {
    get(out, program, low);
    out.c().i64_eqz();
    get(out, program, high);
    out.c()
        .i32_const(0x7fff)
        .i32_and()
        .i32_eqz()
        .i32_and()
        .if_(BlockType::Result(ValType::I32))
        .i32_const(1)
        .else_();
    get(out, program, high);
    out.c()
        .i32_const(0x7fff)
        .i32_and()
        .i32_const(1)
        .i32_sub()
        .i32_const(0x7ffe)
        .i32_lt_u();
    get(out, program, low);
    out.c()
        .i64_const(0)
        .i64_lt_s()
        .i32_and()
        .if_(BlockType::Result(ValType::I32))
        .i32_const(0)
        .else_()
        .i32_const(2)
        .end()
        .end();
}

fn emit_native_class(out: &mut Output<'_>, value: RefId, program: &Program) {
    get(out, program, value);
    out.c()
        .f64_const(0.0.into())
        .f64_eq()
        .if_(BlockType::Result(ValType::I32))
        .i32_const(1)
        .else_()
        .i32_const(0)
        .end();
}

fn emit_classify(out: &mut Output<'_>, payload: &Payload, program: &Program) {
    match *payload {
        Payload::Native { value } => emit_native_class(out, value, program),
        Payload::Opaque { low, high } => emit_raw_class(out, low, high, program),
        Payload::Tagged {
            valid,
            native,
            low,
            high,
        } => {
            get(out, program, valid);
            out.c().if_(BlockType::Result(ValType::I32));
            emit_native_class(out, native, program);
            out.c().else_();
            emit_raw_class(out, low, high, program);
            out.c().end();
        }
    }
}
