//! One cold, same-body projection program. Request sites retain only a selector.
mod data;
mod executor;
pub(super) mod fp;
mod leaves;
mod lists;
pub(super) mod program;

use super::Output;
use crate::cpu::core::region::values::{
    FrameStateId,
    executable::{Recovery, fp::FpRecovery},
};
use program::{KernelMasks, Program, RefId, ScalarProjection, SourceContext};
use std::collections::BTreeMap;

// Header words followed by pairs (source marker/color, destination/gate).
const ALWAYS: u64 = 0;
const ACTIVE: u64 = 1;
const NATIVE: u64 = 2;
const RAW: u64 = 3;
const PHASE_ONE: u64 = 1 << 18;
const NO_DEST: u32 = 0;

#[derive(Clone, Copy)]
pub(super) enum FrameAction {
    None,
    Clear,
    Select(FrameStateId),
}

#[derive(Clone, Copy, Default)]
pub(super) struct ExitAction {
    pub pc: Option<u32>,
    pub retired: u32,
}

#[derive(Default)]
struct Header {
    list: u32,
    fp_info: u32,
    frame: u32,
    certificates: i64,
    instruction: Option<(u32, u16)>,
    segment_plus_one: u8,
    kind_plus_one: u32,
    width: u32,
    exit: ExitAction,
}

impl Header {
    fn words(&self) -> Vec<i64> {
        fn pair(low: u32, high: u32) -> i64 {
            (u64::from(low) | u64::from(high) << 32) as i64
        }
        assert!(
            self.segment_plus_one <= 7,
            "bounded recovery segment selector"
        );
        let metadata = self.instruction.map_or(0, |(pc, opcode)| {
            u64::from(pc) | u64::from(opcode) << 32 | 1 << 48
        }) | u64::from(self.segment_plus_one) << 49;
        vec![
            i64::from(self.list),
            pair(
                self.fp_info | u32::from(self.exit.pc.is_some()) << 17,
                self.frame,
            ),
            self.certificates,
            metadata as i64,
            pair(self.kind_plus_one, self.width),
            pair(self.exit.retired, self.exit.pc.unwrap_or(0)),
        ]
    }
}

pub(super) struct State {
    pub program: Program,
    pub words: Vec<i64>,
    sites: BTreeMap<Vec<i64>, u32>,
    row_data: Vec<[i64; 2]>,
    row_ids: BTreeMap<[i64; 2], u32>,
    lists: lists::ListPool,
    relocations: Vec<usize>,
    leaf_base: u32,
    frozen: bool,
    pub locals: Locals,
}

#[derive(Clone, Copy)]
pub(super) struct Locals {
    pub base: u32,
    pub colors: u32,
    pub expression_base: u32,
}

impl Locals {
    pub fn request(self) -> u32 {
        self.base
    }
    pub fn at(self, offset: u32) -> u32 {
        self.base + offset
    }
    pub fn need(self, color: u32) -> u32 {
        self.base + 16 + color - self.expression_base
    }
    pub fn wide(self, offset: u32) -> u32 {
        self.base + 16 + self.colors + offset
    }
    pub fn float(self) -> u32 {
        self.wide(3)
    }
}

