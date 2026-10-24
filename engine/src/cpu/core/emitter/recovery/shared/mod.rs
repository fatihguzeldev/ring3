//! one cpu-wide pure recovery evaluator with request-local typed cells.
mod data;
mod sources;
#[cfg(test)]
mod tests;
use super::packet as p;
use crate::cpu::core::{
    continuation as d,
    emitter::{link, mem},
};
use wasm_encoder::{BlockType, Function, InstructionSink, ValType};

const HEADER: u32 = 4;
const INDEX: u32 = 5;
const SLOT: u32 = 6;
const OWNER: u32 = 7;
const SITE: u32 = 8;
const COUNT: u32 = 9;
const BITS: u32 = 10;
const BASE: u32 = 11;
const REFS: u32 = 12;
const COLORS: u32 = 13;
const EPOCH: u32 = 14;
const DIRTY: u32 = 15;
const PHASE: u32 = 16;
const LIST: u32 = 17;
const CURSOR: u32 = 18;
const LIMIT: u32 = 19;
const ROW: u32 = 20;
const MARKER: u32 = 21;
const COLOR: u32 = 22;
const CELL: u32 = 23;
const KIND: u32 = 24;
const ARG: u32 = 25;
const ARGHI: u32 = 26;
const DEP: u32 = 27;
const DEPMARK: u32 = 28;
const DEPCOLOR: u32 = 29;
const DEPCELL: u32 = 30;
const RECORD: u32 = 31;
const MODE: u32 = 32;
const TEMP: u32 = 33;
const ROLE: u32 = 34;
const DEST: u32 = 35;
const GATE: u32 = 36;
const VALID: u32 = 37;
const ALIASES: u32 = 38;
const FRAME: u32 = 39;
const WORDS: u32 = 40;
const CLOSURE: u32 = 41;
const CLOSURE_COUNT: u32 = 42;
const STEPS: u32 = 43;
const W0: u32 = 44;
const W1: u32 = 45;
const VALUE: u32 = 46;
const D0: u32 = 47;
const D1: u32 = 48;
const D2: u32 = 49;
const D3: u32 = 50;
const RESULT: u32 = 51;
const MASK: u32 = 52;
const MEMORY_BYTES: u32 = 53;
const ID: u32 = 54;
const GENERATION: u32 = 55;
const NONCE: u32 = 56;

