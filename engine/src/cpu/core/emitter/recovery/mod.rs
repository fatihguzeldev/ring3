//! One cold, same-body projection program. Request sites retain only a selector.
mod capture;
mod data;
mod executor;
pub(super) mod fp;
mod leaves;
mod legacy;
mod lists;
pub(crate) mod packet;
pub(super) mod program;
mod recipe_vm;
mod recipes;
mod shared;

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
    closure: u32,
    closure_count: u32,
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
            pair(self.closure, self.closure_count),
        ]
    }
}

#[derive(Clone)]
pub(super) struct State {
    compact: bool,
    pub(super) shared: bool,
    site_words: u32,
    pub program: Program,
    pub words: Vec<i64>,
    sites: BTreeMap<Vec<i64>, u32>,
    row_data: Vec<[i64; 2]>,
    row_ids: BTreeMap<[i64; 2], u32>,
    lists: lists::ListPool,
    relocations: Vec<usize>,
    closure_relocations: Vec<usize>,
    closures: BTreeMap<Vec<u32>, u32>,
    recipes: recipes::Recipes,
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
        self.base + 32 + color - self.expression_base
    }
    pub fn wide(self, offset: u32) -> u32 {
        self.base + 32 + self.colors + offset
    }
    pub fn float(self) -> u32 {
        self.wide(8)
    }
}

