use super::*;

const PAGE: u32 = 26;
const SLOT: u32 = 27;
const HOST: u32 = 28;
const READY: u32 = 29;
const LENGTH: u32 = 30;
const BIT: u32 = 31;

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

fn present(out: &mut Output, fields: &[u64]) {
    for (index, &field) in fields.iter().enumerate() {
        out.c()
            .local_get(0)
            .i32_load(mem(field))
            .i32_eqz()
            .i32_eqz();
        if index != 0 {
            out.c().i32_and();
        }
    }
}

fn translate(out: &mut Output, width: u32, required: u32) {
    out.c()
        .i32_const(0)
        .local_set(READY)
        .local_get(ADDRESS)
        .i32_const(12)
        .i32_shr_u()
        .local_set(PAGE);
    present(out, &[76, 80, 84]);
    out.c()
        .local_get(ADDRESS)
        .i32_const(4095)
        .i32_and()
        .i32_const((4096 - width) as i32)
        .i32_le_u()
        .i32_and()
        .if_(BlockType::Empty)
        .local_get(0)
        .i32_load(mem(76))
        .local_get(PAGE)
        .i32_const(2)
        .i32_shl()
        .i32_add()
        .i32_load(mem(0))
        .local_tee(SLOT)
        .i32_eqz()
        .i32_eqz()
        .local_get(SLOT)
        .local_get(0)
        .i32_load(mem(88))
        .i32_le_u()
        .i32_and()
        .if_(BlockType::Empty)
        .local_get(0)
        .i32_load(mem(80))
        .local_get(PAGE)
        .i32_add()
        .i32_load8_u(access(1))
        .i32_const((required | 8) as i32)
        .i32_and()
        .i32_const(required as i32)
        .i32_eq()
        .if_(BlockType::Empty)
        .local_get(SLOT)
        .i32_const(1)
        .i32_sub()
        .local_tee(SLOT)
        .i32_const(12)
        .i32_shl()
        .local_get(0)
        .i32_load(mem(84))
        .i32_add()
        .local_get(ADDRESS)
        .i32_const(4095)
        .i32_and()
        .i32_add()
        .local_set(HOST)
        .i32_const(1)
        .local_set(READY)
        .end()
        .end()
        .end();
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
        .local_set(destination)
        .local_get(0)
        .i32_load(mem(52))
        .local_set(TEMP);
    out.failed(TEMP, false);
    out.c().end();
}

fn admit_store(out: &mut Output) {
    out.c().local_get(READY).if_(BlockType::Empty);
    present(out, &[92, 100, 104, 156]);
    out.c()
        .if_(BlockType::Empty)
        .local_get(0)
        .i32_load(mem(108))
        .local_set(LENGTH)
        .local_get(LENGTH)
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
        .local_get(0)
        .i32_load(mem(92))
        .local_get(SLOT)
        .i32_const(2)
        .i32_shl()
        .i32_add()
        .i32_load(mem(0))
        .i32_eqz()
        .i32_and()
        .local_get(0)
        .i32_load(mem(96))
        .local_tee(BIT)
        .if_(BlockType::Result(ValType::I32))
        .local_get(BIT)
        .local_get(PAGE)
        .i32_const(2)
        .i32_shl()
        .i32_add()
        .i32_load(mem(0))
        .i32_eqz()
        .else_()
        .i32_const(1)
        .end()
        .i32_and()
        .local_set(READY)
        .else_()
        .i32_const(0)
        .local_set(READY)
        .end()
        .end()
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
