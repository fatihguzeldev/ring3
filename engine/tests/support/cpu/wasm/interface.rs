use super::reader::{Reader, section};
use std::{
    io::Write,
    process::{Command, Stdio},
};

pub fn validate(wasm: &[u8]) {
    let mut child = Command::new("node")
        .args(["-e", "const b=require('node:fs').readFileSync(0);if(!WebAssembly.validate(b))process.exit(1);new WebAssembly.Module(b);"])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .spawn().unwrap();
    child.stdin.take().unwrap().write_all(wasm).unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

pub fn assert_packed(input: &[u8], count: usize, profiled: bool) {
    let mut input = Reader(input);
    for index in 0..count {
        let owner = if profiled { input.byte() } else { 1 };
        assert!(owner <= 2);
        if profiled && count == 6 {
            assert_eq!(owner, [0, 0, 1, 1, 2, 2][index]);
        }
        let length = u32::from_le_bytes(input.take(4).try_into().unwrap()) as usize;
        assert!(length > 8 && length <= 65_536);
        let bytes = input.take(length);
        if owner == 0 {
            assert_interface(bytes, &[], &[4], false);
        } else {
            let resident = owner == 2;
            assert_bound(
                bytes,
                resident,
                &[],
                &[4, if resident { 7 } else { 6 }],
                false,
            );
        }
    }
    input.end();
}

// callers supply family-specific helpers and every declared pre-segment type, including
// unused shared helper signatures. detection is a common bound-module ABI contract.
pub fn assert_bound(
    wasm: &[u8],
    resident: bool,
    helpers: &[(&str, usize)],
    arities: &[usize],
    segmented: bool,
) {
    let mut expected = vec![(
        if resident {
            "guard_detect_resident"
        } else {
            "guard_detect"
        },
        if resident { 7 } else { 6 },
    )];
    expected.extend_from_slice(helpers);
    expected.push(("finish_exception_detection", 12));
    let mut types = arities.to_vec();
    if segmented {
        types.push(6);
    }
    types.push(12);
    assert_interface(wasm, &expected, &types, segmented);
}

pub fn assert_interface(
    wasm: &[u8],
    expected: &[(&str, usize)],
    arities: &[usize],
    segmented: bool,
) {
    validate(wasm);
    let bound = expected
        .first()
        .is_some_and(|(name, _)| matches!(*name, "guard_detect" | "guard_detect_resident"));
    let detection = expected
        .iter()
        .any(|(name, _)| *name == "finish_exception_detection");
    assert_eq!(detection, bound);
    let mut globals = section(wasm, 6);
    assert_eq!(globals.unsigned(), 3);
    for expected in [u32::from(bound), 0, 1] {
        assert_eq!(
            (globals.byte(), globals.byte(), globals.byte()),
            (0x7f, 0, 0x41)
        );
        assert_eq!(globals.signed_word(), expected);
        assert_eq!(globals.byte(), 0x0b);
    }
    globals.end();
    let mut types = section(wasm, 1);
    assert_eq!(types.unsigned(), arities.len());
    for (index, &arity) in arities.iter().enumerate() {
        assert_eq!(types.byte(), 0x60);
        assert_eq!(types.unsigned(), arity);
        let mut parameters = vec![0x7f; arity];
        if detection && index == arities.len() - 1 {
            parameters[0] = 0x7e;
        }
        assert_eq!(types.take(arity), parameters, "type {index}");
        assert_eq!(types.unsigned(), 1);
        assert_eq!(
            types.byte(),
            if bound && index == 1 { 0x7e } else { 0x7f },
            "type {index}"
        );
    }
    types.end();
    let mut imports = section(wasm, 2);
    assert_eq!(imports.unsigned(), expected.len() + 1);
    assert_eq!(
        (imports.name(), imports.name(), imports.byte()),
        ("env", "memory", 2)
    );
    assert_eq!((imports.unsigned(), imports.unsigned()), (0, 1));
    for &(name, arity) in expected {
        assert_eq!(
            (imports.name(), imports.name(), imports.byte()),
            ("ring3", name, 0)
        );
        let index = imports.unsigned();
        assert_eq!(arities[index], arity, "import {name}");
        if name == "finish_exception_detection" {
            assert_eq!(index, arities.len() - 1);
        }
        if matches!(name, "guard_detect" | "guard_detect_resident") {
            assert_eq!(index, 1);
        } else if name != "finish_exception_detection" {
            if bound {
                assert_ne!(index, 1, "ordinary helper {name} must return i32");
            }
            if detection {
                assert_ne!(
                    index,
                    arities.len() - 1,
                    "ordinary helper {name} must take only i32 parameters"
                );
            }
        }
    }
    imports.end();
    let mut functions = section(wasm, 3);
    assert_eq!(functions.unsigned(), if segmented { 2 } else { 1 });
    assert_eq!(functions.unsigned(), 0);
    if segmented {
        assert_eq!(
            functions.unsigned(),
            arities.len() - 1 - usize::from(detection)
        );
    }
    functions.end();
    let mut exports = section(wasm, 7);
    assert_eq!(
        (
            exports.unsigned(),
            exports.name(),
            exports.byte(),
            exports.unsigned()
        ),
        (1, "run", 0, expected.len())
    );
    exports.end();
    let mut bodies = section(wasm, 10);
    assert_eq!(bodies.unsigned(), if segmented { 2 } else { 1 });
    let length = bodies.unsigned();
    let mut body = Reader(bodies.take(length));
    let mut locals = vec![(16, 0x7f), (1, 0x7e), (10, 0x7f)];
    if bound {
        locals.push((1, 0x7f));
    }
    if detection {
        locals.extend([(1, 0x7e), (6, 0x7f)]);
    }
    assert_eq!(body.unsigned(), locals.len());
    for expected in locals {
        assert_eq!((body.unsigned(), body.byte()), expected);
    }
    assert!(!body.0.is_empty());
    assert_eq!(body.0.last(), Some(&0x0b));
    if segmented {
        let length = bodies.unsigned();
        let mut body = Reader(bodies.take(length));
        assert_eq!(
            (body.unsigned(), body.unsigned(), body.byte()),
            (1, 3, 0x7f)
        );
        assert!(!body.0.is_empty());
        assert_eq!(body.0.last(), Some(&0x0b));
    }
    bodies.end();
}

pub fn assert_binding_prefix(wasm: &[u8], key: u64, id: u64, resident: bool) {
    let mut bodies = section(wasm, 10);
    assert!((1..=2).contains(&bodies.unsigned()));
    let length = bodies.unsigned();
    let mut body = Reader(bodies.take(length));
    for _ in 0..body.unsigned() {
        body.unsigned();
        body.byte();
    }
    let mut constants = vec![key as u32, (key >> 32) as u32, id as u32];
    if resident {
        constants.push((id >> 32) as u32);
    }
    for expected in constants {
        assert_eq!(body.byte(), 0x41);
        assert_eq!(body.signed_word(), expected);
    }
    for local in [0, 1, 3] {
        assert_eq!((body.byte(), body.unsigned()), (0x20, local));
    }
    assert_eq!((body.byte(), body.unsigned()), (0x10, 0));
}
