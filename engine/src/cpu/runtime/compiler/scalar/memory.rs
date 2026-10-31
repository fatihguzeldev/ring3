use super::*;

const PAGE: u32 = 26;
const SLOT: u32 = 27;
const HOST: u32 = 28;
const READY: u32 = 29;
const LENGTH: u32 = 30;
const BIT: u32 = 31;
const CACHE: u32 = 79;
const CACHE_ENTRY: u32 = 80;
const CACHE_FLAGS: u32 = 81;
const OWNER: u32 = 82;
const CACHE_RECORD: u32 = 83;
const CACHE_ROWS: u32 = 84;

pub(super) fn initialize_bound(out: &mut Output, binding: NativeMemoryBinding) {
    out.c()
        .i32_const(binding.header() as i32)
        .local_set(CACHE)
        .i32_const(binding.rows() as i32)
        .local_set(CACHE_ROWS);
    pack_owner(out);
}

fn pack_owner(out: &mut Output) {
    out.c()
        .local_get(CACHE)
        .i32_load(mem(0))
        .i64_extend_i32_u()
        .i64_const(32)
        .i64_shl()
        .local_get(0)
        .i32_load(mem(84))
        .i64_extend_i32_u()
        .i64_or()
        .local_set(OWNER);
}

pub(super) fn initialize(out: &mut Output) {
    out.c()
        .local_get(0)
        .i32_load(mem(72))
        .local_tee(CACHE)
        .i32_eqz()
        .i32_eqz()
        .local_get(CACHE)
        .i32_const(15)
        .i32_and()
        .i32_eqz()
        .i32_and()
        .local_get(CACHE)
        .i64_extend_i32_u()
        .i64_const(32)
        .i64_add()
        .memory_size(0)
        .i64_extend_i32_u()
        .i64_const(16)
        .i64_shl()
        .i64_le_u()
        .i32_and()
        .if_(BlockType::Empty)
        .local_get(CACHE)
        .i32_load(mem(4))
        .i32_const(0x434d_3352)
        .i32_eq()
        .local_get(CACHE)
        .i32_load(mem(8))
        .i32_const(32)
        .i32_eq()
        .i32_and()
        .local_get(CACHE)
        .i32_load(mem(12))
        .i32_const(2)
        .i32_eq()
        .i32_and()
        .local_get(CACHE)
        .i32_load(mem(0))
        .i64_extend_i32_u()
        .i64_const(32)
        .i64_shl()
        .local_tee(OWNER)
        .i64_eqz()
        .i32_eqz()
        .i32_and()
        .local_get(CACHE)
        .i32_load(mem(20))
        .i32_const(1 << 20)
        .i32_eq()
        .i32_and()
        .local_get(CACHE)
        .i32_load(mem(24))
        .i32_eqz()
        .i32_and()
        .local_get(CACHE)
        .i32_load(mem(28))
        .i32_eqz()
        .i32_and()
        .local_get(CACHE)
        .i32_load(mem(16))
        .local_tee(CACHE_ROWS)
        .i32_eqz()
        .i32_eqz()
        .i32_and()
        .local_get(CACHE_ROWS)
        .i32_const(7)
        .i32_and()
        .i32_eqz()
        .i32_and()
        .local_get(CACHE_ROWS)
        .i64_extend_i32_u()
        .i64_const(8 * 1024 * 1024)
        .i64_add()
        .memory_size(0)
        .i64_extend_i32_u()
        .i64_const(16)
        .i64_shl()
        .i64_le_u()
        .i32_and()
        .i32_eqz()
        .if_(BlockType::Empty)
        .i32_const(0)
        .local_set(CACHE)
        .end()
        .else_()
        .i32_const(0)
        .local_set(CACHE)
        .end()
        .local_get(OWNER)
        .local_get(0)
        .i32_load(mem(84))
        .i64_extend_i32_u()
        .i64_or()
        .local_set(OWNER);
}

fn reload(out: &mut Output) {
    out.c().local_get(CACHE).if_(BlockType::Empty);
    pack_owner(out);
    out.c().end();
}

// only a completed instruction may release this scratch; paired RMW stores
// and multiword accesses consume their translation proof before this boundary.
pub(super) fn release_scratch(out: &mut Output) {
    out.c()
        .i32_const(0)
        .local_set(CACHE_ENTRY)
        .i32_const(0)
        .local_set(CACHE_FLAGS)
        .i64_const(0)
        .local_set(CACHE_RECORD);
}

fn access(width: u32) -> MemArg {
    MemArg {
        offset: 0,
        align: match width {
            1 => 0,
            2 => 1,
            _ => 2,
        },
        memory_index: 0,
    }
}

