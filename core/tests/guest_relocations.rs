#[path = "support/dll_executable.rs"]
mod dll_executable;
#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/relocated_executable.rs"]
mod relocated_executable;

use dll_executable::put;
use ring3_core::execution::{
    GuestModule, LoadError, Process32, ProcessOptions, ProcessStop, Register32, StopReason,
};

fn load(program: &[u8], modules: &[GuestModule<'_>]) -> Result<Process32, LoadError> {
    Process32::load_with_options(
        program,
        128,
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
fn relocated_dll_executes_attach_and_binds_named_ordinal_and_data_exports() {
    let bytes = relocated_executable::dll(0x1000_0000);
    let original = bytes.clone();
    let program = dll_executable::exe("demo.dll");
    let modules = [GuestModule {
        name: "demo.dll",
        bytes: &bytes,
    }];
    let mut whole = load(&program, &modules).unwrap();
    let mut stepped = load(&program, &modules).unwrap();
    let result = whole.run(100);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!(whole.cpu.register(Register32::Eax), 42);
    assert_eq!(whole.cpu.register(Register32::Ebx), 42);
    assert_eq!(whole.cpu.register(Register32::Esp), 0x1001_0000);
    for (address, expected) in [
        (0x0040_2060, 0x3000_1080),
        (0x0040_2064, 0x3000_2180),
        (0x0040_2068, 0x3000_1080),
        (0x3000_2190, 0x3000_0000),
        (0x3000_2194, 1),
        (0x3000_00b4, 0x1000_0000),
    ] {
        assert_eq!(word(&whole, address), expected);
    }
    assert_ne!(word(&whole, 0x3000_2198), 0);
    assert!(whole.memory.write(0x3000_1000, &[0]).is_err());
    assert!(whole.memory.fetch(0x3000_2180, &mut [0]).is_err());
    let mut count = 0;
    loop {
        let step = stepped.run(1);
        count += step.instructions;
        if step.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
            assert_eq!(step.reason, result.reason);
            break;
        }
    }
    assert_eq!(count, result.instructions);
    assert_eq!(stepped.cpu, whole.cpu);
    assert_eq!(bytes, original);
}

#[test]
fn preferred_bases_are_reserved_before_displaced_modules_are_placed() {
    let first = relocated_executable::dll(0x1000_0000);
    let second = relocated_executable::dll(0x3000_0000);
    let third = relocated_executable::dll(0x3000_0000);
    let mut process = load(
        &dll_executable::exe("first.dll"),
        &[
            GuestModule {
                name: "first.dll",
                bytes: &first,
            },
            GuestModule {
                name: "second.dll",
                bytes: &second,
            },
            GuestModule {
                name: "third.dll",
                bytes: &third,
            },
        ],
    )
    .unwrap();
    assert_eq!(
        process.run(200).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    for base in [0x3000_0000, 0x3001_0000, 0x3002_0000] {
        assert_eq!(word(&process, base + 0x2190), base);
        assert_eq!(word(&process, base + 0x2180), 41);
    }
    assert_eq!(word(&process, 0x0040_2060), 0x3001_1080);
}

#[test]
fn exe_stack_future_heap_api_parameters_diagnostics_and_thread_are_reserved() {
    for (base, size) in [
        (0x0040_0000, 0x3000),
        (0x1000_0000, 0x3000),
        (0x2000_0000, 0x3000),
        (0x2fff_0000, 0x3000),
        (0x7000_0000, 0x3000),
        (0x7001_0000, 0x6000),
        (0x7100_0000, 0x3000),
        (0x7ffd_0000, 0xf000),
    ] {
        let mut library = relocated_executable::dll(base);
        put(&mut library, 0xd0, size);
        let mut process = load(
            &dll_executable::exe("demo.dll"),
            &[GuestModule {
                name: "demo.dll",
                bytes: &library,
            }],
        )
        .unwrap();
        assert_eq!(
            process.run(100).reason,
            ProcessStop::Stopped(StopReason::Breakpoint)
        );
        assert_eq!(word(&process, 0x3000_2190), 0x3000_0000);
        assert_eq!(process.cpu.register(Register32::Eax), 42);
        assert!(process.memory.mapped_pages() < 50);
    }
}

#[test]
fn missing_or_stripped_relocations_fail_only_when_movement_is_required() {
    for stripped in [false, true] {
        let mut library = relocated_executable::dll(0x1000_0000);
        if stripped {
            library[0x96] |= 1;
        } else {
            put(&mut library, 0x120, 0);
            put(&mut library, 0x124, 0);
        }
        let program = dll_executable::exe("demo.dll");
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
        put(&mut library, 0xb4, 0x5000_0000);
        assert!(
            load(
                &program,
                &[GuestModule {
                    name: "demo.dll",
                    bytes: &library
                }]
            )
            .is_ok()
        );
    }
}

fn one_block(library: &mut [u8], page: u32, entries: &[u16]) {
    let size = 8 + u32::try_from(entries.len()).unwrap() * 2;
    put(library, 0x120, 0x2300);
    put(library, 0x124, size);
    put(library, 0x700, page);
    put(library, 0x704, size);
    for (index, entry) in entries.iter().enumerate() {
        library[0x708 + index * 2..0x70a + index * 2].copy_from_slice(&entry.to_le_bytes());
    }
}

#[test]
fn highlow_wraps_in_both_directions_and_targets_headers_zero_fill_and_cross_page_words() {
    for (preferred, delta) in [(0x1000_0000, 0x2000_0000_u32), (0x7000_0000, 0xc000_0000)] {
        for (page, offset, original) in [
            (0, 0x40, 0xffff_fffe_u32),
            (0x1000, 0xffe, 0),
            (0x2000, 0xf00, 0),
        ] {
            let mut library = relocated_executable::dll(preferred);
            if page == 0 {
                put(&mut library, 0x40, original);
            }
            if page == 0x1000 {
                library[0x400..0x402].fill(0);
            }
            one_block(&mut library, page, &[0x3000 | offset, 0]);
            let process = load(
                &dll_executable::exe("demo.dll"),
                &[GuestModule {
                    name: "demo.dll",
                    bytes: &library,
                }],
            )
            .unwrap();
            assert_eq!(
                word(&process, 0x3000_0000 + page + u32::from(offset)),
                original.wrapping_add(delta)
            );
        }
    }
    let mut library = relocated_executable::dll(0x1000_0000);
    one_block(&mut library, 0x2000, &[0x3f00, 0x3f00]);
    let process = load(
        &dll_executable::exe("demo.dll"),
        &[GuestModule {
            name: "demo.dll",
            bytes: &library,
        }],
    )
    .unwrap();
    assert_eq!(word(&process, 0x3000_2f00), 0x4000_0000);
}

#[test]
fn unsupported_malformed_and_out_of_image_relocations_fail_before_import_resolution() {
    for (page, entry, size, expected) in [
        (
            0x1001,
            0_u16,
            0x3000,
            LoadError::UnalignedRelocationPage { rva: 0x1001 },
        ),
        (
            0x1000,
            0xa005,
            0x3000,
            LoadError::UnsupportedRelocation { kind: 10 },
        ),
        (
            0x2000,
            0x3ffe,
            0x3000,
            LoadError::RelocationTarget { rva: 0x2ffe },
        ),
        (
            0xffff_f000,
            0x3fff,
            0x3000,
            LoadError::RelocationTarget { rva: 0xffff_ffff },
        ),
        (
            0x2000,
            0x3ffc,
            0x2fff,
            LoadError::RelocationTarget { rva: 0x2ffc },
        ),
    ] {
        let mut library = relocated_executable::dll(0x1000_0000);
        put(&mut library, 0xd0, size);
        one_block(&mut library, page, &[entry, 0]);
        let error = load(
            &dll_executable::exe("missing.dll"),
            &[GuestModule {
                name: "demo.dll",
                bytes: &library,
            }],
        )
        .err()
        .unwrap();
        assert_eq!(error, expected);
    }
    let mut library = relocated_executable::dll(0x1000_0000);
    put(&mut library, 0x704, 7);
    assert!(matches!(
        load(
            &dll_executable::exe("demo.dll"),
            &[GuestModule {
                name: "demo.dll",
                bytes: &library
            }]
        ),
        Err(LoadError::Relocations(_))
    ));
    one_block(&mut library, 0xffff_f000, &[0x0fff, 0]);
    assert!(
        load(
            &dll_executable::exe("demo.dll"),
            &[GuestModule {
                name: "demo.dll",
                bytes: &library
            }]
        )
        .is_ok()
    );
}

#[test]
fn exhausted_relocation_pool_fails_before_mapping_large_images() {
    let mut resident = relocated_executable::dll(0x3000_0000);
    put(&mut resident, 0xd0, 0x4000_0000);
    let displaced = relocated_executable::dll(0x1000_0000);
    assert!(matches!(
        load(
            &dll_executable::exe("demo.dll"),
            &[
                GuestModule {
                    name: "resident.dll",
                    bytes: &resident
                },
                GuestModule {
                    name: "demo.dll",
                    bytes: &displaced
                },
            ]
        ),
        Err(LoadError::NoModuleAddress)
    ));
}

#[test]
fn rebased_dependencies_initialize_before_consumers_and_bind_their_iat() {
    let base = 0x1000_0000_u32;
    let dependency = relocated_executable::dll(base);
    let mut code = vec![0xa1];
    code.extend_from_slice(&(base + 0x2060).to_le_bytes());
    code.extend_from_slice(&[0x8b, 0, 0x83, 0xc0, 1, 0xa3]);
    code.extend_from_slice(&(base + 0x2180).to_le_bytes());
    code.extend_from_slice(&[0xb8, 1, 0, 0, 0, 0xc2, 12, 0]);
    let mut dependent = dll_executable::dll(base, &code, Some("dependency.dll"));
    one_block(&mut dependent, 0x1000, &[0x3001, 0x300b, 0x3081, 0]);
    let mut process = load(
        &dll_executable::exe("dependent.dll"),
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
        process.run(200).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(word(&process, 0x3000_2060), 0x3001_2180);
    assert_eq!(word(&process, 0x3000_2180), 42);
    assert_eq!(process.cpu.register(Register32::Eax), 43);
}
