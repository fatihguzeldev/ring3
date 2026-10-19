use super::reader::{Reader, section};
use super::validation::validate;

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
    let mut imports = section(wasm, 2);
    let import_count = imports.unsigned();
    let mut expected = expected.to_vec();
    let mut arities = arities.to_vec();
    if import_count == expected.len() + 2 {
        expected.push(("cpu_read32_view", 1));
        arities.push(1);
    }
    let read_view = expected
        .last()
        .is_some_and(|(name, arity)| *name == "cpu_read32_view" && *arity == 1);
    if read_view {
        assert_eq!(expected.first(), Some(&("guard_detect_resident", 7)));
        assert!(
            expected
                .iter()
                .any(|&(name, arity)| name == "cpu_read32" && arity == 1)
        );
    }
    assert_eq!(import_count, expected.len() + 1);
    let finish_type = arities.len() - 1 - usize::from(read_view);
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
        if (detection && index == finish_type) || (read_view && index == arities.len() - 1) {
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
    assert_eq!(
        (imports.name(), imports.name(), imports.byte()),
        ("env", "memory", 2)
    );
    assert_eq!((imports.unsigned(), imports.unsigned()), (0, 1));
    for &(name, arity) in &expected {
        assert_eq!(
            (imports.name(), imports.name(), imports.byte()),
            ("ring3", name, 0)
        );
        let index = imports.unsigned();
        assert_eq!(arities[index], arity, "import {name}");
        if name == "finish_exception_detection" {
            assert_eq!(index, finish_type);
        }
        if name == "cpu_read32_view" {
            assert_eq!(index, arities.len() - 1);
        }
        if matches!(name, "guard_detect" | "guard_detect_resident") {
            assert_eq!(index, 1);
        } else if !matches!(name, "finish_exception_detection" | "cpu_read32_view") {
            if bound {
                assert_ne!(index, 1, "ordinary helper {name} must return i32");
            }
            if detection {
                assert_ne!(
                    index, finish_type,
                    "ordinary helper {name} must take only i32 parameters"
                );
            }
            if read_view {
                assert_ne!(
                    index,
                    arities.len() - 1,
                    "ordinary helper {name} cannot use the view signature"
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
            arities.len() - 1 - usize::from(detection) - usize::from(read_view)
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
    if read_view {
        locals.extend([(7, 0x7f), (1, 0x7e)]);
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