fn translate(out: &mut Output, width: u32, required: u32) {
    out.c()
        .i32_const(0)
        .local_set(READY)
        .i32_const(0)
        .local_set(CACHE_FLAGS)
        .local_get(ADDRESS)
        .i32_const(12)
        .i32_shr_u()
        .local_set(PAGE)
        .local_get(CACHE)
        .if_(BlockType::Empty)
        .local_get(ADDRESS)
        .i32_const(4095)
        .i32_and()
        .i32_const((4096 - width) as i32)
        .i32_le_u()
        .if_(BlockType::Empty)
        .local_get(CACHE_ROWS)
        .local_get(PAGE)
        .i32_const(3)
        .i32_shl()
        .i32_add()
        .local_tee(CACHE_ENTRY)
        .i64_load(MemArg {
            offset: 0,
            align: 3,
            memory_index: 0,
        })
        .local_tee(CACHE_RECORD)
        .local_get(OWNER)
        .i64_xor()
        .i64_const(32)
        .i64_shr_u()
        .i64_eqz()
        .local_get(CACHE_RECORD)
        .i32_wrap_i64()
        .local_tee(CACHE_FLAGS)
        .i32_const(required as i32)
        .i32_and()
        .i32_const(required as i32)
        .i32_eq()
        .i32_and()
        .if_(BlockType::Empty)
        .local_get(CACHE_FLAGS)
        .i32_const(12)
        .i32_shr_u()
        .local_tee(SLOT)
        .i32_const(12)
        .i32_shl()
        .local_get(OWNER)
        .i32_wrap_i64()
        .i32_add()
        .local_get(ADDRESS)
        .i32_const(4095)
        .i32_and()
        .i32_add()
        .local_set(HOST)
        .local_get(CACHE_FLAGS)
        .i32_const(7)
        .i32_and()
        .local_set(CACHE_FLAGS)
        .i32_const(1)
        .local_set(READY)
        .end()
        .end()
        .end()
        .local_get(READY)
        .i32_eqz()
        .if_(BlockType::Empty)
        .i32_const(0)
        .local_set(CACHE_FLAGS);
    translate_uncached(out, width, required);
    out.c()
        .local_get(CACHE_FLAGS)
        .i32_const(3)
        .i32_and()
        .local_set(CACHE_FLAGS)
        .local_get(READY)
        .local_get(CACHE)
        .i32_eqz()
        .i32_eqz()
        .i32_and()
        .local_get(SLOT)
        .i32_const(0x000f_ffff)
        .i32_le_u()
        .i32_and()
        .if_(BlockType::Empty);
    cache_store(out);
    out.c().end().end();
}

fn cache_store(out: &mut Output) {
    out.c()
        .local_get(CACHE_ENTRY)
        .local_get(OWNER)
        .i64_const(-0x1_0000_0000)
        .i64_and()
        .local_get(SLOT)
        .i32_const(12)
        .i32_shl()
        .local_get(CACHE_FLAGS)
        .i32_const(7)
        .i32_and()
        .i32_or()
        .i64_extend_i32_u()
        .i64_or()
        .i64_store(MemArg {
            offset: 0,
            align: 3,
            memory_index: 0,
        });
}

fn translate_uncached(out: &mut Output, width: u32, required: u32) {
    out.c()
        .local_get(0)
        .local_get(ADDRESS)
        .i32_const(width as i32)
        .i32_const(required as i32)
        .call(4)
        .local_set(CACHE_FLAGS)
        .local_set(SLOT)
        .local_set(HOST)
        .local_get(CACHE_FLAGS)
        .i32_eqz()
        .i32_eqz()
        .local_set(READY);
}

pub(super) fn read(out: &mut Output, width: u32, destination: u32) {
    let bytes = width & 255;
    out.segment(bytes, width & 0x100 != 0);
    translate(out, bytes, if width & 0x100 != 0 { 3 } else { 1 });
    out.c()
        .local_get(READY)
        .if_(BlockType::Empty)
        .local_get(HOST);
    match bytes {
        1 => {
            out.c().i32_load8_u(access(1));
        }
        2 => {
            out.c().i32_load16_u(access(2));
        }
        _ => {
            out.c().i32_load(access(4));
        }
    }
    out.c()
        .local_set(destination)
        .else_()
        .local_get(0)
        .local_get(ADDRESS)
        .i32_const(width as i32)
        .call(0)
        .local_set(destination);
    reload(out);
    out.c().local_get(0).i32_load(mem(52)).local_set(TEMP);
    out.failed(TEMP, false);
    out.c().end();
}