impl State {
    pub fn new(limit: usize, locals: Locals) -> Self {
        Self {
            compact: true,
            shared: false,
            site_words: 0,
            program: Program::new(limit),
            words: Vec::new(),
            sites: BTreeMap::new(),
            row_data: Vec::new(),
            row_ids: BTreeMap::new(),
            lists: lists::ListPool::default(),
            relocations: Vec::new(),
            closure_relocations: Vec::new(),
            closures: BTreeMap::new(),
            recipes: recipes::Recipes::default(),
            leaf_base: 0,
            frozen: false,
            locals,
        }
    }
    fn intern_closure(&mut self, roots: &[RefId]) -> (u32, u32) {
        let closure = self
            .program
            .validate_roots(roots)
            .expect("one recovery request has unique live colors");
        let closure_count = u32::try_from(closure.len()).expect("bounded recovery closure count");
        let markers: Vec<_> = closure
            .into_iter()
            .map(|id| id.0.checked_add(1).expect("bounded recovery source marker"))
            .collect();
        let closure = if markers.is_empty() {
            0
        } else {
            let next = u32::try_from(self.closures.len())
                .ok()
                .and_then(|count| count.checked_add(1))
                .expect("bounded recovery closure identity");
            *self.closures.entry(markers).or_insert(next)
        };
        (closure, closure_count)
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
        self.site_words = u32::try_from(self.words.len()).expect("bounded recovery site directory");
        if self.words.is_empty() {
            return;
        }
        let base = self.words.len() as u32;
        self.words.extend(self.row_data.iter().flatten().copied());
        let lists = self.lists.append(&mut self.words, base);
        for &index in &self.relocations {
            self.words[index] = i64::from(lists[self.words[index] as usize]);
        }
        if !self.compact {
            // Only the compact executor consumes the seventh private word.
            for &index in &self.closure_relocations {
                self.words[index] = 0;
            }
            self.leaf_base = leaves::append(&self.program, &mut self.words);
            return;
        }
        self.recipes = recipes::append(&self.program, &mut self.words, self.locals.expression_base);
        let mut closures = vec![0; self.closures.len() + 1];
        for (markers, &id) in &self.closures {
            closures[id as usize] =
                u32::try_from(self.words.len()).expect("bounded recovery closure address");
            for pair in markers.chunks(2) {
                self.words.push(
                    (u64::from(pair[0]) | u64::from(pair.get(1).copied().unwrap_or(0)) << 32)
                        as i64,
                );
            }
            u32::try_from(self.words.len()).expect("bounded recovery closure end");
        }
        for &index in &self.closure_relocations {
            let value = self.words[index] as u64;
            self.words[index] = ((value & !u64::from(u32::MAX))
                | u64::from(closures[value as u32 as usize]))
                as i64;
        }
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
    let (closure, closure_count) = out.recovery.intern_closure(&roots);
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
        closure,
        closure_count,
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
        out.recovery.closure_relocations.push(index as usize + 6);
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
    if state.shared {
        state.freeze_data();
        capture::emit(out, &state, out.owner_identity);
        out.recovery = state;
        return;
    }
    let mut legacy = state.clone();
    legacy.compact = false;
    legacy.freeze_data();
    let prefix = out.function.clone();
    let depth = out.depth;
    executor::emit(out, &legacy);
    assert_eq!(out.depth, depth, "legacy recovery controls are balanced");
    let legacy_cost = candidate_cost(out, &legacy);
    let legacy_function = std::mem::replace(out.function, prefix);
    state.freeze_data();
    executor::emit(out, &state);
    assert_eq!(out.depth, depth, "compact recovery controls are balanced");
    if candidate_cost(out, &state).improves(legacy_cost) {
        out.recovery = state;
    } else {
        *out.function = legacy_function;
        out.recovery = legacy;
    }
}

#[derive(Clone, Copy)]
struct CandidateCost {
    run: usize,
    getter: usize,
    data: usize,
}

impl CandidateCost {
    fn improves(self, previous: Self) -> bool {
        // The unchanged run suffix is appended later. Requiring its raw body
        // and the total code payload to be nonincreasing also makes both run
        // and CodeSection length prefixes nonincreasing. Getter and custom
        // section framing are already encoded exactly below. Thus the strict
        // final sum is a real module-byte reduction, without a guessed margin.
        self.run <= previous.run
            && self.run + self.getter <= previous.run + previous.getter
            && self.run + self.getter + self.data < previous.run + previous.getter + previous.data
    }
}

fn candidate_cost(out: &Output<'_>, state: &State) -> CandidateCost {
    use std::borrow::Cow;
    use wasm_encoder::Encode;
    // Owner identity occupies a fixed raw u64 header, so it cannot affect cost.
    let blob = metadata(&state.words, 1);
    let mut data = Vec::new();
    wasm_encoder::CustomSection {
        name: Cow::Borrowed("ring3.recovery"),
        data: Cow::Borrowed(&blob),
    }
    .encode(&mut data);
    let mut encoded_getter = Vec::new();
    getter(&state.words, super::link::GLOBAL_COUNT + 1).encode(&mut encoded_getter);
    CandidateCost {
        run: out.function.byte_len(),
        getter: encoded_getter.len(),
        data: data.len(),
    }
}

pub(super) fn shared_support(word: u32, continuation_global: u32) -> wasm_encoder::Function {
    shared::emit(word, continuation_global)
}
pub(super) fn shared_word() -> wasm_encoder::Function {
    shared::emit_word()
}
pub(super) fn shared_entry(out: &mut Output<'_>, identity: u64) {
    capture::entry_guard(out, identity);
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
    fn seven_word_header_preserves_full_width_boundaries_and_certificate_bits() {
        for certificates in [0, i64::MIN, i64::MAX, -1, 0x0123_4567_89ab_cdef] {
            let words = Header {
                list: u32::MAX,
                closure: u32::MAX,
                closure_count: u32::MAX,
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
            assert_eq!(words.len(), 7);
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
            assert_eq!(words[6] as u64, u64::MAX);
        }
    }

    #[test]
    fn zero_pc_and_metadata_remain_distinct_from_absent_actions() {
        let empty = Header::default().words();
        assert_eq!(empty, [0; 7]);
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
    fn seven_word_sites_relocate_lists_after_headers_and_rows_before_recipes() {
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
        state.relocations.extend([0, 7]);
        state.freeze_data();
        assert_eq!(state.words.len(), 19);
        assert_eq!(state.words[0], 16);
        assert_eq!(state.words[7], 16);
        assert_eq!(state.words[2], i64::MIN);
        assert_eq!((state.words[8] as u64) >> 32, u64::from(u32::MAX));
        assert_eq!(&state.words[14..], &[17, 23, 0, 1, 14]);
        assert_eq!(state.recipes.base, 19);
        assert_eq!(state.recipes.count, 0);
    }

    #[test]
    fn descriptor_closures_pack_odd_and_even_markers_deduplicate_and_relocate_exactly() {
        use crate::cpu::core::region::values::{
            ValueType,
            executable::{Binary32, ExecValueId},
        };
        use program::{Color, SourceOp, ValueKey};

        let mut state = State::new(
            8,
            Locals {
                base: 300,
                colors: 3,
                expression_base: 180,
            },
        );
        let mut reference = |value, local, op| {
            state
                .program
                .reference(
                    ValueKey::Executable(ExecValueId(value)),
                    Color {
                        local,
                        ty: ValueType::I32,
                    },
                    op,
                )
                .unwrap()
        };
        let first = reference(0, 180, SourceOp::Available);
        let second = reference(1, 181, SourceOp::Constant { bits: 0x8000_0001 });
        let sum = reference(
            2,
            182,
            SourceOp::Binary32 {
                kind: Binary32::Add,
                left: first,
                right: second,
            },
        );
        let (odd, odd_count) = state.intern_closure(&[sum, first, sum]);
        let (even, even_count) = state.intern_closure(&[second, first]);
        assert_eq!(
            state.intern_closure(&[first, sum, second]),
            (odd, odd_count)
        );
        assert_eq!((odd_count, even_count), (3, 2));
        assert_ne!(odd, even);
        assert_eq!(state.closures.len(), 2);
        assert_eq!(state.intern_closure(&[]), (0, 0));
        let headers = [
            Header {
                closure: odd,
                closure_count: odd_count,
                certificates: i64::MIN,
                ..Header::default()
            }
            .words(),
            Header {
                closure: even,
                closure_count: even_count,
                frame: u32::MAX,
                ..Header::default()
            }
            .words(),
            Header {
                closure: odd,
                closure_count: odd_count,
                instruction: Some((0xffff_ffff, 0xffff)),
                ..Header::default()
            }
            .words(),
        ];
        for header in &headers {
            let start = state.words.len();
            state.relocations.push(start);
            state.closure_relocations.push(start + 6);
            state.words.extend(header);
        }
        state.freeze_data();
        assert_eq!(state.recipes.base, 21);
        assert_eq!(state.recipes.count, 3);
        for (index, original) in headers.iter().enumerate() {
            assert_eq!(&state.words[index * 7..index * 7 + 6], &original[..6]);
        }
        // Exact lexical list order puts the two-marker closure before the
        // three-marker closure, independently of their original interned IDs.
        assert_eq!(state.words[6] as u64, (3u64 << 32) | 34);
        assert_eq!(state.words[13] as u64, (2u64 << 32) | 33);
        assert_eq!(state.words[20], state.words[6]);
        assert_eq!(&state.words[33..], &[(2i64 << 32) | 1, (2i64 << 32) | 1, 3]);
        assert_eq!(state.words.len(), 36);
    }

    #[test]
    fn separate_descriptor_closures_keep_distinct_ref_ids_when_a_color_is_reused() {
        use crate::cpu::core::region::values::{ValueType, executable::ExecValueId};
        use program::{Color, SourceOp, ValueKey};
        use std::panic::{AssertUnwindSafe, catch_unwind};

        let mut state = State::new(
            8,
            Locals {
                base: 300,
                colors: 1,
                expression_base: 180,
            },
        );
        let color = Color {
            local: 180,
            ty: ValueType::I32,
        };
        let old = state
            .program
            .reference(
                ValueKey::Executable(ExecValueId(0)),
                color,
                SourceOp::Available,
            )
            .unwrap();
        let fresh = state
            .program
            .reference(
                ValueKey::Executable(ExecValueId(1)),
                color,
                SourceOp::PhysicalInput { local: 2 },
            )
            .unwrap();
        let originals = state.program.refs().to_vec();
        let old_closure = state.intern_closure(&[old]);
        let fresh_closure = state.intern_closure(&[fresh]);
        assert_ne!(old_closure.0, fresh_closure.0);
        assert_eq!((old_closure.1, fresh_closure.1), (1, 1));
        assert!(catch_unwind(AssertUnwindSafe(|| state.intern_closure(&[old, fresh]))).is_err());
        assert_eq!(
            state.closures.len(),
            2,
            "the invalid mixed closure cannot enter the immutable table"
        );
        for (closure, closure_count) in [old_closure, fresh_closure] {
            let start = state.words.len();
            state.words.extend(
                Header {
                    closure,
                    closure_count,
                    ..Header::default()
                }
                .words(),
            );
            state.relocations.push(start);
            state.closure_relocations.push(start + 6);
        }
        state.freeze_data();
        assert_eq!(state.program.refs(), originals);
        assert_eq!((state.recipes.base, state.recipes.count), (14, 2));
        assert_eq!(state.words[6] as u64, (1u64 << 32) | 22);
        assert_eq!(state.words[13] as u64, (1u64 << 32) | 23);
        assert_eq!(&state.words[22..], &[1, 2]);
        assert_eq!(&state.words[14..22], &[0, 0, 0, 0, 1, 2, 0, 0]);
    }

    fn encoded_cost_fixture(
        run_prefix: usize,
        getter_body: usize,
        data_length: usize,
        common_suffix: usize,
    ) -> (CandidateCost, Vec<u8>) {
        use std::borrow::Cow;
        use wasm_encoder::{
            CodeSection, CustomSection, Encode, Function, FunctionSection, Module, TypeSection,
            ValType,
        };

        assert!(run_prefix >= 1 && getter_body >= 4);
        let mut run = Function::new([]);
        for _ in 1..run_prefix {
            run.instructions().nop();
        }
        let mut getter = Function::new([]);
        for _ in 4..getter_body {
            getter.instructions().nop();
        }
        getter.instructions().i64_const(0).end();
        assert_eq!(run.byte_len(), run_prefix);
        assert_eq!(getter.byte_len(), getter_body);
        let mut getter_encoded = Vec::new();
        getter.encode(&mut getter_encoded);
        let payload = vec![0x5a; data_length];
        let section = CustomSection {
            name: Cow::Borrowed("ring3.recovery"),
            data: Cow::Borrowed(&payload),
        };
        let mut data_encoded = Vec::new();
        section.encode(&mut data_encoded);
        let cost = CandidateCost {
            run: run.byte_len(),
            getter: getter_encoded.len(),
            data: data_encoded.len(),
        };
        // The cost is measured before a common epilogue, exactly as in emit().
        for _ in 0..common_suffix {
            run.instructions().nop();
        }
        run.instructions().end();
        let mut types = TypeSection::new();
        types.ty().function([], []);
        types.ty().function([], [ValType::I64]);
        let mut functions = FunctionSection::new();
        functions.function(0).function(1);
        let mut code = CodeSection::new();
        code.function(&run).function(&getter);
        let mut module = Module::new();
        module
            .section(&types)
            .section(&functions)
            .section(&code)
            .section(&section);
        (cost, module.finish())
    }

    #[test]
    fn candidate_cost_acceptance_decreases_final_modules_across_uleb_and_suffix_boundaries() {
        // Getter growth is deliberately compensated by run shrink. The actual
        // Function, CodeSection and CustomSection encoders supply every length
        // prefix; no hand-computed byte count is used as the final-size oracle.
        let cases = [
            // CodeSection payloads 126/127/128 before adding a suffix.
            (58, 64, 110),
            (59, 64, 111),
            (60, 64, 112),
            // CodeSection payloads 16382/16383/16384.
            (8188, 8188, 16366),
            (8189, 8188, 16367),
            (8190, 8188, 16368),
            (118, 124, 110),
            (119, 125, 111),
            (126, 126, 112),
            (127, 127, 113),
            (128, 128, 114),
            (16374, 16378, 16366),
            (16375, 16379, 16367),
            (16382, 16382, 16368),
            (16383, 16383, 16369),
            (16384, 16384, 16370),
        ];
        for (run, getter, data) in cases {
            for suffix in [0, 1, 2, 5, 126, 127, 128, 16383] {
                let (old_cost, old_module) = encoded_cost_fixture(run, getter, data, suffix);
                let (new_cost, new_module) =
                    encoded_cost_fixture(run - 8, getter + 4, data, suffix);
                assert!(new_cost.getter > old_cost.getter);
                assert!(
                    new_cost.improves(old_cost),
                    "run={run} getter={getter} data={data} suffix={suffix}"
                );
                assert!(
                    new_module.len() < old_module.len(),
                    "accepted candidate increased final module: run={run} getter={getter} data={data} suffix={suffix}"
                );
                let (data_cost, data_module) = encoded_cost_fixture(run, getter, data - 1, suffix);
                assert!(data_cost.improves(old_cost));
                assert!(
                    data_module.len() < old_module.len(),
                    "exact custom-section savings must survive final framing"
                );
            }
        }
    }

    #[test]
    fn equal_or_costlier_encoded_candidates_keep_the_legacy_module() {
        for (run, getter, data) in [
            (58, 64, 110),
            (59, 64, 111),
            (60, 64, 112),
            (8188, 8188, 16366),
            (8189, 8188, 16367),
            (8190, 8188, 16368),
            (126, 126, 111),
            (127, 127, 112),
            (128, 128, 113),
            (16382, 16382, 16367),
            (16383, 16383, 16368),
            (16384, 16384, 16369),
        ] {
            for suffix in [0, 1, 126, 127, 128] {
                let (old_cost, old_module) = encoded_cost_fixture(run, getter, data, suffix);
                for (new_run, new_getter, new_data) in [
                    (run, getter, data),
                    (run + 1, getter, data),
                    (run, getter + 1, data),
                    (run, getter, data + 1),
                ] {
                    let (new_cost, new_module) =
                        encoded_cost_fixture(new_run, new_getter, new_data, suffix);
                    assert!(new_module.len() >= old_module.len());
                    assert!(
                        !new_cost.improves(old_cost),
                        "equal or costlier candidate selected at run={run} getter={getter} data={data} suffix={suffix}"
                    );
                    let selected = if new_cost.improves(old_cost) {
                        new_module
                    } else {
                        old_module.clone()
                    };
                    assert_eq!(selected, old_module);
                }
            }
        }
    }
}