impl State {
    pub fn new(limit: usize, locals: Locals) -> Self {
        Self {
            program: Program::new(limit),
            words: Vec::new(),
            sites: BTreeMap::new(),
            row_data: Vec::new(),
            row_ids: BTreeMap::new(),
            lists: lists::ListPool::default(),
            relocations: Vec::new(),
            leaf_base: 0,
            frozen: false,
            locals,
        }
    }
    fn row(&self, rows: &mut Vec<i64>, id: RefId, destination: u32, gate: u64) {
        let source = self.program.get(id);
        let color = source.color.local - self.locals.expression_base;
        let marker = if matches!(source.op, program::SourceOp::Available) {
            (-(id.0 as i32 + 1)) as u32
        } else {
            id.0 + 1
        };
        rows.push((u64::from(marker) | u64::from(color) << 32) as i64);
        rows.push((u64::from(destination) | gate) as i64);
    }
    fn scalar_rows(&self, rows: &mut Vec<i64>, scalar: &ScalarProjection) {
        for (r, id) in scalar.gpr.iter().enumerate() {
            if let Some(id) = id {
                self.row(rows, *id, r as u32 + 2, ALWAYS);
            }
        }
        if let Some(flags) = &scalar.flags {
            let targets = [
                super::FLAGS,
                super::KIND,
                super::WIDTH,
                super::LEFT,
                super::RIGHT,
                super::VALUE,
                super::CARRY_IN,
            ];
            for (id, target) in flags.origin.into_iter().zip(targets) {
                self.row(rows, id, target, ALWAYS);
            }
            if let Some(recipe) = &flags.arithmetic {
                for (id, target) in [
                    (recipe.left, super::LEFT),
                    (recipe.right, super::RIGHT),
                    (recipe.result, super::VALUE),
                ] {
                    self.row(rows, id, target, ALWAYS);
                }
            }
        }
    }
    fn fp_rows(&self, rows: &mut Vec<i64>, fp: &fp::Projection) {
        for &(target, id) in &fp.header {
            self.row(rows, id, target, ACTIVE << 16);
        }
        for cell in &fp.cells {
            let gate = ACTIVE << 16 | u64::from(cell.aliases) << 24;
            match cell.payload {
                fp::Payload::Native { value } => {
                    self.row(rows, value, 76 + u32::from(cell.cell), gate | PHASE_ONE)
                }
                fp::Payload::Opaque { low, high } => {
                    self.row(rows, low, 110 + u32::from(cell.cell), gate | PHASE_ONE);
                    self.row(rows, high, 118 + u32::from(cell.cell), gate | PHASE_ONE);
                }
                fp::Payload::Tagged {
                    valid,
                    native,
                    low,
                    high,
                } => {
                    self.row(rows, valid, NO_DEST, gate);
                    let valid_color =
                        self.program.get(valid).color.local - self.locals.expression_base + 1;
                    let common =
                        u64::from(cell.aliases) << 24 | u64::from(valid_color) << 32 | PHASE_ONE;
                    self.row(
                        rows,
                        native,
                        76 + u32::from(cell.cell),
                        common | NATIVE << 16,
                    );
                    self.row(rows, low, 110 + u32::from(cell.cell), common | RAW << 16);
                    self.row(rows, high, 118 + u32::from(cell.cell), common | RAW << 16);
                }
            }
        }
    }
    fn freeze_data(&mut self) {
        assert!(
            !self.frozen,
            "recovery descriptors are finalized exactly once"
        );
        self.frozen = true;
        if self.words.is_empty() {
            return;
        }
        let base = self.words.len() as u32;
        self.words.extend(self.row_data.iter().flatten().copied());
        let lists = self.lists.append(&mut self.words, base);
        for &index in &self.relocations {
            self.words[index] = i64::from(lists[self.words[index] as usize]);
        }
        self.leaf_base = leaves::append(&self.program, &mut self.words);
    }
    pub fn finish_data(&mut self) -> Vec<i64> {
        assert!(
            self.frozen,
            "recovery descriptors are frozen before executor emission"
        );
        std::mem::take(&mut self.words)
    }
}