struct Vm {
    f: Function,
    word: u32,
    continuation: u32,
}
impl Vm {
    fn c(&mut self) -> InstructionSink<'_> {
        self.f.instructions()
    }
    fn reject(&mut self) {
        self.c()
            .if_(BlockType::Empty)
            .i32_const(p::REJECT)
            .return_()
            .end();
    }
    fn load(&mut self, base: u32, offset: u32) {
        self.c().local_get(base).i32_load(mem(u64::from(offset), 0));
    }
    fn wide(&mut self, base: u32, offset: u32) {
        self.c().local_get(base).i64_load(mem(u64::from(offset), 0));
    }
    fn put(&mut self, offset: u32, value: i32) {
        self.c()
            .local_get(3)
            .i32_const(value)
            .i32_store(mem(u64::from(offset), 0));
    }
    fn store(&mut self, offset: u32, local: u32) {
        self.c()
            .local_get(3)
            .local_get(local)
            .i32_store(mem(u64::from(offset), 0));
    }
    fn word(&mut self, index: u32) {
        let word = self.word;
        self.c().local_get(1).local_get(index).call(word);
    }
    fn field(&mut self, index: u32) {
        let word = self.word;
        self.c()
            .local_get(1)
            .local_get(SITE)
            .i32_const(index as i32)
            .i32_add()
            .call(word);
    }
    fn record(&mut self, index: u32) {
        let word = self.word;
        self.c()
            .local_get(1)
            .local_get(RECORD)
            .i32_const(index as i32)
            .i32_add()
            .call(word);
    }
    fn address(&mut self, color: u32, target: u32) {
        self.c().local_get(color).local_get(COLORS).i32_ge_u();
        self.reject();
        self.c()
            .local_get(3)
            .i32_const(p::CELLS_OFFSET as i32)
            .i32_add()
            .local_get(color)
            .i32_const(4)
            .i32_shl()
            .i32_add()
            .local_set(target);
    }
    fn range(&mut self, base: u32, bytes: u32) {
        self.c()
            .local_get(base)
            .i64_extend_i32_u()
            .i64_const(i64::from(bytes))
            .i64_add()
            .local_get(MEMORY_BYTES)
            .i64_gt_u();
        self.reject();
    }
    fn auth(&mut self) {
        self.c()
            .memory_size(0)
            .i64_extend_i32_u()
            .i64_const(16)
            .i64_shl()
            .local_set(MEMORY_BYTES);
        self.range(3, p::WORKSPACE_BYTES);
        for (offset, value) in [
            (p::MAGIC_OFFSET, p::MAGIC),
            (p::VERSION_OFFSET, p::VERSION),
            (p::BYTES_OFFSET, p::WORKSPACE_BYTES),
            (p::CAPACITY_OFFSET, p::MAX_CAPTURE_COLORS),
            (p::CELLS, p::CELLS_OFFSET),
        ] {
            self.load(3, offset);
            self.c().i32_const(value as i32).i32_ne();
            self.reject();
        }
        self.load(3, p::CONTEXT);
        self.c().local_get(0).i32_ne();
        self.reject();
        let continuation = self.continuation;
        self.c()
            .global_get(continuation)
            .local_tee(HEADER)
            .i32_eqz();
        self.reject();
        self.load(3, p::CONTINUATION);
        self.c().local_get(HEADER).i32_ne();
        self.reject();
        self.range(HEADER, d::HEADER_SIZE as u32);
        for (offset, value) in [(d::FORMAT, d::FORMAT_VERSION), (d::ACTIVE, 1)] {
            self.load(HEADER, offset as u32);
            self.c().i32_const(value as i32).i32_ne();
            self.reject();
        }
        self.load(HEADER, d::CONTEXT as u32);
        self.c().local_get(0).i32_ne();
        self.reject();
        self.wide(HEADER, d::MEMORY as u32);
        self.wide(3, p::MEMORY);
        self.c().i64_ne();
        self.reject();
        self.wide(3, p::ARMED_NONCE);
        self.c().local_tee(NONCE).i64_eqz();
        self.reject();
        for offset in [d::NONCE, d::CURRENT_NONCE] {
            self.wide(HEADER, offset as u32);
            self.c().local_get(NONCE).i64_ne();
            self.reject();
        }
        self.wide(3, p::EXPECTED_NONCE);
        self.c().local_get(NONCE).i64_ne();
        self.reject();
        self.wide(3, p::EXPECTED_OWNER);
        self.c().local_tee(ID).i64_eqz();
        self.reject();
        self.wide(HEADER, d::CURRENT_ID as u32);
        self.c().local_get(ID).i64_ne();
        self.reject();
        self.load(HEADER, d::CURRENT_SLOT as u32);
        self.c().local_set(SLOT);
        self.load(3, p::EXPECTED_SLOT);
        self.c().local_get(SLOT).i32_ne();
        self.reject();
        self.wide(3, p::EXPECTED_GENERATION);
        self.c().local_set(GENERATION);
        self.wide(HEADER, d::CURRENT_GENERATION as u32);
        self.c().local_get(GENERATION).i64_ne();
        self.reject();
        self.load(HEADER, d::CURRENT_INDEX as u32);
        self.c().local_set(INDEX);
        self.load(HEADER, d::FLAGS as u32);
        self.c()
            .local_tee(TEMP)
            .i32_const(d::TRANSIENT as i32)
            .i32_eq()
            .if_(BlockType::Empty);
        self.c().local_get(INDEX).local_get(SLOT).i32_or();
        self.reject();
        self.c().local_get(GENERATION).i64_eqz().i32_eqz();
        self.reject();
        self.c().else_().local_get(INDEX).i32_eqz();
        self.reject();
        self.load(HEADER, d::OWNER_LEN as u32);
        self.c().local_get(INDEX).i32_lt_u();
        self.reject();
        self.load(HEADER, d::OWNERS as u32);
        self.c()
            .local_get(INDEX)
            .i32_const(1)
            .i32_sub()
            .i32_const(d::OWNER_SIZE as i32)
            .i32_mul()
            .i32_add()
            .local_set(OWNER);
        self.range(OWNER, d::OWNER_SIZE as u32);
        self.wide(OWNER, d::OWNER_ID as u32);
        self.c().local_get(ID).i64_ne();
        self.reject();
        self.c()
            .local_get(SLOT)
            .i32_eqz()
            .if_(BlockType::Empty)
            .local_get(TEMP)
            .i32_const(d::SELECTED_UNBOUND as i32)
            .i32_ne();
        self.reject();
        self.load(OWNER, d::OWNER_SLOT as u32);
        self.reject();
        self.c().else_().local_get(TEMP).i32_eqz().i32_eqz();
        self.reject();
        self.wide(OWNER, d::OWNER_GENERATION as u32);
        self.c().local_get(GENERATION).i64_ne();
        self.reject();
        self.load(OWNER, d::OWNER_SLOT as u32);
        self.c().local_get(SLOT).i32_ne();
        self.reject();
        self.load(OWNER, d::OWNER_FLAGS as u32);
        self.c().i32_const(d::BOUND as i32).i32_and().i32_eqz();
        self.reject();
        self.load(HEADER, d::SLOT_LEN as u32);
        self.c().local_get(SLOT).i32_le_u();
        self.reject();
        self.load(HEADER, d::LINK_OWNERS as u32);
        self.c()
            .local_get(SLOT)
            .i32_const(3)
            .i32_shl()
            .i32_add()
            .local_set(TEMP);
        self.range(TEMP, 8);
        self.wide(TEMP, 0);
        self.c().local_get(ID).i64_ne();
        self.reject();
        self.load(HEADER, d::SLOT_REGIONS as u32);
        self.c()
            .local_get(SLOT)
            .i32_const(2)
            .i32_shl()
            .i32_add()
            .local_set(TEMP);
        self.range(TEMP, 4);
        self.load(TEMP, 0);
        self.c().local_get(INDEX).i32_const(1).i32_sub().i32_ne();
        self.reject();
        self.load(HEADER, d::SLOT_GENERATIONS as u32);
        self.c()
            .local_get(SLOT)
            .i32_const(3)
            .i32_shl()
            .i32_add()
            .local_set(TEMP);
        self.range(TEMP, 8);
        self.wide(TEMP, 0);
        self.c().local_get(GENERATION).i64_ne();
        self.reject();
        self.c().end().end();
        self.load(3, p::BLOB);
        self.c().local_get(1).i32_ne();
        self.reject();
        self.load(3, p::SIGNED_SITE);
        self.c().local_get(2).i32_ne();
        self.reject();
        self.range(1, 48);
        for (offset, value) in [(0, 0x4452_3352u32), (4, 2)] {
            self.load(1, offset);
            self.c().i32_const(value as i32).i32_ne();
            self.reject();
        }
        self.wide(1, 32);
        self.c().local_get(ID).i64_ne();
        self.reject();
        self.load(1, 8);
        self.c().local_set(WORDS);
        self.load(1, 12);
        self.c()
            .local_tee(BITS)
            .i32_const(1)
            .i32_sub()
            .i32_const(32)
            .i32_ge_u();
        self.reject();
        self.c()
            .local_get(WORDS)
            .i64_extend_i32_u()
            .local_get(BITS)
            .i64_extend_i32_u()
            .i64_mul()
            .i64_const(u32::MAX as i64)
            .i64_gt_u();
        self.reject();
        self.load(1, 24);
        self.c().i32_const(48).i32_ne();
        self.reject();
        self.load(1, 16);
        self.c()
            .i64_extend_i32_u()
            .i64_const(8)
            .i64_mul()
            .i64_const(48)
            .i64_add()
            .local_set(W0);
        self.load(1, 28);
        self.c().i64_extend_i32_u().local_get(W0).i64_ne();
        self.reject();
        self.load(1, 40);
        self.c().local_tee(MODE).i32_const(2).i32_gt_u();
        self.reject();
        self.c().local_get(W0);
        self.load(1, 20);
        self.c()
            .i64_extend_i32_u()
            .local_get(MODE)
            .i32_const(0)
            .i32_eq()
            .if_(BlockType::Result(ValType::I32))
            .i32_const(8)
            .else_()
            .i32_const(4)
            .local_get(MODE)
            .i32_add()
            .end()
            .i64_extend_i32_u()
            .i64_mul()
            .i64_add();
        self.load(1, 44);
        self.c()
            .i64_extend_i32_u()
            .i64_const(4)
            .i64_mul()
            .i64_add()
            .local_get(1)
            .i64_extend_i32_u()
            .i64_add()
            .local_get(MEMORY_BYTES)
            .i64_gt_u();
        self.reject();
        self.load(3, p::COLORS);
        self.c()
            .local_tee(COLORS)
            .i32_const(p::MAX_CAPTURE_COLORS as i32)
            .i32_gt_u();
        self.reject();
        self.load(3, p::RECIPES_BASE);
        self.c().local_set(BASE);
        self.load(3, p::RECIPES_COUNT);
        self.c().local_set(REFS);
        self.c()
            .local_get(BASE)
            .i64_extend_i32_u()
            .local_get(REFS)
            .i64_extend_i32_u()
            .i64_const(4)
            .i64_mul()
            .i64_add()
            .local_get(WORDS)
            .i64_extend_i32_u()
            .i64_gt_u();
        self.reject();
        self.load(3, p::SITE_WORDS);
        self.c().local_set(COUNT);
        self.c().local_get(2).i32_eqz();
        self.reject();
        self.c().local_get(2).i32_const(i32::MIN).i32_eq();
        self.reject();
        self.c()
            .local_get(2)
            .local_get(2)
            .i32_const(-1)
            .i32_mul()
            .local_get(2)
            .i32_const(0)
            .i32_gt_s()
            .select()
            .i32_const(1)
            .i32_sub()
            .local_tee(SITE)
            .i32_const(7)
            .i32_rem_u();
        self.reject();
        self.c()
            .local_get(SITE)
            .i64_extend_i32_u()
            .i64_const(7)
            .i64_add()
            .local_get(COUNT)
            .i64_extend_i32_u()
            .i64_gt_u();
        self.reject();
        self.c().local_get(COUNT).local_get(WORDS).i32_gt_u();
        self.reject();
        self.field(6);
        self.c()
            .local_tee(W0)
            .i32_wrap_i64()
            .local_set(CLOSURE)
            .local_get(W0)
            .i64_const(32)
            .i64_shr_u()
            .i32_wrap_i64()
            .local_tee(CLOSURE_COUNT)
            .local_get(COLORS)
            .i32_gt_u();
        self.reject();
        self.c()
            .local_get(CLOSURE)
            .i64_extend_i32_u()
            .local_get(CLOSURE_COUNT)
            .i64_extend_i32_u()
            .i64_const(1)
            .i64_add()
            .i64_const(1)
            .i64_shr_u()
            .i64_add()
            .local_get(WORDS)
            .i64_extend_i32_u()
            .i64_gt_u();
        self.reject();
        self.load(3, p::FP_ACTIVE);
        self.c().if_(BlockType::Empty);
        self.load(3, p::BANK);
        self.load(0, 92);
        self.c().i32_ne();
        self.reject();
        self.load(3, p::POOL);
        self.load(3, p::BANK);
        self.c().i32_const(512).i32_add().i32_ne();
        self.reject();
        self.c().end();
        self.load(3, p::EPOCH);
        self.c().local_set(EPOCH);
        self.field(1);
        self.c().i32_wrap_i64().local_set(TEMP);
        self.c().i32_const(0).local_set(DIRTY);
        self.load(3, p::FP_ACTIVE);
        self.c()
            .local_get(TEMP)
            .i32_const(1 << 16)
            .i32_and()
            .i32_eqz()
            .i32_eqz()
            .i32_and()
            .if_(BlockType::Empty);
        self.load(3, p::FP_DIRTY);
        self.c().local_get(TEMP).i32_const(7).i32_and().i32_shr_u();
        self.load(3, p::FP_DIRTY);
        self.c()
            .i32_const(8)
            .local_get(TEMP)
            .i32_const(7)
            .i32_and()
            .i32_sub()
            .i32_shl()
            .i32_or()
            .i32_const(255)
            .i32_and()
            .local_get(TEMP)
            .i32_const(8)
            .i32_shr_u()
            .i32_const(255)
            .i32_and()
            .i32_or()
            .local_set(DIRTY)
            .end();
    }
    fn mark(&mut self) {
        self.address(COLOR, CELL);
        self.load(CELL, p::CELL_EPOCH);
        self.c()
            .local_get(EPOCH)
            .i32_ne()
            .if_(BlockType::Empty)
            .local_get(CELL)
            .local_get(EPOCH)
            .i32_store(mem(u64::from(p::CELL_EPOCH), 0))
            .local_get(CELL)
            .i32_const(0)
            .i32_store(mem(u64::from(p::CELL_TAG), 0))
            .end();
        self.load(CELL, p::CELL_TAG);
        self.c()
            .local_get(MARKER)
            .i32_const(-1)
            .i32_mul()
            .i32_ne()
            .if_(BlockType::Empty)
            .local_get(CELL)
            .local_get(MARKER)
            .i32_store(mem(u64::from(p::CELL_TAG), 0))
            .end();
    }
    fn ready(&mut self, cell: u32, marker: u32) {
        self.load(cell, p::CELL_EPOCH);
        self.c().local_get(EPOCH).i32_ne();
        self.reject();
        self.load(cell, p::CELL_TAG);
        self.c().local_get(marker).i32_const(-1).i32_mul().i32_ne();
        self.reject();
    }
    fn row_loop(&mut self, phase: u32) {
        self.c().i32_const(phase as i32).local_set(PHASE);
        self.field(0);
        self.c()
            .i32_wrap_i64()
            .local_set(LIST)
            .i32_const(0)
            .local_set(STEPS)
            .block(BlockType::Empty)
            .loop_(BlockType::Empty)
            .local_get(LIST)
            .i32_eqz()
            .br_if(1);
        self.c().local_get(LIST).local_get(WORDS).i32_ge_u();
        self.reject();
        let word = self.word;
        self.c()
            .local_get(1)
            .local_get(LIST)
            .i32_const(1)
            .i32_add()
            .call(word)
            .i32_wrap_i64()
            .local_set(LIMIT)
            .i32_const(0)
            .local_set(CURSOR)
            .block(BlockType::Empty)
            .loop_(BlockType::Empty)
            .local_get(CURSOR)
            .local_get(LIMIT)
            .i32_ge_u()
            .br_if(1);
        self.c()
            .local_get(STEPS)
            .i32_const(1)
            .i32_add()
            .local_tee(STEPS)
            .local_get(WORDS)
            .i32_gt_u();
        self.reject();
        self.c()
            .local_get(1)
            .local_get(LIST)
            .i32_const(2)
            .i32_add()
            .local_get(CURSOR)
            .i32_add()
            .call(word)
            .i32_wrap_i64()
            .local_set(ROW)
            .local_get(1)
            .local_get(ROW)
            .call(word)
            .local_set(W0)
            .local_get(1)
            .local_get(ROW)
            .i32_const(1)
            .i32_add()
            .call(word)
            .local_set(W1)
            .local_get(W1)
            .i32_wrap_i64()
            .i32_const(0xffff)
            .i32_and()
            .local_set(DEST)
            .block(BlockType::Empty);
        if phase < 2 {
            self.c()
                .local_get(W1)
                .i32_wrap_i64()
                .i32_const(18)
                .i32_shr_u()
                .i32_const(1)
                .i32_and()
                .i32_const(phase as i32)
                .i32_ne()
                .br_if(0);
        } else {
            self.c().local_get(DEST).i32_eqz().br_if(0);
        }
        self.c()
            .local_get(W1)
            .i32_wrap_i64()
            .i32_const(16)
            .i32_shr_u()
            .i32_const(3)
            .i32_and()
            .local_tee(GATE)
            .if_(BlockType::Empty);
        self.load(3, p::FP_ACTIVE);
        self.c()
            .i32_eqz()
            .br_if(1)
            .local_get(W1)
            .i32_wrap_i64()
            .i32_const(24)
            .i32_shr_u()
            .local_tee(ALIASES)
            .if_(BlockType::Empty)
            .local_get(ALIASES)
            .local_get(DIRTY)
            .i32_and()
            .i32_eqz()
            .br_if(2)
            .end()
            .end();
        self.c()
            .local_get(W1)
            .i64_const(32)
            .i64_shr_u()
            .i32_wrap_i64()
            .local_tee(VALID)
            .if_(BlockType::Empty)
            .local_get(VALID)
            .i32_const(1)
            .i32_sub()
            .local_set(COLOR);
        self.address(COLOR, CELL);
        self.load(CELL, p::CELL_EPOCH);
        self.c().local_get(EPOCH).i32_ne();
        self.reject();
        self.load(CELL, p::CELL_TAG);
        self.c().i32_const(0).i32_ge_s();
        self.reject();
        self.wide(CELL, p::CELL_BITS);
        self.c()
            .i64_eqz()
            .local_get(GATE)
            .i32_const(2)
            .i32_eq()
            .i32_eq()
            .br_if(1)
            .end();
        self.c()
            .local_get(W0)
            .i32_wrap_i64()
            .local_tee(MARKER)
            .i32_const(0)
            .i32_lt_s()
            .if_(BlockType::Empty)
            .local_get(MARKER)
            .i32_const(-1)
            .i32_mul()
            .local_set(MARKER)
            .end()
            .local_get(MARKER)
            .i32_eqz();
        self.reject();
        self.c().local_get(MARKER).local_get(REFS).i32_gt_u();
        self.reject();
        self.c()
            .local_get(W0)
            .i64_const(32)
            .i64_shr_u()
            .i32_wrap_i64()
            .local_set(COLOR);
        if phase < 2 {
            self.mark();
        } else {
            self.address(COLOR, CELL);
            self.ready(CELL, MARKER);
            self.wide(CELL, p::CELL_BITS);
            self.c().local_set(VALUE);
            self.output_dynamic();
            self.c()
                .local_get(DEST)
                .i32_const(76)
                .i32_ge_u()
                .local_get(DEST)
                .i32_const(84)
                .i32_lt_u()
                .i32_and()
                .if_(BlockType::Empty);
            self.output_get(85);
            self.c()
                .i32_wrap_i64()
                .i32_const(1)
                .local_get(DEST)
                .i32_const(76)
                .i32_sub()
                .i32_shl()
                .i32_or()
                .i64_extend_i32_u()
                .local_set(VALUE);
            self.output(85);
            self.c().end();
        }
        self.c()
            .end()
            .local_get(CURSOR)
            .i32_const(1)
            .i32_add()
            .local_set(CURSOR)
            .br(0)
            .end()
            .end();
        self.c()
            .local_get(1)
            .local_get(LIST)
            .call(word)
            .i32_wrap_i64()
            .local_set(LIST)
            .br(0)
            .end()
            .end();
    }
    fn closure_marker(&mut self) {
        let word = self.word;
        self.c()
            .local_get(1)
            .local_get(CLOSURE)
            .local_get(CURSOR)
            .i32_const(1)
            .i32_shr_u()
            .i32_add()
            .call(word)
            .local_get(CURSOR)
            .i32_const(1)
            .i32_and()
            .i32_const(5)
            .i32_shl()
            .i64_extend_i32_u()
            .i64_shr_u()
            .i32_wrap_i64()
            .local_set(MARKER);
        self.c().local_get(MARKER).i32_eqz();
        self.reject();
        self.c().local_get(MARKER).local_get(REFS).i32_gt_u();
        self.reject();
        self.c()
            .local_get(BASE)
            .local_get(MARKER)
            .i32_const(1)
            .i32_sub()
            .i32_const(2)
            .i32_shl()
            .i32_add()
            .local_set(RECORD);
        self.record(0);
        self.c()
            .local_tee(W0)
            .i32_wrap_i64()
            .i32_const(255)
            .i32_and()
            .local_set(KIND)
            .local_get(W0)
            .i64_const(32)
            .i64_shr_u()
            .i32_wrap_i64()
            .local_set(COLOR);
        self.address(COLOR, CELL);
    }
    fn sweep(&mut self, forward: bool) {
        self.c()
            .i32_const(if forward { 0 } else { -1 })
            .local_set(MODE)
            .local_get(CLOSURE_COUNT)
            .local_set(CURSOR);
        if forward {
            self.c().i32_const(0).local_set(CURSOR);
        }
        self.c().block(BlockType::Empty).loop_(BlockType::Empty);
        if forward {
            self.c()
                .local_get(CURSOR)
                .local_get(CLOSURE_COUNT)
                .i32_ge_u()
                .br_if(1);
        } else {
            self.c()
                .local_get(CURSOR)
                .i32_eqz()
                .br_if(1)
                .local_get(CURSOR)
                .i32_const(1)
                .i32_sub()
                .local_set(CURSOR);
        }
        self.closure_marker();
        self.load(CELL, p::CELL_EPOCH);
        self.c().local_get(EPOCH).i32_eq();
        self.load(CELL, p::CELL_TAG);
        self.c()
            .local_get(MARKER)
            .i32_eq()
            .i32_and()
            .if_(BlockType::Empty);
        if forward {
            self.c().local_get(KIND).i32_const(1).i32_le_u();
            self.reject();
        }
        self.record(1);
        self.c()
            .local_tee(VALUE)
            .i32_wrap_i64()
            .local_set(ARG)
            .local_get(VALUE)
            .i64_const(32)
            .i64_shr_u()
            .i32_wrap_i64()
            .local_set(ARGHI)
            .i32_const(0)
            .local_set(DEP);
        for dest in [D0, D1, D2, D3] {
            self.c().i64_const(0).local_set(dest);
        }
        self.c()
            .block(BlockType::Empty)
            .loop_(BlockType::Empty)
            .local_get(DEP)
            .i32_const(4)
            .i32_ge_u()
            .br_if(1);
        let word = self.word;
        self.c()
            .local_get(1)
            .local_get(RECORD)
            .i32_const(2)
            .i32_add()
            .local_get(DEP)
            .i32_const(1)
            .i32_shr_u()
            .i32_add()
            .call(word)
            .local_get(DEP)
            .i32_const(1)
            .i32_and()
            .i32_const(5)
            .i32_shl()
            .i64_extend_i32_u()
            .i64_shr_u()
            .i32_wrap_i64()
            .local_tee(DEPMARK)
            .if_(BlockType::Empty)
            .local_get(DEPMARK)
            .local_get(MARKER)
            .i32_ge_u();
        self.reject();
        self.c()
            .local_get(1)
            .local_get(BASE)
            .local_get(DEPMARK)
            .i32_const(1)
            .i32_sub()
            .i32_const(2)
            .i32_shl()
            .i32_add()
            .call(word)
            .i64_const(32)
            .i64_shr_u()
            .i32_wrap_i64()
            .local_set(DEPCOLOR);
        self.address(DEPCOLOR, DEPCELL);
        if forward {
            self.ready(DEPCELL, DEPMARK);
            self.wide(DEPCELL, p::CELL_BITS);
            self.c().local_set(RESULT);
            for (index, dest) in [D0, D1, D2, D3].into_iter().enumerate() {
                self.c()
                    .local_get(DEP)
                    .i32_const(index as i32)
                    .i32_eq()
                    .if_(BlockType::Empty)
                    .local_get(RESULT)
                    .local_set(dest)
                    .end();
            }
        } else {
            self.load(DEPCELL, p::CELL_EPOCH);
            self.c()
                .local_get(EPOCH)
                .i32_ne()
                .if_(BlockType::Empty)
                .local_get(DEPCELL)
                .local_get(EPOCH)
                .i32_store(mem(u64::from(p::CELL_EPOCH), 0))
                .local_get(DEPCELL)
                .i32_const(0)
                .i32_store(mem(u64::from(p::CELL_TAG), 0))
                .end();
            self.load(DEPCELL, p::CELL_TAG);
            self.c()
                .local_get(DEPMARK)
                .i32_const(-1)
                .i32_mul()
                .i32_ne()
                .if_(BlockType::Empty)
                .local_get(DEPCELL)
                .local_get(DEPMARK)
                .i32_store(mem(u64::from(p::CELL_TAG), 0))
                .end();
        }
        self.c()
            .end()
            .local_get(DEP)
            .i32_const(1)
            .i32_add()
            .local_set(DEP)
            .br(0)
            .end()
            .end();
        if forward {
            sources::evaluate(self);
            self.c()
                .local_get(CELL)
                .local_get(RESULT)
                .i64_store(mem(u64::from(p::CELL_BITS), 0))
                .local_get(CELL)
                .local_get(MARKER)
                .i32_const(-1)
                .i32_mul()
                .i32_store(mem(u64::from(p::CELL_TAG), 0));
        }
        self.c().end();
        if forward {
            self.c()
                .local_get(CURSOR)
                .i32_const(1)
                .i32_add()
                .local_set(CURSOR);
        }
        self.c().br(0).end().end();
    }
    fn output(&mut self, local: u32) {
        let role = p::output_role(local).expect("finite recovery output role");
        self.c()
            .local_get(3)
            .local_get(VALUE)
            .i64_store(mem(u64::from(p::OUTPUTS + 8 * role), 0))
            .local_get(3)
            .local_get(3)
            .i64_load(mem(u64::from(p::OUTPUT_MASK), 0))
            .i64_const((1u64 << role) as i64)
            .i64_or()
            .i64_store(mem(u64::from(p::OUTPUT_MASK), 0));
    }
    fn output_get(&mut self, local: u32) {
        let role = p::output_role(local).expect("finite recovery output role");
        self.wide(3, p::OUTPUTS + 8 * role);
    }
    fn output_dynamic(&mut self) {
        self.c().i32_const(-1).local_set(ROLE);
        for (role, &local) in p::OUTPUT_LOCALS.iter().enumerate() {
            self.c()
                .local_get(DEST)
                .i32_const(local as i32)
                .i32_eq()
                .if_(BlockType::Empty)
                .i32_const(role as i32)
                .local_set(ROLE)
                .end();
        }
        self.c().local_get(ROLE).i32_const(-1).i32_eq();
        self.reject();
        self.c()
            .local_get(3)
            .i32_const(p::OUTPUTS as i32)
            .i32_add()
            .local_get(ROLE)
            .i32_const(3)
            .i32_shl()
            .i32_add()
            .local_get(VALUE)
            .i64_store(mem(0, 0))
            .local_get(3)
            .local_get(3)
            .i64_load(mem(u64::from(p::OUTPUT_MASK), 0))
            .i64_const(1)
            .local_get(ROLE)
            .i64_extend_i32_u()
            .i64_shl()
            .i64_or()
            .i64_store(mem(u64::from(p::OUTPUT_MASK), 0));
    }
    fn finish(&mut self) {
        self.field(1);
        self.c()
            .local_tee(W0)
            .i32_wrap_i64()
            .i32_const(1 << 16)
            .i32_and()
            .i32_eqz()
            .i32_eqz();
        self.load(3, p::FP_ACTIVE);
        self.c().i32_and().if_(BlockType::Empty);
        self.field(2);
        self.c().local_set(VALUE);
        self.output(177);
        self.c().i64_const(0).local_set(VALUE);
        self.output(85);
        self.c()
            .local_get(DIRTY)
            .i64_extend_i32_u()
            .local_set(VALUE);
        self.output(158);
        self.c().end();
        self.row_loop(2);
        self.field(4);
        self.c()
            .local_tee(W0)
            .i32_wrap_i64()
            .local_tee(TEMP)
            .if_(BlockType::Empty)
            .local_get(TEMP)
            .i32_const(1)
            .i32_sub()
            .i64_extend_i32_u()
            .local_set(VALUE);
        self.output(20);
        self.c()
            .local_get(W0)
            .i64_const(32)
            .i64_shr_u()
            .local_set(VALUE);
        self.output(25);
        self.c().i64_const(0).local_set(VALUE);
        self.output(24);
        self.c().end();
        self.field(1);
        self.c()
            .i64_const(32)
            .i64_shr_u()
            .i32_wrap_i64()
            .local_tee(FRAME)
            .i32_const(1)
            .i32_eq()
            .if_(BlockType::Empty)
            .i64_const(0)
            .local_set(VALUE);
        self.output(109);
        self.c().else_().local_get(FRAME).i32_const(2).i32_ge_u();
        self.load(3, p::FP_ACTIVE);
        self.c().i32_and();
        self.load(3, p::EXACT_SUFFIX);
        self.c().i32_eqz().i32_and();
        self.load(3, p::SELECTED);
        self.c().i32_eqz().i32_eqz().i32_and().if_(BlockType::Empty);
        self.field(3);
        self.c()
            .local_tee(W0)
            .i64_const(48)
            .i64_shr_u()
            .i32_wrap_i64()
            .i32_const(1)
            .i32_and()
            .if_(BlockType::Empty)
            .local_get(W0)
            .i32_wrap_i64()
            .i64_extend_i32_u()
            .local_set(VALUE);
        self.output(91);
        self.c()
            .local_get(W0)
            .i64_const(32)
            .i64_shr_u()
            .i64_const(0xffff)
            .i64_and()
            .local_set(VALUE);
        self.output(93);
        self.load(3, p::BANK);
        self.c()
            .i32_load16_u(mem(102, 0))
            .i64_extend_i32_u()
            .local_set(VALUE);
        self.output(94);
        self.c()
            .end()
            .local_get(W0)
            .i64_const(49)
            .i64_shr_u()
            .i32_wrap_i64()
            .i32_const(7)
            .i32_and()
            .local_tee(TEMP)
            .if_(BlockType::Empty);
        self.load(3, p::BANK);
        self.c()
            .local_get(TEMP)
            .i32_const(1)
            .i32_sub()
            .i32_const(1)
            .i32_shl()
            .i32_add()
            .i32_load16_u(mem(100, 0))
            .i64_extend_i32_u()
            .local_set(VALUE);
        self.output(95);
        self.c()
            .end()
            .local_get(FRAME)
            .i32_const(1)
            .i32_sub()
            .i64_extend_i32_u()
            .local_set(VALUE);
        self.output(109);
        self.c().end().end();
        self.field(5);
        self.c()
            .i32_wrap_i64()
            .local_tee(TEMP)
            .if_(BlockType::Empty);
        self.c().local_get(TEMP);
        self.load(3, p::PREFIX_BASE);
        self.c().i32_sub().local_set(TEMP);
        self.load(3, p::REMAIN);
        self.c()
            .local_get(TEMP)
            .i32_sub()
            .i64_extend_i32_u()
            .local_set(VALUE);
        self.output(12);
        self.load(3, p::RETIRED);
        self.c()
            .local_get(TEMP)
            .i32_add()
            .i64_extend_i32_u()
            .local_set(VALUE);
        self.output(13);
        self.c().i64_const(0).local_set(VALUE);
        self.output(131);
        self.c().end();
        self.field(1);
        self.c()
            .i32_wrap_i64()
            .i32_const(1 << 17)
            .i32_and()
            .if_(BlockType::Empty);
        self.field(5);
        self.c().i64_const(32).i64_shr_u().local_set(VALUE);
        self.output(10);
        self.c().end();
    }
}

