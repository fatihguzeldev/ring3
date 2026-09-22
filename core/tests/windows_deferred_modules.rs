#[path = "support/dll_executable.rs"]
#[allow(dead_code)]
mod dll_executable;
#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{GuestModule, LoadError, Process32, ProcessOptions};

const DLL: u32 = 0x5000_0000;

fn program() -> Vec<u8> {
    imported_executable::pe32(&[0xcc], "kernel32.dll", &["LoadLibraryA"])
}

fn word(process: &Process32, address: u32) -> u32 {
    let mut bytes = [0; 4];
    process.memory.read(u64::from(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

#[test]
fn deferred_module_is_mapped_without_running_startup_attach() {
    let library = dll_executable::dll(DLL, &dll_executable::attach(DLL, 42, true), None);
    let modules = [GuestModule {
        name: "demo.dll",
        bytes: &library,
    }];
    let process = Process32::load_with_options(
        &program(),
        64,
        ProcessOptions {
            deferred_modules: &modules,
            ..ProcessOptions::default()
        },
    )
    .unwrap();
    assert_eq!(process.cpu.eip, 0x0040_1000);
    assert_eq!(word(&process, DLL + 0x2180), 0);
}

#[test]
fn static_and_deferred_inputs_share_name_and_dependency_validation() {
    let first = dll_executable::dll(DLL, &dll_executable::attach(DLL, 1, true), None);
    let second = dll_executable::dll(
        DLL + 0x0001_0000,
        &dll_executable::attach(DLL + 0x0001_0000, 2, true),
        Some("first.dll"),
    );
    assert!(matches!(
        Process32::load_with_options(
            &program(),
            96,
            ProcessOptions {
                modules: &[GuestModule {
                    name: "first.dll",
                    bytes: &first,
                }],
                deferred_modules: &[GuestModule {
                    name: "FIRST.dll",
                    bytes: &first,
                }],
                ..ProcessOptions::default()
            },
        ),
        Err(LoadError::DuplicateModule { .. })
    ));
    assert!(matches!(
        Process32::load_with_options(
            &program(),
            96,
            ProcessOptions {
                deferred_modules: &[
                    GuestModule {
                        name: "second.dll",
                        bytes: &second,
                    },
                    GuestModule {
                        name: "first.dll",
                        bytes: &first,
                    },
                ],
                ..ProcessOptions::default()
            },
        ),
        Err(LoadError::DeferredModuleDependency { .. })
    ));
}
