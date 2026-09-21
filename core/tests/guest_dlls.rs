#[path = "support/dll_executable.rs"]
mod dll_executable;
#[path = "support/imported_executable.rs"]
mod imported_executable;

use dll_executable::{attach, dll, exe, put};
use ring3_core::execution::{
    GuestModule, LoadError, Process32, ProcessOptions, ProcessStop, Register32, StopReason,
};

const BASE: u32 = 0x5000_0000;

fn load(bytes: &[u8], modules: &[GuestModule<'_>]) -> Result<Process32, LoadError> {
    Process32::load_with_options(
        bytes,
        96,
        ProcessOptions {
            modules,
            ..ProcessOptions::default()
        },
    )
}

fn word(process: &Process32, address: u32) -> u32 {
    let mut bytes = [0; 4];
    process.memory.read(u64::from(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

#[test]
fn dll_attach_named_ordinal_and_data_exports_share_guest_memory() {
    let library = dll(BASE, &attach(BASE, 41, true), None);
    let program = exe("DEMO.dll");
    let modules = [GuestModule {
        name: "demo.DLL",
        bytes: &library,
    }];
    let mut whole = load(&program, &modules).unwrap();
    let mut stepped = load(&program, &modules).unwrap();
    let result = whole.run(100);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!(result.api_calls, 0);
    assert_eq!(whole.cpu.register(Register32::Eax), 42);
    assert_eq!(whole.cpu.register(Register32::Ebx), 42);
    assert_eq!(whole.cpu.register(Register32::Esp), 0x1001_0000);
    assert_eq!(word(&whole, 0x0040_2060), BASE + 0x1080);
    assert_eq!(word(&whole, 0x0040_2064), BASE + 0x2180);
    assert_eq!(word(&whole, 0x0040_2068), BASE + 0x1080);
    assert_eq!(word(&whole, BASE + 0x2190), BASE);
    assert_eq!(word(&whole, BASE + 0x2194), 1);
    assert_ne!(word(&whole, BASE + 0x2198), 0);
    assert!(whole.memory.write(u64::from(BASE + 0x1000), &[0]).is_err());
    assert!(
        whole
            .memory
            .fetch(u64::from(BASE + 0x2180), &mut [0])
            .is_err()
    );
    let mut instructions = 0;
    loop {
        let step = stepped.run(1);
        instructions += step.instructions;
        if step.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
            assert_eq!(step.reason, result.reason);
            break;
        }
    }
    assert_eq!(instructions, result.instructions);
    assert_eq!(stepped.cpu, whole.cpu);
}

#[test]
fn failed_attach_is_terminal_and_zero_entry_skips_initialization() {
    let library = dll(BASE, &attach(BASE, 99, false), None);
    let mut process = load(
        &exe("demo.dll"),
        &[GuestModule {
            name: "demo.dll",
            bytes: &library,
        }],
    )
    .unwrap();
    let reason = ProcessStop::DllInitializationFailed {
        module: "demo.dll".to_owned(),
    };
    assert_eq!(process.run(100).reason, reason);
    assert_eq!(word(&process, BASE + 0x2180), 99);
    process.cpu.eip = 0x0040_1000;
    assert_eq!(process.run(100).reason, reason);
    assert_eq!(process.run(100).instructions, 0);
    let mut library = library;
    put(&mut library, 0xa8, 0);
    let mut process = load(
        &exe("demo.dll"),
        &[GuestModule {
            name: "demo.dll",
            bytes: &library,
        }],
    )
    .unwrap();
    assert_eq!(process.cpu.eip, 0x0040_1000);
    assert_eq!(
        process.run(20).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(process.cpu.register(Register32::Eax), 1);
}

#[test]
fn dependencies_initialize_first_even_when_supplied_in_reverse_order() {
    let dependency = dll(
        BASE + 0x0001_0000,
        &attach(BASE + 0x0001_0000, 40, true),
        None,
    );
    let mut code = vec![0xa1];
    code.extend_from_slice(&(BASE + 0x2060).to_le_bytes());
    code.extend_from_slice(&[0x8b, 0, 0x83, 0xc0, 1, 0xa3]);
    code.extend_from_slice(&(BASE + 0x2180).to_le_bytes());
    code.extend_from_slice(&[0xb8, 1, 0, 0, 0, 0xc2, 12, 0]);
    let dependent = dll(BASE, &code, Some("dependency.dll"));
    let mut process = load(
        &exe("dependent.dll"),
        &[
            GuestModule {
                name: "dependent.dll",
                bytes: &dependent,
            },
            GuestModule {
                name: "dependency.dll",
                bytes: &dependency,
            },
        ],
    )
    .unwrap();
    assert_eq!(
        process.run(100).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(process.cpu.register(Register32::Eax), 42);
}

#[test]
fn cyclic_dependencies_and_wrong_image_kinds_are_rejected() {
    let first = dll(BASE, &attach(BASE, 1, true), Some("second.dll"));
    let second = dll(
        BASE + 0x0001_0000,
        &attach(BASE + 0x0001_0000, 2, true),
        Some("first.dll"),
    );
    assert!(matches!(
        load(
            &exe("first.dll"),
            &[
                GuestModule {
                    name: "first.dll",
                    bytes: &first
                },
                GuestModule {
                    name: "second.dll",
                    bytes: &second
                },
            ]
        ),
        Err(LoadError::CyclicModules { .. })
    ));
    assert!(matches!(
        load(&first, &[]),
        Err(LoadError::UnsupportedImage)
    ));
    assert!(matches!(
        load(
            &exe("demo.dll"),
            &[GuestModule {
                name: "demo.dll",
                bytes: &exe("other.dll")
            }]
        ),
        Err(LoadError::UnsupportedImage)
    ));
}

#[test]
fn supplied_exports_are_authoritative_even_in_diagnostic_mode() {
    let program = exe("msvcrt.dll");
    for (offset, value) in [
        (1600, 0),
        (1600, 0x2270),
        (1600, 0x3000),
        (1608, 0x2270),
        (1612, 0x2260),
    ] {
        let mut library = dll(BASE, &attach(BASE, 1, true), None);
        put(&mut library, offset, value);
        assert!(matches!(
            Process32::load_with_options(
                &program,
                96,
                ProcessOptions {
                    modules: &[GuestModule {
                        name: "msvcrt.dll",
                        bytes: &library
                    }],
                    diagnostic_imports: true,
                    ..ProcessOptions::default()
                }
            ),
            Err(LoadError::InvalidExport { .. })
        ));
    }
    let mut library = dll(BASE, &attach(BASE, 1, true), None);
    put(&mut library, 1564, 0x3000);
    assert!(matches!(
        load(
            &program,
            &[GuestModule {
                name: "msvcrt.dll",
                bytes: &library
            }]
        ),
        Err(LoadError::Exports { .. })
    ));
    let library = dll(BASE, &attach(BASE, 1, true), None);
    let program = imported_executable::pe32(&[0xcc], "msvcrt.dll", &["_initterm"]);
    assert!(matches!(
        load(
            &program,
            &[GuestModule {
                name: "msvcrt.dll",
                bytes: &library
            }]
        ),
        Err(LoadError::InvalidExport { .. })
    ));
}

#[test]
fn names_and_module_input_caps_are_checked_before_parsing() {
    for name in [
        "",
        ".",
        "..",
        "../a.dll",
        "a/b.dll",
        "a\\b.dll",
        "a\0dll",
        "é.dll",
        "a dll",
        &"a".repeat(256),
    ] {
        assert!(matches!(
            load(&[], &[GuestModule { name, bytes: &[] }]),
            Err(LoadError::InvalidModuleName)
        ));
    }
    assert!(matches!(
        load(
            &[],
            &[
                GuestModule {
                    name: "demo.dll",
                    bytes: &[]
                },
                GuestModule {
                    name: "DEMO.DLL",
                    bytes: &[]
                },
            ]
        ),
        Err(LoadError::DuplicateModule { .. })
    ));
    assert!(matches!(
        load(
            &[],
            &[GuestModule {
                name: "demo.dll",
                bytes: &[]
            }; 17]
        ),
        Err(LoadError::ModuleLimitExceeded)
    ));
    let bytes = vec![0; 8 * 1024 * 1024 + 1];
    let names: Vec<_> = (0..16).map(|index| format!("m{index}.dll")).collect();
    let modules: Vec<_> = names
        .iter()
        .map(|name| GuestModule {
            name,
            bytes: &bytes,
        })
        .collect();
    assert!(matches!(
        load(&[], &modules),
        Err(LoadError::ModuleLimitExceeded)
    ));
}

#[test]
fn module_mapping_limits_protections_and_unsupported_directories_are_enforced() {
    use ring3_core::execution::MemoryError;
    let program = exe("demo.dll");
    for base in [0x0040_0000, 0x1000_0000, 0x7000_0000, 0x7001_0000] {
        let mut library = dll(base, &attach(base, 1, true), None);
        if base == 0x7001_0000 {
            put(&mut library, 0x1a8, 0x4000);
            put(&mut library, 0xd0, 0x6000);
        }
        assert!(matches!(
            load(
                &program,
                &[GuestModule {
                    name: "demo.dll",
                    bytes: &library
                }]
            ),
            Err(LoadError::RelocationRequired)
        ));
    }
    let library = dll(BASE, &attach(BASE, 1, true), None);
    assert!(matches!(
        Process32::load_with_options(
            &program,
            28,
            ProcessOptions {
                modules: &[GuestModule {
                    name: "demo.dll",
                    bytes: &library
                }],
                ..ProcessOptions::default()
            }
        ),
        Err(LoadError::Memory(MemoryError::PageLimitExceeded))
    ));
    for index in [9, 10] {
        let mut unsupported = library.clone();
        put(&mut unsupported, 0xf8 + index * 8, 0x2200);
        put(&mut unsupported, 0xfc + index * 8, 32);
        assert!(
            matches!(load(&program, &[GuestModule { name: "demo.dll", bytes: &unsupported }]), Err(LoadError::UnsupportedDirectory { index: actual }) if usize::from(actual) == index)
        );
    }
    let mut bad_entry = library;
    put(&mut bad_entry, 0xa8, 0x2180);
    assert!(matches!(
        load(
            &program,
            &[GuestModule {
                name: "demo.dll",
                bytes: &bad_entry
            }]
        ),
        Err(LoadError::Memory(MemoryError::PermissionDenied { .. }))
    ));
}

#[test]
fn unresolved_imports_during_attach_stop_before_exe_without_skipping_dll_work() {
    let mut code = vec![0xff, 0x15];
    code.extend_from_slice(&(BASE + 0x2060).to_le_bytes());
    code.extend_from_slice(&[0xb8, 1, 0, 0, 0, 0xc2, 12, 0]);
    let library = dll(BASE, &code, Some("missing.dll"));
    let mut process = Process32::load_with_options(
        &exe("demo.dll"),
        96,
        ProcessOptions {
            modules: &[GuestModule {
                name: "demo.dll",
                bytes: &library,
            }],
            diagnostic_imports: true,
            ..ProcessOptions::default()
        },
    )
    .unwrap();
    assert!(
        matches!(process.run(100).reason, ProcessStop::UnresolvedImport { module, symbol, .. } if module == "missing.dll" && symbol == "state")
    );
    assert_eq!(word(&process, BASE + 0x2180), 0);
}

#[test]
fn ordinal_beyond_the_old_table_cap_resolves_and_executes() {
    let base = 0x5000_0000;
    let mut library = dll(base, &attach(base, 41, true), None);
    library.resize(0x6400, 0);
    for (offset, value) in [
        (0x1a8, 0x6000),
        (0x1b0, 0x6000),
        (0xd0, 0x8000),
        (1556, 5000),
        (1564, 0x2300),
        (0x700, 0x1080),
        (0x704, 0x2180),
        (0x700 + 4999 * 4, 0x1080),
    ] {
        put(&mut library, offset, value);
    }
    let mut program = imported_executable::pe32(
        &[0xff, 0x15, 0x60, 0x20, 0x40, 0, 0xcc],
        "demo.dll",
        &["ordinal"],
    );
    put(&mut program, 1088, 0x8000_138e);
    let mut process = Process32::load_with_options(
        &program,
        64,
        ProcessOptions {
            modules: &[GuestModule {
                name: "demo.dll",
                bytes: &library,
            }],
            ..ProcessOptions::default()
        },
    )
    .unwrap();
    assert_eq!(
        process.run(100).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(process.cpu.register(Register32::Eax), 42);
}