fn admit_store(out: &mut Output) {
    out.c()
        .local_get(READY)
        .local_get(CACHE)
        .i32_eqz()
        .i32_eqz()
        .i32_and()
        .local_get(CACHE_FLAGS)
        .i32_const(4)
        .i32_and()
        .i32_eqz()
        .i32_eqz()
        .i32_and()
        .if_(BlockType::Empty)
        .local_get(0)
        .i32_load(mem(108))
        .local_tee(LENGTH)
        .local_get(0)
        .i32_load(mem(112))
        .i32_le_u()
        .local_get(SLOT)
        .local_get(0)
        .i32_load(mem(112))
        .i32_lt_u()
        .i32_and()
        .local_get(0)
        .i32_load(mem(156))
        .i64_load(MemArg {
            offset: 0,
            align: 3,
            memory_index: 0,
        })
        .i64_const(-1)
        .i64_ne()
        .i32_and()
        .local_set(READY)
        .else_();
    admit_store_uncached(out);
    out.c()
        .local_get(READY)
        .local_get(CACHE)
        .i32_eqz()
        .i32_eqz()
        .i32_and()
        .local_get(SLOT)
        .i32_const(0x000f_ffff)
        .i32_le_u()
        .i32_and()
        .if_(BlockType::Empty)
        .local_get(CACHE_FLAGS)
        .i32_const(4)
        .i32_or()
        .local_set(CACHE_FLAGS);
    cache_store(out);
    out.c().end().end();
    out.c()
        .local_get(READY)
        .if_(BlockType::Empty)
        .local_get(0)
        .i32_load(mem(100))
        .local_get(SLOT)
        .i32_add()
        .i32_load8_u(access(1))
        .local_tee(BIT)
        .i32_eqz()
        .if_(BlockType::Empty)
        .local_get(LENGTH)
        .local_get(0)
        .i32_load(mem(112))
        .i32_lt_u()
        .local_set(READY)
        .end()
        .end();
}

fn admit_store_uncached(out: &mut Output) {
    out.c()
        .local_get(READY)
        .if_(BlockType::Empty)
        .local_get(0)
        .local_get(PAGE)
        .local_get(SLOT)
        .call(5)
        .local_set(BIT)
        .local_set(LENGTH)
        .local_set(READY)
        .end();
}

pub(super) fn write(out: &mut Output, width: u32, value: u32) {
    out.segment(width, true);
    translate(out, width, 2);
    store(out, width, value);
}

// only the paired store of the same instruction may consume this proof, with
// the same width and address and no intervening access or helper. a slow read
// leaves READY zero and takes the canonical write.
pub(super) fn write_after_modify(out: &mut Output, width: u32, value: u32) {
    store(out, width, value);
    out.c().i32_const(0).local_set(READY);
}

fn store(out: &mut Output, width: u32, value: u32) {
    out.emitted_guest_write = true;
    admit_store(out);
    out.c().local_get(READY).if_(BlockType::Empty);
    record_store(out);
    out.c().local_get(HOST).local_get(value);
    match width {
        1 => {
            out.c().i32_store8(access(1));
        }
        2 => {
            out.c().i32_store16(access(2));
        }
        _ => {
            out.c().i32_store(access(4));
        }
    }
    out.c()
        .i32_const(0)
        .local_set(WRITE_STATUS)
        .else_()
        .local_get(0)
        .local_get(ADDRESS)
        .i32_const(width as i32)
        .local_get(value)
        .call(1)
        .local_set(WRITE_STATUS);
    reload(out);
    out.failed(WRITE_STATUS, true);
    out.c().end();
}

fn record_store(out: &mut Output) {
    out.c()
        .local_get(BIT)
        .i32_eqz()
        .if_(BlockType::Empty)
        .local_get(0)
        .i32_load(mem(104))
        .local_get(LENGTH)
        .i32_const(2)
        .i32_shl()
        .i32_add()
        .local_get(SLOT)
        .i32_store(mem(0))
        .local_get(0)
        .i32_load(mem(100))
        .local_get(SLOT)
        .i32_add()
        .i32_const(1)
        .i32_store8(access(1))
        .local_get(0)
        .local_get(LENGTH)
        .i32_const(1)
        .i32_add()
        .i32_store(mem(108))
        .end();
}

fn wide_access(out: &mut Output, words: &[u32], alignment: u32, write: bool) {
    assert!(matches!(words.len(), 2 | 4));
    assert!(alignment.is_power_of_two() && alignment <= 16);
    let width = words.len() as u32 * 4;
    out.segment(width, write);
    if alignment != 1 {
        out.c()
            .local_get(ADDRESS)
            .i32_const((alignment - 1) as i32)
            .i32_and()
            .if_(BlockType::Empty);
        out.exit(7);
        out.c().end();
    }
    translate(out, width, if write { 2 } else { 1 });
    if write {
        admit_store(out);
    }
    out.c().local_get(READY).i32_eqz().if_(BlockType::Empty);
    out.exit(7);
    out.c().end();
}

// wide accesses only commit within one fully admitted ordinary page. all other
// cases return the whole instruction to its canonical service before effects.
pub(super) fn read_words(out: &mut Output, words: &[u32], alignment: u32) {
    wide_access(out, words, alignment, false);
    for (index, &local) in words.iter().enumerate() {
        out.c()
            .local_get(HOST)
            .i32_load(mem(index as u64 * 4))
            .local_set(local);
    }
}

pub(super) fn write_words(out: &mut Output, words: &[u32], alignment: u32) {
    out.emitted_guest_write = true;
    wide_access(out, words, alignment, true);
    record_store(out);
    for (index, &local) in words.iter().enumerate() {
        out.c()
            .local_get(HOST)
            .local_get(local)
            .i32_store(mem(index as u64 * 4));
    }
    out.c().i32_const(0).local_set(WRITE_STATUS);
}