pub(super) fn request(
    out: &mut Output<'_>,
    scalar: Option<&Recovery>,
    fp: Option<&FpRecovery>,
    frame: FrameAction,
    kernel: KernelMasks,
    exit: ExitAction,
) {
    let context = SourceContext {
        executable: out.executable,
        layout: out.local_layout,
        available: &out.available,
        physical_gpr: out.physical_gpr,
        physical_flags: out.physical_flags,
        active_effect: out.active_effect,
        captured_effect: out.captured_effect,
    };
    let scalar = scalar.map(|r| {
        out.recovery
            .program
            .scalar(r, kernel, &context)
            .expect("recovery scalar sources are initialized and live")
    });
    let fp = if out.native.is_empty() {
        None
    } else {
        fp.map(|r| {
            fp::Projection::build(
                &mut out.recovery.program,
                &context,
                r,
                match frame {
                    FrameAction::Select(frame) => frame,
                    _ => out.frame.expect("FP request has an exact frame"),
                },
                out.plan,
            )
            .expect("recovery FP sources are initialized and live")
        })
    };
    let roots: Vec<_> = scalar
        .iter()
        .flat_map(ScalarProjection::roots)
        .chain(fp.iter().flat_map(fp::Projection::roots))
        .collect();
    out.recovery
        .program
        .validate_roots(&roots)
        .expect("one recovery request has unique live colors");
    let mut rows = Vec::new();
    if let Some(scalar) = &scalar {
        out.recovery.scalar_rows(&mut rows, scalar);
    }
    let scalar_work = !rows.is_empty();
    if let Some(fp) = &fp {
        out.recovery.fp_rows(&mut rows, fp);
    }
    let mut row_ids = Vec::with_capacity(rows.len() / 2);
    for row in rows.chunks_exact(2) {
        let pair = [row[0], row[1]];
        let next = out.recovery.row_data.len() as u32;
        let id = *out.recovery.row_ids.entry(pair).or_insert_with(|| {
            out.recovery.row_data.push(pair);
            next
        });
        row_ids.push(id);
    }
    let (frame_word, metadata) = match frame {
        FrameAction::None => (0, None),
        FrameAction::Clear => (1, None),
        FrameAction::Select(frame) if !out.native.is_empty() => (
            u32::try_from(frame.0)
                .ok()
                .and_then(|id| id.checked_add(2))
                .expect("bounded recovery frame action"),
            Some(out.plan.metadata(frame)),
        ),
        FrameAction::Select(_) => (0, None),
    };
    let exit_work = exit.pc.is_some() || exit.retired != 0;
    if rows.is_empty() && fp.is_none() && frame_word == 0 && !exit_work {
        let selector = out.recovery.locals.request();
        out.c().i32_const(0).local_set(selector);
        return;
    }
    let list = out.recovery.lists.intern(&row_ids);
    let mut header = Header {
        list,
        frame: frame_word,
        exit,
        ..Header::default()
    };
    if let Some(fp) = &fp {
        header.fp_info = fp.top_delta.rem_euclid(8) as u32 | u32::from(fp.written) << 8 | 1 << 16;
        header.certificates = fp.certificates;
    }
    if let Some(metadata) = metadata {
        header.instruction = metadata.instruction;
        if let Some(data) = &metadata.data {
            header.segment_plus_one = data.address.resolved_segment().index() as u8 + 1;
        }
    }
    if let Some(recipe) = scalar
        .as_ref()
        .and_then(|s| s.flags.as_ref())
        .and_then(|f| f.arithmetic.as_ref())
    {
        header.kind_plus_one = recipe.kind as u32 + 1;
        header.width = recipe.width;
    }
    let words = header.words();
    let index = if let Some(&index) = out.recovery.sites.get(&words) {
        index
    } else {
        let index = i32::try_from(out.recovery.words.len())
            .expect("bounded signed recovery descriptor selector") as u32;
        out.recovery.relocations.push(index as usize);
        out.recovery.words.extend_from_slice(&words);
        out.recovery.sites.insert(words, index);
        index
    };
    let selector = out.recovery.locals.request();
    // Negative requests contain only FP work and can be skipped when the
    // actual local FP view is inactive. The absolute value remains the exact
    // descriptor selector; no predicted state is used for this decision.
    let selected = index as i32 + 1;
    let selected = if scalar_work || frame_word == 1 || exit_work {
        selected
    } else {
        -selected
    };
    out.c().i32_const(selected).local_set(selector);
}

pub(super) fn request_fp(out: &mut Output<'_>, fp: &FpRecovery) {
    request(
        out,
        None,
        Some(fp),
        out.frame.map_or(FrameAction::None, FrameAction::Select),
        KernelMasks {
            gpr: 0,
            flags: false,
        },
        ExitAction::default(),
    );
}