pub(super) fn emit(word: u32, continuation_global: u32) -> Function {
    let mut v = Vm {
        f: Function::new([(40, ValType::I32), (13, ValType::I64)]),
        word,
        continuation: continuation_global,
    };
    v.auth();
    v.load(3, p::STATE);
    v.c().i32_eqz().if_(BlockType::Empty);
    v.c()
        .local_get(EPOCH)
        .i32_const(1)
        .i32_add()
        .local_tee(EPOCH)
        .i32_eqz()
        .if_(BlockType::Empty)
        .i32_const(0)
        .local_set(COLOR)
        .block(BlockType::Empty)
        .loop_(BlockType::Empty)
        .local_get(COLOR)
        .i32_const(p::MAX_CAPTURE_COLORS as i32)
        .i32_ge_u()
        .br_if(1)
        .local_get(3)
        .i32_const(p::CELLS_OFFSET as i32)
        .i32_add()
        .local_get(COLOR)
        .i32_const(4)
        .i32_shl()
        .i32_add()
        .i64_const(0)
        .i64_store(mem(8, 0))
        .local_get(COLOR)
        .i32_const(1)
        .i32_add()
        .local_set(COLOR)
        .br(0)
        .end()
        .end()
        .i32_const(1)
        .local_set(EPOCH)
        .end();
    v.store(p::EPOCH, EPOCH);
    v.c()
        .local_get(3)
        .i64_const(0)
        .i64_store(mem(u64::from(p::OUTPUT_MASK), 0));
    v.row_loop(0);
    v.sweep(false);
    v.put(p::STATE, p::PHASE_ZERO as i32);
    v.c().i32_const(p::CAPTURE_PHASE_ZERO).return_().end();
    v.load(3, p::STATE);
    v.c()
        .i32_const(p::PHASE_ZERO as i32)
        .i32_eq()
        .if_(BlockType::Empty);
    v.sweep(true);
    v.row_loop(1);
    v.sweep(false);
    v.put(p::STATE, p::PHASE_ONE as i32);
    v.c().i32_const(p::CAPTURE_PHASE_ONE).return_().end();
    v.load(3, p::STATE);
    v.c()
        .i32_const(p::PHASE_ONE as i32)
        .i32_eq()
        .if_(BlockType::Empty);
    v.sweep(true);
    v.finish();
    v.put(p::STATE, p::READY as i32);
    v.c()
        .i32_const(p::DONE)
        .return_()
        .end()
        .i32_const(p::REJECT)
        .end();
    v.f
}
pub(super) fn emit_word() -> Function {
    data::emit()
}
