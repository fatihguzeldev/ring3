use crate::{
    cpu::core::{CompileOptions, compile},
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

#[test]
fn memory_heavy_module_keeps_the_complete_program_with_bounded_code_bytes() {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    let mut instructions = [0x83, 0x03, 1].repeat(24);
    instructions.extend_from_slice(&[0x83, 0xc0, 4, 0xff, 0xe2]);
    memory.write(GuestAddress(0x1000), &instructions).unwrap();
    let code = compile(
        &memory,
        GuestAddress(0x1000),
        CompileOptions {
            instructions: 64,
            blocks: 8,
            wasm_bytes: 1 << 20,
        },
    )
    .unwrap();
    assert_eq!(code.instruction_count(), 26);
    assert_eq!(code.entrypoints(), &[GuestAddress(0x1000)]);
    assert!(code.native_instructions().is_empty());
    assert!(code.is_current(&memory));
    println!(
        "complete 26-instruction module: {} bytes",
        code.wasm().len()
    );
    assert!(
        code.wasm().len() <= 36 * 1024,
        "the complete memory-heavy program needs compact side exits; got {} bytes",
        code.wasm().len()
    );
}