pub(super) fn emit(out: &mut Output<'_>) {
    let locals = out.recovery.locals;
    let mut state = std::mem::replace(&mut out.recovery, State::new(0, locals));
    state.freeze_data();
    executor::emit(out, &state);
    out.recovery = state;
}

pub(super) fn metadata(words: &[i64], identity: u64) -> Vec<u8> {
    data::encode(words, identity)
}

pub(super) fn getter(words: &[i64], base_global: u32) -> wasm_encoder::Function {
    data::getter(words, base_global)
}

#[cfg(test)]
mod header_tests {
    use super::*;

    #[test]
    fn six_word_header_preserves_full_width_boundaries_and_certificate_bits() {
        for certificates in [0, i64::MIN, i64::MAX, -1, 0x0123_4567_89ab_cdef] {
            let words = Header {
                list: u32::MAX,
                fp_info: 7 | 255 << 8 | 1 << 16,
                frame: u32::MAX,
                certificates,
                instruction: Some((u32::MAX, u16::MAX)),
                segment_plus_one: 6,
                kind_plus_one: u32::MAX,
                width: u32::MAX,
                exit: ExitAction {
                    retired: u32::MAX,
                    pc: Some(u32::MAX),
                },
            }
            .words();
            assert_eq!(words.len(), 6);
            assert_eq!(words[0] as u64, u64::from(u32::MAX));
            let info = words[1] as u64;
            assert_eq!(info as u32, 7 | 255 << 8 | 1 << 16 | 1 << 17);
            assert_eq!((info >> 32) as u32, u32::MAX);
            assert_eq!(words[2], certificates);
            let metadata = words[3] as u64;
            assert_eq!(metadata as u32, u32::MAX);
            assert_eq!((metadata >> 32) as u16, u16::MAX);
            assert_eq!(metadata >> 48 & 1, 1);
            assert_eq!(metadata >> 49 & 7, 6);
            assert_eq!(metadata >> 52, 0);
            assert_eq!(words[4] as u32, u32::MAX);
            assert_eq!(((words[4] as u64) >> 32) as u32, u32::MAX);
            assert_eq!(words[5] as u32, u32::MAX);
            assert_eq!(((words[5] as u64) >> 32) as u32, u32::MAX);
        }
    }

    #[test]
    fn zero_pc_and_metadata_remain_distinct_from_absent_actions() {
        let empty = Header::default().words();
        assert_eq!(empty, [0; 6]);
        let explicit = Header {
            frame: 1,
            instruction: Some((0, 0)),
            exit: ExitAction {
                pc: Some(0),
                retired: 0,
            },
            ..Header::default()
        }
        .words();
        assert_ne!(explicit, empty);
        assert_eq!(explicit[1] as u32, 1 << 17);
        assert_eq!((explicit[1] as u64) >> 32, 1);
        assert_eq!(explicit[3] as u64, 1 << 48);
        assert_eq!(explicit[5], 0);
        assert_eq!(
            Header {
                segment_plus_one: 6,
                ..Header::default()
            }
            .words()[3] as u64,
            6 << 49
        );
    }

    #[test]
    fn six_word_sites_relocate_lists_after_headers_and_rows_before_leaf_map() {
        let mut state = State::new(
            0,
            Locals {
                base: 300,
                colors: 0,
                expression_base: 179,
            },
        );
        state.row_data.push([17, 23]);
        let list = state.lists.intern(&[0]);
        state.words.extend(
            Header {
                list,
                certificates: i64::MIN,
                ..Header::default()
            }
            .words(),
        );
        state.words.extend(
            Header {
                list,
                frame: u32::MAX,
                ..Header::default()
            }
            .words(),
        );
        state.relocations.extend([0, 6]);
        state.freeze_data();
        assert_eq!(state.words.len(), 17);
        assert_eq!(state.words[0], 14);
        assert_eq!(state.words[6], 14);
        assert_eq!(state.words[2], i64::MIN);
        assert_eq!((state.words[7] as u64) >> 32, u64::from(u32::MAX));
        assert_eq!(&state.words[12..], &[17, 23, 0, 1, 12]);
        assert_eq!(state.leaf_base, 17);
    }
}
