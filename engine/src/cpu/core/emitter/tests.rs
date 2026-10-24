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

#[test]
fn folded_scalar_flags_recovery_emits_valid_wasm_on_exit_and_memory_fault_paths() {
    use std::{fmt::Write, process::Command};

    let cases: [(&str, &[u8], usize); 3] = [
        ("self-subtract", &[0x2b, 0xc0, 0xff, 0xe6], 2),
        (
            "compare-then-replace",
            &[0x83, 0xf8, 1, 0xb8, 0, 0, 0, 0, 0xff, 0xe6],
            3,
        ),
        (
            "flags-only-input-before-faulting-read",
            &[0x83, 0xf8, 1, 0xb8, 0, 0, 0, 0, 0x8b, 0x13, 0xff, 0xe6],
            4,
        ),
    ];
    let mut modules = String::from("const modules=[");
    for (name, instructions, count) in cases {
        let mut memory = AddressSpace::new(1).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        memory.write(GuestAddress(0x1000), instructions).unwrap();
        // Exercise the real decoder, mixed program, allocator and emitter.
        // The read's unmapped data address is deliberately unknown at compile
        // time, so its generated precise-fault recovery must also be emitted.
        let code = compile(&memory, GuestAddress(0x1000), CompileOptions::default()).unwrap();
        assert_eq!(code.instruction_count(), count, "complete {name} program");
        write!(&mut modules, "{{name:{name:?},hex:\"").unwrap();
        for byte in code.wasm() {
            write!(&mut modules, "{byte:02x}").unwrap();
        }
        modules.push_str("\"},");
    }
    modules.push_str(
        "];for(const {name,hex} of modules){try{new WebAssembly.Module(Buffer.from(hex,'hex'));}\
         catch(error){throw new Error(name+': '+error.message);}}",
    );
    // This needs only the same Node validator used by the other generated-code
    // tests, never an environment-selected engine.wasm or a mocked owner.
    let result = Command::new("node")
        .args(["-e", &modules])
        .output()
        .expect("validate emitted scalar recovery modules");
    assert!(
        result.status.success(),
        "generated scalar recovery validation failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}
