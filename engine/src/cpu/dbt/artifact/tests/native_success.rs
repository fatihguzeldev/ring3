use super::*;
use crate::{
    cpu::x86::{SegmentContext, decode::DecodeAuthority},
    memory::{PageRange, Permissions},
};

fn memory(bytes: &[u8]) -> AddressSpace {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    memory
}

fn prepared(memory: &AddressSpace, length: usize) -> PreparedRegion {
    prepare_embedded_region(
        memory,
        &[BlockSpec {
            entry: GuestAddress(0x1000),
            byte_length: length as u32,
        }],
        CompileLimits::default(),
        &[],
        CodeFetchContext::from_segments(&SegmentContext::default()),
        DecodeAuthority::default(),
    )
    .unwrap()
}

fn region(memory: &AddressSpace, length: usize) -> CompiledRegion {
    let prepared = prepared(memory, length);
    let binding = Some(wasm::EmbeddedBinding::Resident { key: 7, id: 1 });
    let tier = super::super::CompilationTier32::Optimized;
    let bytes = wasm::emit_with_tier(&prepared.blocks, binding, prepared.decode_key, tier);
    CompiledRegion::new(prepared, bytes, tier, binding).unwrap()
}

fn unsigned(bytes: &mut &[u8]) -> u32 {
    let mut result = 0;
    for shift in (0..35).step_by(7) {
        let (byte, rest) = bytes.split_first().unwrap();
        *bytes = rest;
        result |= u32::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return result;
        }
    }
    panic!("invalid authored wasm integer");
}

fn string(bytes: &mut &[u8]) -> String {
    let length = unsigned(bytes) as usize;
    let (value, rest) = bytes.split_at(length);
    *bytes = rest;
    std::str::from_utf8(value).unwrap().to_owned()
}

fn imports(module: &[u8]) -> Vec<(String, String, u32)> {
    assert_eq!(&module[..8], b"\0asm\x01\0\0\0");
    let mut input = &module[8..];
    while !input.is_empty() {
        let section = input[0];
        input = &input[1..];
        let length = unsigned(&mut input) as usize;
        let (mut payload, rest) = input.split_at(length);
        input = rest;
        if section != 2 {
            continue;
        }
        let count = unsigned(&mut payload);
        let mut result = Vec::new();
        for _ in 0..count {
            let namespace = string(&mut payload);
            let name = string(&mut payload);
            let kind = payload[0];
            payload = &payload[1..];
            let index = match kind {
                0 => unsigned(&mut payload),
                2 => {
                    assert_eq!(unsigned(&mut payload), 0);
                    assert_eq!(unsigned(&mut payload), 1);
                    u32::MAX
                }
                _ => panic!("unexpected resident import kind"),
            };
            result.push((namespace, name, index));
        }
        assert!(payload.is_empty());
        return result;
    }
    panic!("missing resident imports");
}

#[test]
fn native_success_sibling_preserves_checked_bytes_and_native_helper_import_order() {
    let bytes = [
        0x8b, 0x03, // mov eax,[ebx]
        0x83, 0x00, 1, // add dword [eax],1
        0xd9, 0x43, 0x10, // fld dword [ebx+16]
        0xd8, 0x43, 0x14, // fadd dword [ebx+20]
        0xd8, 0x4b, 0x18, // fmul dword [ebx+24]
        0xd9, 0x5b, 0x1c, // fstp dword [ebx+28]
        0xff, 0xe6, // jmp esi
    ];
    let memory = memory(&bytes);
    let region = region(&memory, bytes.len());
    let original = region.wasm_bytes(&memory).unwrap().to_vec();
    let sibling = region.emit_native_success(7, 1).unwrap();
    assert_ne!(sibling, original);
    let checked = imports(&original);
    let mut native = imports(&sibling);
    let guard = native
        .iter_mut()
        .find(|(_, name, _)| name == "guard_detect_resident_native_success")
        .expect("private entry must require the native-success admission");
    guard.1 = "guard_detect_resident".to_owned();
    assert_eq!(native, checked, "same helper names, order and type indices");
    assert_eq!(region.wasm_bytes(&memory).unwrap(), original);
    assert_eq!(region.metadata().instructions, 7);
    assert_eq!(region.native_x87_group(0).unwrap().steps().len(), 4);
}

#[test]
fn native_success_preserves_scalar_pair_and_wide_group_final_ordinals() {
    for length in [1, 2, 4, 32] {
        let bytes = [0xd9, 0xe8].repeat(length);
        let memory = memory(&bytes);
        let region = region(&memory, bytes.len());
        let before = region.wasm_bytes(&memory).unwrap().to_vec();
        let private = region.emit_native_success(7, 1).unwrap();
        assert!(
            imports(&private)
                .iter()
                .any(|(_, name, _)| name == "guard_detect_resident_native_success")
        );
        assert_eq!(region.wasm_bytes(&memory).unwrap(), before);
        assert_eq!(region.metadata().instructions, length);
        if length == 2 {
            assert_eq!(region.native_x87_pair(0x1000).unwrap().steps().len(), 2);
        }
        if length == 4 {
            assert_eq!(region.native_x87_group(0).unwrap().steps().len(), 4);
        }
        if length == 32 {
            assert_eq!(region.native_x87_group32(4).unwrap().steps().len(), 32);
        }
    }
}

#[test]
fn native_success_rejects_unreviewed_helpers_and_context_writers() {
    for bytes in [
        &[0x0f, 0x57, 0xc0][..],          // xorps xmm0,xmm0
        &[0x0f, 0x77][..],                // emms
        &[0x0f, 0xae, 0x00][..],          // fxsave [eax]
        &[0x8e, 0xd8][..],                // mov ds,ax
        &[0xa4][..],                      // movsb
        &[0x0f, 0xa2][..],                // cpuid
        &[0xdb, 0xe3][..],                // fninit
        &[0xd9, 0x25, 0, 0x20, 0, 0][..], // fldenv
        &[0xdd, 0x35, 0, 0x20, 0, 0][..], // fnsave
    ] {
        let memory = memory(bytes);
        let region = region(&memory, bytes.len());
        let before = region.wasm_bytes(&memory).unwrap().to_vec();
        assert!(region.emit_native_success(7, 1).is_err(), "{bytes:02x?}");
        assert_eq!(region.wasm_bytes(&memory).unwrap(), before);
    }
}

#[test]
fn native_success_checks_every_retained_block_before_emission() {
    let memory = memory(&[0x90, 0x0f, 0x57, 0xc0]);
    let mut first = prepared(&memory, 1);
    first.blocks.append(&mut prepared(&memory, 4).blocks);
    let region = CompiledRegion::new(
        first,
        Vec::new(),
        super::super::CompilationTier32::Baseline,
        Some(wasm::EmbeddedBinding::Resident { key: 7, id: 1 }),
    )
    .unwrap();
    assert!(region.emit_native_success(7, 1).is_err());
}
