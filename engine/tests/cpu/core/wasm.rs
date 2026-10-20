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
        ("ret", "c3"),
        ("indirect", "ffe6"),
        ("loop", "83c00331c283430c014975f40f0b"),
        ("unsupported", "0f0b"),
        ("add-jo", "83c0017007bb11111111eb05bb222222220f0b"),
        ("add-jb", "83c0017207bb11111111eb05bb222222220f0b"),
        ("sub-jl", "83e8017c07bb11111111eb05bb222222220f0b"),
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
    let mut manifest = String::new();
    for (name, hex) in programs {
        let memory = memory(&bytes(&hex));
        let code = compile(&memory, GuestAddress(0x1000), CompileOptions::default())
            .unwrap_or_else(|error| panic!("compile {name}: {error:?}"));
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
