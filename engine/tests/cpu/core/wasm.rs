use ring3_engine::{
    cpu::core::{CompileOptions, compile},
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

struct Scratch(PathBuf, bool);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ring3-core-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&path).unwrap();
        Self(path, true)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if self.1 {
            eprintln!("core fixture retained: {}", self.0.display());
        } else {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

fn memory(bytes: &[u8]) -> AddressSpace {
    let mut memory = AddressSpace::new(3).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x3000), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x4000), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    memory
}

fn bytes(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

#[test]
fn compiled_source_currentness_tracks_code_and_mapping_identity() {
    let mut memory = memory(&bytes("83c0010f0b"));
    let code = compile(&memory, GuestAddress(0x1000), CompileOptions::default()).unwrap();
    assert!(code.is_current(&memory));
    memory.write(GuestAddress(0x3000), &[1, 2, 3, 4]).unwrap();
    assert!(code.is_current(&memory));
    memory.write(GuestAddress(0x1002), &[2]).unwrap();
    assert!(!code.is_current(&memory));

    let other = self::memory(&bytes("83c0010f0b"));
    assert!(!code.is_current(&other));
}

#[test]
fn generated_zero_flag_branches_preserve_state_without_materializing_the_condition() {
    let mut scratch = Scratch::new();
    let mut manifest = String::new();
    for (name, prefix, condition) in [
        ("add8", "0401", 0x74),
        ("sub16", "6683e801", 0x75),
        ("logical32", "31c0", 0x74),
        ("inc32", "40", 0x75),
        ("dec32", "48", 0x75),
        ("adc8", "1400", 0x74),
        ("sbb16", "6683d800", 0x75),
        ("neg32", "f7d8", 0x74),
        ("entry-equal", "", 0x74),
        ("entry-not-equal", "", 0x75),
        ("canonical-zf", "04019e", 0x74),
        ("postbranch-carry", "04017400f89f", 0x74),
        ("other-condition", "0401", 0x77),
        ("preserved-recipe", "040189d6", 0x74),
        ("rmw", "830301", 0x74),
        ("rmw-fault", "31c0830301", 0x74),
    ] {
        let hex = format!("{prefix}{condition:02x}07bb11111111eb05bb222222220f0b");
        let memory = memory(&bytes(&hex));
        let code = compile(&memory, GuestAddress(0x1000), CompileOptions::default()).unwrap();
        assert!(code.is_current(&memory));
        assert!(code.native_instructions().is_empty());
        fs::write(scratch.0.join(format!("{name}.wasm")), code.wasm()).unwrap();
        manifest.push_str(&format!("{name}\t{hex}\t{}\n", code.identity()));
    }
    fs::write(scratch.0.join("conditions.tsv"), manifest).unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let result = Command::new("node")
        .arg(root.join("engine/tests/cpu/core/fixtures/conditions.mjs"))
        .arg(&scratch.0)
        .output()
        .expect("run generated condition modules with node");
    assert!(
        result.status.success(),
        "generated condition fixture failed (output: {}):\n{}\n{}",
        scratch.0.display(),
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
    scratch.1 = false;
}

#[test]
fn generated_wasm_runs_authored_integer_control_memory_programs() {
    let mut scratch = Scratch::new();
    let mut programs: Vec<(String, String)> = [
        ("add", "83c0010f0b"),
        ("sub", "83e8010f0b"),
        ("xor", "31d00f0b"),
        ("inc", "400f0b"),
        ("dec", "480f0b"),
        ("adc", "83d0010f0b"),
        ("sbb", "83d8010f0b"),
        ("neg", "f7d80f0b"),
        ("add8", "04010f0b"),
        ("add16", "6683c0010f0b"),
        ("load-store", "8b038943040f0b"),
        ("fs-load", "648b030f0b"),
        ("load-store8", "8a038843010f0b"),
        ("load-store16", "668b03668943020f0b"),
        ("store", "89030f0b"),
        ("rmw", "8303010f0b"),
        ("push-pop", "50590f0b"),
        ("call", "e80b00000083c0020f0b90909090909083c003c3"),
        ("call-callee", "e80b00000083c0020f0b90909090909083c003c3"),
        ("ret", "c3"),
        ("indirect", "ffe6"),
        ("loop", "83c00331c283430c014975f40f0b"),
        ("unsupported", "0f0b"),
        ("add-jo", "83c0017007bb11111111eb05bb222222220f0b"),
        ("add-jb", "83c0017207bb11111111eb05bb222222220f0b"),
        ("sub-jl", "83e8017c07bb11111111eb05bb222222220f0b"),
        ("memory-shift", "d3230f0b"),
        ("extend", "0fbec10fbfd20f0b"),
        ("byte-swap", "0fc80f0b"),
        ("flags-ah", "9f9e0f0b"),
        ("translate-byte", "d70f0b"),
        ("memory-exchange", "87030f0b"),
        ("word-call", "66e80c006683c0020f0b9090909090906683c00366c3"),
        (
            "word-call-callee",
            "66e80c006683c0020f0b9090909090906683c00366c3",
        ),
        ("word-pop-sp", "665c0f0b"),
        ("word-pop-flags", "669d0f0b"),
        ("rep-move", "f3a40f0b"),
        ("rep-prefix", "40f3a40f0b"),
        ("rep-compare", "f3a60f0b"),
        ("rep-scan", "f2ae0f0b"),
        ("rep-move16-address", "67f3a40f0b"),
        ("move-word-string", "66a50f0b"),
        ("bsf32", "0fbcc10f0b"),
        ("bsr32", "0fbdc10f0b"),
        ("bsf16", "660fbcc10f0b"),
        ("bsr16", "660fbdc10f0b"),
        ("bsfmem", "0fbc030f0b"),
        ("bt32", "0fa3c80f0b"),
        ("bts32", "0fabc80f0b"),
        ("btr16", "660fb3c80f0b"),
        ("btmem", "0fa30b0f0b"),
        ("btsmem", "0fab0b0f0b"),
        ("bt16mem", "660fa30b0f0b"),
        ("lockbtsmem", "f00fab0b0f0b"),
        ("tzcnt", "f30fbcc10f0b"),
    ]
    .into_iter()
    .map(|(name, code)| (name.to_owned(), code.to_owned()))
    .collect();
    for (kind, encodings) in [
        ("add", ["0401", "6683c001", "83c001"]),
        ("sub", ["2c01", "6683e801", "83e801"]),
        ("adc", ["1401", "6683d001", "83d001"]),
        ("sbb", ["1c01", "6683d801", "83d801"]),
        ("xor", ["30d0", "6631d0", "31d0"]),
        ("inc", ["fec0", "6640", "40"]),
        ("dec", ["fec8", "6648", "48"]),
        ("neg", ["f6d8", "66f7d8", "f7d8"]),
    ] {
        for (width, encoding) in [8, 16, 32].into_iter().zip(encodings) {
            programs.push((format!("flags{width}-{kind}"), format!("{encoding}0f0b")));
        }
    }
    for condition in 0..16 {
        programs.push((
            format!("jcc{condition}"),
            format!("{:02x}07bb11111111eb05bb222222220f0b", 0x70 + condition),
        ));
    }
    for (kind, opcode) in [
        ("shl", "e0"),
        ("shr", "e8"),
        ("sar", "f8"),
        ("rol", "c0"),
        ("ror", "c8"),
        ("rcl", "d0"),
        ("rcr", "d8"),
    ] {
        for (width, prefix) in [(8, "d2"), (16, "66d3"), (32, "d3")] {
            programs.push((
                format!("shift{width}-{kind}"),
                format!("{prefix}{opcode}0f0b"),
            ));
        }
    }
    for (kind, opcode) in [("shld", "0fa5d0"), ("shrd", "0fadd0")] {
        for (width, prefix) in [(16, "66"), (32, "")] {
            programs.push((
                format!("shift{width}-{kind}"),
                format!("{prefix}{opcode}0f0b"),
            ));
        }
    }
    for (width, prefix, opcode) in [(8, "", "f6"), (16, "66", "f7"), (32, "", "f7")] {
        for (kind, suffix) in [("mul", "e1"), ("imul", "e9"), ("div", "f1"), ("idiv", "f9")] {
            programs.push((
                format!("multiply{width}-{kind}"),
                format!("{prefix}{opcode}{suffix}0f0b"),
            ));
        }
    }
    for (width, prefix) in [(16, "66"), (32, "")] {
        programs.push((
            format!("multiply{width}-two"),
            format!("{prefix}0fafc10f0b"),
        ));
        programs.push((
            format!("multiply{width}-immediate"),
            format!("{prefix}6bc1fd0f0b"),
        ));
    }
    programs.push(("multiply32-memory".into(), "f7230f0b".into()));
    programs.push(("divide32-memory".into(), "f7330f0b".into()));
    for (kind, byte, wide) in [
        ("add", "0003", "0103"),
        ("adc", "1003", "1103"),
        ("sub", "2803", "2903"),
        ("sbb", "1803", "1903"),
        ("and", "2003", "2103"),
        ("or", "0803", "0903"),
        ("xor", "3003", "3103"),
        ("inc", "fe03", "ff03"),
        ("dec", "fe0b", "ff0b"),
        ("neg", "f61b", "f71b"),
        ("not", "f613", "f713"),
    ] {
        for (width, prefix, opcode) in [(8, "", byte), (16, "66", wide), (32, "", wide)] {
            programs.push((
                format!("atomic{width}-{kind}"),
                format!("f0{prefix}{opcode}0f0b"),
            ));
        }
    }
    programs.push((
        "bounded-prefix".into(),
        format!("{}0f0b", "830301d32366830301".repeat(24)),
    ));
    let mut manifest = String::new();
    for (name, hex) in programs {
        let memory = memory(&bytes(&hex));
        let entry = if name.ends_with("-callee") {
            0x1010
        } else {
            0x1000
        };
        let options = if name == "bounded-prefix" {
            let first = compile(
                &memory,
                GuestAddress(entry),
                CompileOptions {
                    instructions: 1,
                    ..CompileOptions::default()
                },
            )
            .unwrap();
            let second = compile(
                &memory,
                GuestAddress(entry),
                CompileOptions {
                    instructions: 2,
                    ..CompileOptions::default()
                },
            )
            .unwrap();
            let quota = first.wasm().len() + 128;
            assert!(
                second.wasm().len() > quota,
                "two operations exceed authored prefix quota"
            );
            CompileOptions {
                wasm_bytes: quota,
                ..CompileOptions::default()
            }
        } else {
            CompileOptions::default()
        };
        let code = compile(&memory, GuestAddress(entry), options)
            .unwrap_or_else(|error| panic!("compile {name}: {error:?}"));
        if name == "bounded-prefix" {
            assert_eq!(code.instruction_count(), 1);
        }
        assert!(code.is_current(&memory), "fresh source: {name}");
        fs::write(scratch.0.join(format!("{name}.wasm")), code.wasm()).unwrap();
        manifest.push_str(&format!("{name}\t{hex}\t{}\n", code.identity()));
    }
    fs::write(scratch.0.join("programs.tsv"), manifest).unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let result = Command::new("node")
        .arg(root.join("engine/tests/cpu/core/fixtures/run.mjs"))
        .arg(&scratch.0)
        .output()
        .expect("run generated modules with node");
    assert!(
        result.status.success(),
        "generated core fixture failed (output: {}):\n{}\n{}",
        scratch.0.display(),
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
    scratch.1 = false;
}
