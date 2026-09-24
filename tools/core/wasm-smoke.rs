use ring3_core::{
    AsciiSourcePathBatch, AsciiSourcePathCollision as Collision, AsciiSourcePathEntry,
    AsciiSourcePathError, AsciiSourcePathLimits, AsciiSourcePathSegmentError, FileOffset,
    PeFileRange, PeFileRangeSource, PeFingerprintError, PeHeaderPrefix, PeKind, PeRvaError,
    RelativeVirtualAddress, admit_ascii_source_paths, fingerprint_pe_declared_evidence,
    parse_pe_headers, resolve_pe_file_range,
};

#[path = "../../core/tests/support/executable.rs"]
mod executable;

fn execute_accumulator_sign_extension() {
    use ring3_core::execution::{Cpu32, Register32, StopReason, load_pe32};

    for (eax, edx) in [(39, 0), (0x8000_0000, u32::MAX)] {
        let mut image = load_pe32(&executable::pe32(&[0x99]), 3).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        cpu.set_register(Register32::Eax, eax);
        cpu.set_register(Register32::Edx, 0xa5a5_a5a5);
        cpu.eflags = 0xced7;
        let result = cpu.run(&mut image.memory, 1);
        assert_eq!(result.reason, StopReason::InstructionLimit);
        assert_eq!(result.instructions, 1);
        assert_eq!(cpu.register(Register32::Eax), eax);
        assert_eq!(cpu.register(Register32::Edx), edx);
        assert_eq!(cpu.eflags, 0xced7);
    }
}

#[path = "../../core/tests/support/complement_executable.rs"]
mod complement_executable;

#[path = "../../core/tests/support/string_scan_executable.rs"]
mod string_scan_executable;

#[path = "../../core/tests/support/repeated_moves_executable.rs"]
mod repeated_moves_executable;

#[path = "../../core/tests/support/string_stores_executable.rs"]
mod string_stores_executable;

#[path = "../../core/tests/support/register_stack_executable.rs"]
mod register_stack_executable;

#[path = "../../core/tests/support/flag_stack_executable.rs"]
mod flag_stack_executable;

#[path = "../../core/tests/support/cpuid_executable.rs"]
mod cpuid_executable;

fn execute_cpuid() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&cpuid_executable::pe32(), 32).unwrap();
    let result = process.run(100);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (15, 0));
    let mut actual = [0xff; 32];
    process.memory.read(0x0040_2080, &mut actual).unwrap();
    let mut expected = [0; 32];
    expected[0] = 1;
    expected[4..16].copy_from_slice(b"Ring3CPUCore");
    assert_eq!(actual, expected);
    assert_eq!(process.cpu.register(Register32::Eax), 0x8000_0000);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    assert_eq!(process.cpu.eflags, 0x46);
}

fn execute_flag_stack() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&flag_stack_executable::pe32(), 32).unwrap();
    let result = process.run(100);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (16, 0));
    for (register, expected) in [
        (Register32::Eax, 0),
        (Register32::Ebx, 0x0020_0002),
        (Register32::Ecx, 2),
        (Register32::Edx, 2),
        (Register32::Esp, 0x1001_0000),
    ] {
        assert_eq!(process.cpu.register(register), expected);
    }
    assert_eq!(process.cpu.eflags, 2);
}

fn execute_register_stack() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&register_stack_executable::pe32(), 32).unwrap();
    process.cpu.eflags = 0xced7;
    let result = process.run(100);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (26, 0));
    for (register, expected) in [
        (Register32::Eax, 0xa1a1_3344),
        (Register32::Ecx, 0xa2a2_7788),
        (Register32::Edx, 0xa3a3_bbcc),
        (Register32::Ebx, 0xa4a4_ff00),
        (Register32::Esp, 0x1001_0000),
        (Register32::Ebp, 0xa5a5_3040),
        (Register32::Esi, 0xa6a6_7080),
        (Register32::Edi, 0xa7a7_b0c0),
    ] {
        assert_eq!(process.cpu.register(register), expected);
    }
    assert_eq!(process.cpu.eflags, 0xced7);
}

fn execute_repeated_moves() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&repeated_moves_executable::pe32(), 32).unwrap();
    process.cpu.eflags = 0xcad7;
    let result = process.run(50);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (10, 0));
    assert_eq!(process.cpu.register(Register32::Ecx), 0);
    assert_eq!(process.cpu.register(Register32::Esi), 0x0040_208b);
    assert_eq!(process.cpu.register(Register32::Edi), 0x0040_20cb);
    assert_eq!(process.cpu.eflags, 0xcad7);
    let mut copied = [0; 11];
    process.memory.read(0x0040_20c0, &mut copied).unwrap();
    assert_eq!(copied, [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]);
}

fn execute_string_stores() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut p = Process32::load(&string_stores_executable::pe32(), 32).unwrap();
    p.cpu.set_fs_base(0x5000_0000);
    p.cpu.set_register(Register32::Esi, 0xdead_beef);
    p.cpu.eflags = 0xcad7;
    let run = p.run(40);
    assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((run.instructions, run.api_calls), (10, 0));
    assert_eq!(p.cpu.register(Register32::Eax), 0x1234_5678);
    assert_eq!(p.cpu.register(Register32::Esi), 0xdead_beef);
    assert_eq!(p.cpu.register(Register32::Edi), 0x0040_208b);
    assert_eq!(p.cpu.register(Register32::Ecx), 0);
    assert_eq!(p.cpu.eflags, 0xcad7);
    let mut bytes = [0; 13];
    p.memory.read(0x0040_2080, &mut bytes).unwrap();
    assert_eq!(
        bytes,
        [
            0x78, 0x56, 0x34, 0x12, 0x78, 0x56, 0x34, 0x12, 0x78, 0x56, 0x78, 0xaa, 0xaa
        ]
    );
    #[cfg(windows_demo)]
    {
        let mut p = Process32::load(
            include_bytes!("../../target/windows-api/string-stores.exe"),
            64,
        )
        .unwrap();
        let run = p.run(10000);
        assert_eq!(run.reason, ProcessStop::Exited(42));
        assert_eq!((run.instructions, run.api_calls), (731, 1));
    }
}

#[path = "../../core/tests/support/string_compare_executable.rs"]
mod string_compare_executable;

fn execute_string_comparisons() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&string_compare_executable::pe32(), 32).unwrap();
    let run = process.run(100);
    assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((run.instructions, run.api_calls), (17, 0));
    for register in [Register32::Ebx, Register32::Ebp, Register32::Ecx] {
        assert_eq!(process.cpu.register(register), 1);
    }
    for register in [Register32::Esi, Register32::Edi] {
        assert_eq!(process.cpu.register(register), 0x0040_20c4);
    }
    assert_eq!(process.cpu.eflags, 0x46);
    #[cfg(windows_demo)]
    {
        let mut process = Process32::load(
            include_bytes!("../../target/windows-api/string-comparisons.exe"),
            64,
        )
        .unwrap();
        let run = process.run(1000);
        assert_eq!(run.reason, ProcessStop::Exited(42));
        assert_eq!((run.instructions, run.api_calls), (86, 1));
    }
}

fn execute_string_scan() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&string_scan_executable::pe32(), 32).unwrap();
    let result = process.run(100);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (17, 0));
    assert_eq!(process.cpu.register(Register32::Eax), 0x4242);
    assert_eq!(process.cpu.register(Register32::Ebx), 3);
    assert_eq!(process.cpu.register(Register32::Ecx), 0);
    assert_eq!(process.cpu.register(Register32::Edi), 0x0040_2096);
    assert_eq!(process.cpu.eflags, 2);
}

#[path = "../../core/tests/support/signed_extend_executable.rs"]
mod signed_extend_executable;

fn execute_signed_extension() {
    use ring3_core::execution::{Cpu32, Register32, StopReason, load_pe32};
    let mut image = load_pe32(&signed_extend_executable::pe32(), 3).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_fs_base(0x0040_2000);
    cpu.eflags = 0xced7;
    let result = cpu.run(&mut image.memory, 20);
    assert_eq!(
        (result.reason, result.instructions),
        (StopReason::Breakpoint, 7)
    );
    assert_eq!(cpu.register(Register32::Eax), u32::MAX);
    assert_eq!(cpu.register(Register32::Ebx), 0x1234_ffff);
    assert_eq!(cpu.register(Register32::Ecx), u32::MAX);
    assert_eq!(cpu.register(Register32::Edx), 0xffff_ff80);
    assert_eq!(cpu.eflags, 0xced7);
}

fn execute_complement() {
    use ring3_core::execution::{Cpu32, Register32, StopReason, load_pe32};
    let mut image = load_pe32(&complement_executable::pe32(), 3).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_fs_base(0x0040_2000);
    cpu.eflags = 0xced7;
    let result = cpu.run(&mut image.memory, 50);
    assert_eq!(
        (result.reason, result.instructions),
        (StopReason::Breakpoint, 8)
    );
    assert_eq!(cpu.register(Register32::Eax), 0x1234_5687);
    assert_eq!(cpu.register(Register32::Ebx), 0xffff_ffee);
    assert_eq!(cpu.register(Register32::Esi), 0x7fff_ffff);
    assert_eq!(cpu.eflags, 0xced7);
}

#[path = "../../core/tests/support/imported_executable.rs"]
mod imported_executable;

#[path = "../../core/tests/support/d3d8_executable.rs"]
mod d3d8_executable;

#[path = "../../core/tests/support/thread_executable.rs"]
mod thread_executable;

#[path = "../../core/tests/support/crt_executable.rs"]
mod crt_executable;

#[path = "../../core/tests/support/fp_control_executable.rs"]
mod fp_control_executable;

#[path = "../../core/tests/support/initializer_executable.rs"]
mod initializer_executable;

#[path = "../../core/tests/support/arguments_executable.rs"]
mod arguments_executable;

#[path = "../../core/tests/support/dll_executable.rs"]
mod dll_executable;

#[path = "../../core/tests/support/relocated_executable.rs"]
mod relocated_executable;

#[path = "../../core/tests/support/export_names_executable.rs"]
mod export_names_executable;

#[path = "../../core/tests/support/thread_notifications_executable.rs"]
mod thread_notifications_executable;

#[path = "../../core/tests/support/delay_executable.rs"]
mod delay_executable;

#[path = "../../core/tests/support/modules_executable.rs"]
mod modules_executable;

#[path = "../../core/tests/support/error_mode_executable.rs"]
mod error_mode_executable;

#[path = "../../core/tests/support/heap_executable.rs"]
mod heap_executable;

#[path = "../../core/tests/support/critical_section_executable.rs"]
mod critical_section_executable;

#[path = "../../core/tests/support/tls_executable.rs"]
mod tls_executable;

#[path = "../../core/tests/support/global_memory_executable.rs"]
mod global_memory_executable;

#[path = "../../core/tests/support/memset_executable.rs"]
mod memset_executable;

#[path = "../../core/tests/support/exception_frame_executable.rs"]
mod exception_frame_executable;

#[path = "../../core/tests/support/crt_heap_executable.rs"]
mod crt_heap_executable;

#[path = "../../core/tests/support/dllonexit_executable.rs"]
mod dllonexit_executable;

#[path = "../../core/tests/support/borrow_executable.rs"]
mod borrow_executable;

#[path = "../../core/tests/support/code_pages_executable.rs"]
mod code_pages_executable;

#[path = "../../core/tests/support/cpinfo_executable.rs"]
mod cpinfo_executable;

#[path = "../../core/tests/support/clipboard_formats_executable.rs"]
mod clipboard_formats_executable;
#[path = "../../core/tests/support/messages_executable.rs"]
mod messages_executable;

#[path = "../../core/tests/support/condition_bytes_executable.rs"]
mod condition_bytes_executable;
#[path = "../../core/tests/support/string_moves_executable.rs"]
mod string_moves_executable;
#[path = "../../core/tests/support/zero_extend_executable.rs"]
mod zero_extend_executable;

#[path = "../../core/tests/support/process_version_executable.rs"]
mod process_version_executable;

#[path = "../../core/tests/support/accelerator_executable.rs"]
mod accelerator_executable;
#[path = "../../core/tests/support/metrics_executable.rs"]
mod metrics_executable;

#[path = "../../core/tests/support/gdi_executable.rs"]
mod gdi_executable;

#[path = "../../core/tests/support/brushes_executable.rs"]
mod brushes_executable;

#[path = "../../core/tests/support/cursor_position_executable.rs"]
mod cursor_position_executable;
#[path = "../../core/tests/support/cursors_executable.rs"]
mod cursors_executable;

#[path = "../../core/tests/support/local_realloc_executable.rs"]
mod local_realloc_executable;

#[path = "../../core/tests/support/interlocked_executable.rs"]
mod interlocked_executable;

#[path = "../../core/tests/support/counter_executable.rs"]
mod counter_executable;

#[path = "../../core/tests/support/locale_activity_executable.rs"]
mod locale_activity_executable;

#[path = "../../core/tests/support/locale_metadata_executable.rs"]
mod locale_metadata_executable;

fn execute_locale_metadata() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&locale_metadata_executable::pe32(), 32).unwrap();
    let result = process.run(50);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (12, 0));
    for (register, expected) in [
        (Register32::Eax, 0),
        (Register32::Ebx, 42),
        (Register32::Ecx, 0),
        (Register32::Edx, 0),
        (Register32::Ebp, 1),
    ] {
        assert_eq!(process.cpu.register(register), expected);
    }
}

fn execute_locale_activity() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&counter_executable::pe32(), 32).unwrap();
    let result = process.run(50);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (7, 2));
    assert_eq!(process.cpu.register(Register32::Eax), u32::MAX);
    assert_eq!(process.cpu.register(Register32::Ebx), 0);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    let mut process = Process32::load(&locale_activity_executable::pe32(), 32).unwrap();
    let result = process.run(50);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (9, 0));
    assert_eq!(process.cpu.register(Register32::Eax), 0);
    assert_eq!(process.cpu.register(Register32::Edx), 0);
    assert_eq!(process.cpu.register(Register32::Esi), 42);
    assert_eq!(process.cpu.register(Register32::Edi), 7);
}

#[path = "../../core/tests/support/file_attributes_executable.rs"]
mod file_attributes_executable;
#[path = "../../core/tests/support/registry_default_executable.rs"]
mod registry_default_executable;
#[path = "../../core/tests/support/short_path_executable.rs"]
mod short_path_executable;
#[path = "../../core/tests/support/string_append_executable.rs"]
mod string_append_executable;
#[path = "../../core/tests/support/string_length_executable.rs"]
mod string_length_executable;
#[path = "../../core/tests/support/windows_string_length_executable.rs"]
mod windows_string_length_executable;

#[path = "../../core/tests/support/buffer_copy_executable.rs"]
mod buffer_copy_executable;

#[path = "../../core/tests/support/string_copy_executable.rs"]
mod string_copy_executable;

#[path = "../../core/tests/support/character_search_executable.rs"]
mod character_search_executable;

fn execute_character_search() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&character_search_executable::pe32(), 32).unwrap();
    let result = process.run(50);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (10, 2));
    assert_eq!(process.cpu.register(Register32::Ebx), 0x0040_2180);
    assert_eq!(process.cpu.register(Register32::Eax), 0x0040_2184);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
}

fn execute_string_copy() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&string_copy_executable::pe32(), 32).unwrap();
    let result = process.run(50);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (6, 1));
    assert_eq!(process.cpu.register(Register32::Eax), 0x0040_2190);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    let mut bytes = [0; 10];
    process.memory.read(0x0040_2190, &mut bytes).unwrap();
    assert_eq!(bytes, [b'a', 0x80, b'b', 0, 0, 0, 0, 0, 0x55, 0x55]);
}

fn execute_buffer_copy() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&buffer_copy_executable::pe32(), 32).unwrap();
    let result = process.run(50);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (13, 2));
    assert_eq!(process.cpu.register(Register32::Eax), 0);
    assert_eq!(process.cpu.register(Register32::Ebx), 0x0040_2190);
    assert_eq!(process.cpu.register(Register32::Ecx), 0xff80_0061);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
}

fn execute_string_append() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&string_append_executable::pe32(), 32).unwrap();
    let run = process.run(50);
    assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((run.instructions, run.api_calls), (6, 1));
    assert_eq!(process.cpu.register(Register32::Eax), 0x0040_2190);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    let mut bytes = [0; 8];
    process.memory.read(0x0040_2190, &mut bytes).unwrap();
    assert_eq!(bytes, [b'Q', b'a', 0x80, 0, 0x55, 0x55, 0x55, 0x55]);
}

fn execute_string_length() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&string_length_executable::pe32(), 32).unwrap();
    let result = process.run(50);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (12, 3));
    assert_eq!(process.cpu.register(Register32::Eax), 0);
    assert_eq!(process.cpu.register(Register32::Ebx), 3);
    assert_eq!(process.cpu.register(Register32::Ecx), 2);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
}

fn execute_interlocked() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&interlocked_executable::pe32(), 32).unwrap();
    let result = process.run(50);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (11, 2));
    assert_eq!(process.cpu.register(Register32::Eax), u32::MAX);
    assert_eq!(process.cpu.register(Register32::Ebx), 0x8000_0000);
    assert_eq!(process.cpu.register(Register32::Edi), 42);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    #[cfg(windows_demo)]
    {
        let mut process = Process32::load(
            include_bytes!("../../target/windows-api/interlocked.exe"),
            64,
        )
        .unwrap();
        let result = process.run(1000);
        assert_eq!(result.reason, ProcessStop::Exited(42));
        assert_eq!(result.api_calls, 6);
    }
}

fn execute_local_realloc() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&local_realloc_executable::pe32(), 40).unwrap();
    let result = process.run(100);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (15, 3));
    assert_eq!(process.cpu.register(Register32::Eax), 0);
    assert_eq!(process.cpu.register(Register32::Ecx), 42);
    assert_eq!(process.cpu.register(Register32::Edx), 0);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    assert!(process.memory.read(0x2000_0000, &mut [0]).is_err());
    #[cfg(windows_demo)]
    {
        let mut process = Process32::load(
            include_bytes!("../../target/windows-api/local-realloc.exe"),
            64,
        )
        .unwrap();
        let result = process.run(1000);
        assert_eq!(result.reason, ProcessStop::Exited(42));
        assert_eq!(result.api_calls, 11);
    }
}

#[path = "../../core/tests/support/alternate_teb.rs"]
mod alternate_teb;
#[path = "../../core/tests/support/thread_identity_executable.rs"]
mod thread_identity_executable;

fn execute_alternate_teb() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    for base in [0x1101_0000, 0x5000_0000] {
        for (bytes, expected_eax, expected_error) in [
            (thread_executable::pe32(42), 49, 49),
            (thread_identity_executable::pe32(), 23, 0),
        ] {
            let mut process = Process32::load(&bytes, 32).unwrap();
            alternate_teb::map(&mut process, base, 23);
            process
                .memory
                .write(0x7ffd_e034, &77_u32.to_le_bytes())
                .unwrap();
            process.cpu.set_fs_base(base);
            assert_eq!(
                process.run(100).reason,
                ProcessStop::Stopped(StopReason::Breakpoint)
            );
            assert_eq!(process.cpu.register(Register32::Eax), expected_eax);
            assert_eq!(process.last_error().unwrap(), expected_error);
            process.cpu.set_fs_base(0x7ffd_e000);
            assert_eq!(process.last_error().unwrap(), 77);
        }
    }
}

fn execute_thread_identity() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&thread_identity_executable::pe32(), 25).unwrap();
    let result = process.run(30);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (7, 3));
    assert_eq!(process.cpu.register(Register32::Eax), 1);
    assert_eq!(process.cpu.register(Register32::Ebx), u32::MAX - 1);
    assert_eq!(process.cpu.register(Register32::Ecx), 1);
    assert_eq!(process.cpu.register(Register32::Edx), 1);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    #[cfg(windows_demo)]
    {
        let mut process = Process32::load(
            include_bytes!("../../target/windows-api/thread-identity.exe"),
            64,
        )
        .unwrap();
        let result = process.run(1000);
        assert_eq!(result.reason, ProcessStop::Exited(42));
        assert_eq!(result.api_calls, 11);
    }
}

#[path = "../../core/tests/support/module_file_name_executable.rs"]
mod module_file_name_executable;

fn execute_module_file_name() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&module_file_name_executable::pe32(), 25).unwrap();
    let result = process.run(40);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (11, 2));
    assert_eq!(process.cpu.register(Register32::Eax), 3);
    assert_eq!(process.cpu.register(Register32::Ebx), 14);
    assert_eq!(
        process.cpu.register(Register32::Ecx),
        u32::from_le_bytes(*b"C:\\p")
    );
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    let mut path = [0; 15];
    process.memory.read(0x0040_2180, &mut path).unwrap();
    assert_eq!(&path, b"C:\\program.exe\0");
    #[cfg(windows_demo)]
    {
        let mut process = Process32::load(
            include_bytes!("../../target/windows-api/module-file-name.exe"),
            64,
        )
        .unwrap();
        let result = process.run(1000);
        assert_eq!(result.reason, ProcessStop::Exited(42));
        assert_eq!(result.api_calls, 12);
    }
}

#[path = "../../core/tests/support/resource_executable.rs"]
mod resource_executable;

fn execute_accelerators() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&accelerator_executable::guest(), 32).unwrap();
    let run = process.run(100);
    assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((run.instructions, run.api_calls), (14, 3));
    assert_eq!(process.cpu.register(Register32::Eax), 2);
    assert_eq!(process.cpu.register(Register32::Ebx), 0x7700_0004);
    assert_eq!(process.cpu.register(Register32::Ecx), 2);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    let mut bytes = [0; 12];
    process.memory.read(0x0040_2180, &mut bytes).unwrap();
    assert_eq!(bytes, [9, 0, 65, 0, 100, 0, 0, 0, 120, 0, 200, 0]);
    #[cfg(windows_demo)]
    {
        let mut process = Process32::load(
            include_bytes!("../../target/windows-api/accelerators.exe"),
            64,
        )
        .unwrap();
        let run = process.run(1000);
        assert_eq!(run.reason, ProcessStop::Exited(42));
        assert_eq!((run.instructions, run.api_calls), (118, 12));
    }
}

fn execute_resources() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&resource_executable::guest(), 25).unwrap();
    let result = process.run(50);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (12, 2));
    assert_eq!(process.cpu.register(Register32::Eax), 3);
    assert_eq!(process.cpu.register(Register32::Ebx), 0x0040_2348);
    assert_eq!(process.cpu.register(Register32::Ecx), 0x0070_6c41);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    assert_eq!(process.memory.mapped_pages(), 25);
    #[cfg(windows_demo)]
    {
        let mut process =
            Process32::load(include_bytes!("../../target/windows-api/resources.exe"), 64).unwrap();
        let result = process.run(1000);
        assert_eq!(result.reason, ProcessStop::Exited(42));
        assert_eq!((result.instructions, result.api_calls), (189, 18));
    }
}

#[path = "../../core/tests/support/system_directory_executable.rs"]
mod system_directory_executable;

#[path = "../../core/tests/support/computer_name_executable.rs"]
mod computer_name_executable;

#[path = "../../core/tests/support/event_executable.rs"]
mod event_executable;
#[path = "../../core/tests/support/mutex_executable.rs"]
mod mutex_executable;
#[path = "../../core/tests/support/suspended_thread_executable.rs"]
mod suspended_thread_executable;

#[path = "../../core/tests/support/performance_clock_executable.rs"]
mod performance_clock_executable;

#[path = "../../core/tests/support/current_directory_executable.rs"]
mod current_directory_executable;

#[path = "../../core/tests/support/command_line_executable.rs"]
mod command_line_executable;

fn execute_command_line() {
    use ring3_core::execution::{Process32, ProcessOptions, ProcessStop, Register32, StopReason};
    let mut process = Process32::load_with_options(
        &command_line_executable::pe32(),
        32,
        ProcessOptions {
            command_line: b"\"demo.exe\" --mode test",
            ..ProcessOptions::default()
        },
    )
    .unwrap();
    let result = process.run(50);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (4, 2));
    let pointer = process.cpu.register(Register32::Eax);
    assert_eq!(process.cpu.register(Register32::Ebx), pointer);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    let mut output = [0; 23];
    process
        .memory
        .read(u64::from(pointer), &mut output)
        .unwrap();
    assert_eq!(&output, b"\"demo.exe\" --mode test\0");
    #[cfg(windows_demo)]
    {
        let mut process = Process32::load(
            include_bytes!("../../target/windows-api/command-line.exe"),
            64,
        )
        .unwrap();
        assert_eq!(process.run(1000).reason, ProcessStop::Exited(42));
    }
}

#[path = "../../core/tests/support/change_directory_executable.rs"]
mod change_directory_executable;

fn execute_change_directory() {
    use ring3_core::execution::{Process32, ProcessOptions, ProcessStop, Register32, StopReason};
    let mut process = Process32::load_with_options(
        &change_directory_executable::pe32(),
        64,
        ProcessOptions {
            current_directory: b"Q:\\Initial",
            directories: &[b"D:\\Assets"],
            ..ProcessOptions::default()
        },
    )
    .unwrap();
    process.memory.write(0x0040_2180, b"d:\\Assets\0").unwrap();
    let result = process.run(50);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (7, 2));
    assert_eq!(process.cpu.register(Register32::Eax), 9);
    assert_eq!(process.cpu.register(Register32::Ebx), 1);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    let mut bytes = [0; 10];
    process.memory.read(0x0040_2280, &mut bytes).unwrap();
    assert_eq!(&bytes, b"d:\\Assets\0");
    #[cfg(windows_demo)]
    {
        let mut process = Process32::load(
            include_bytes!("../../target/windows-api/change-directory.exe"),
            64,
        )
        .unwrap();
        assert_eq!(process.run(1000).reason, ProcessStop::Exited(42));
    }
}

#[path = "../../core/tests/support/find_files_executable.rs"]
mod find_files_executable;

fn execute_find_files() {
    use ring3_core::execution::{
        FileMetadata, Process32, ProcessOptions, ProcessStop, Register32, StopReason,
    };
    let files = [
        FileMetadata {
            path: b"C:\\alpha.dat",
            size: 0x1_0000_0007,
        },
        FileMetadata {
            path: b"C:\\beta.bin",
            size: 5,
        },
    ];
    let options = ProcessOptions {
        files: &files,
        directories: &[b"C:\\Data"],
        ..ProcessOptions::default()
    };
    let mut process =
        Process32::load_with_options(&find_files_executable::pe32(), 64, options).unwrap();
    process.memory.write(0x0040_2180, b"*TA.bin\0").unwrap();
    let result = process.run(50);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (10, 3));
    assert_eq!(process.cpu.register(Register32::Eax), 1);
    assert_eq!(process.cpu.register(Register32::Ebx), 0x7300_0004);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    let mut record = [0; 320];
    process.memory.read(0x0040_2280, &mut record).unwrap();
    assert_eq!(record[0], 0x80);
    assert_eq!(&record[32..36], &5_u32.to_le_bytes());
    assert_eq!(&record[44..53], b"beta.bin\0");
    #[cfg(windows_demo)]
    for (options, steps, calls) in [(ProcessOptions::default(), 57, 10), (options, 151, 21)] {
        let mut process = Process32::load_with_options(
            include_bytes!("../../target/windows-api/find-files.exe"),
            64,
            options,
        )
        .unwrap();
        let result = process.run(1000);
        assert_eq!(result.reason, ProcessStop::Exited(42));
        assert_eq!((result.instructions, result.api_calls), (steps, calls));
    }
}

#[path = "../../core/tests/support/millisecond_clock_executable.rs"]
mod millisecond_clock_executable;

#[path = "../../core/tests/support/random_executable.rs"]
mod random_executable;

#[path = "../../core/tests/support/float_to_integer_executable.rs"]
mod float_to_integer_executable;

#[path = "../../core/tests/support/type_name_executable.rs"]
mod type_name_executable;

#[path = "../../core/tests/support/string_prefix_executable.rs"]
mod string_prefix_executable;

#[path = "../../core/tests/support/case_compare_executable.rs"]
mod case_compare_executable;

#[path = "../../core/tests/support/formatting_executable.rs"]
mod formatting_executable;

#[path = "../../core/tests/support/x87_register_executable.rs"]
mod x87_register_executable;
#[path = "../../core/tests/support/x87_status_executable.rs"]
mod x87_status_executable;

#[path = "../../core/tests/support/division_executable.rs"]
mod division_executable;

#[path = "../../core/tests/support/wide_product_executable.rs"]
mod wide_product_executable;

#[path = "../../core/tests/support/division_cases.rs"]
mod division_cases;
#[path = "../../core/tests/support/unsigned_division_executable.rs"]
mod unsigned_division_executable;

fn execute_unsigned_division() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    division_cases::arithmetic();
    division_cases::errors();
    for budget in [1, 100] {
        let mut process = Process32::load(&unsigned_division_executable::pe32(), 32).unwrap();
        let mut steps = 0;
        loop {
            let run = process.run(budget);
            steps += run.instructions;
            if run.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                break;
            }
            assert!(steps < 100);
        }
        assert_eq!(steps, 14);
        assert_eq!(process.cpu.register(Register32::Esi), 252_648_990);
        assert_eq!(process.cpu.register(Register32::Edi), 5);
        assert_eq!(process.cpu.register(Register32::Eax), 0xabcd_00ff);
        assert_eq!(process.cpu.register(Register32::Edx), 4);
    }
    #[cfg(windows_demo)]
    {
        let mut process = Process32::load(
            include_bytes!("../../target/windows-api/unsigned-division.exe"),
            64,
        )
        .unwrap();
        let run = process.run(1000);
        assert_eq!(run.reason, ProcessStop::Exited(42));
        assert_eq!(run.api_calls, 1);
    }
}

fn execute_wide_product() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&wide_product_executable::pe32(), 32).unwrap();
    let run = process.run(40);
    assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((run.instructions, run.api_calls), (10, 0));
    assert_eq!(process.cpu.register(Register32::Eax), 0xabcd_fffa);
    for register in [Register32::Edx, Register32::Esi, Register32::Edi] {
        assert_eq!(process.cpu.register(register), u32::MAX);
    }
    assert_eq!(process.cpu.eflags, 2);
    #[cfg(windows_demo)]
    {
        let mut process = Process32::load(
            include_bytes!("../../target/windows-api/signed-products.exe"),
            64,
        )
        .unwrap();
        let run = process.run(1000);
        assert_eq!(run.reason, ProcessStop::Exited(42));
        assert_eq!((run.instructions, run.api_calls), (59, 1));
    }
}

fn execute_x87_division() {
    use ring3_core::execution::{Cpu32, Register32, StopReason, load_pe32};
    for (numerator, divisor, expected, status) in [
        (8_f64, 2_f32, 0x3ff5_5555_5555_5555_u64, 0x20),
        (-8., 2., 0xbff5_5555_5555_5555, 0x20),
        (0., -2., 0x8000_0000_0000_0000, 0),
    ] {
        let image = load_pe32(&division_executable::pe32(), 32).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        let mut memory = image.memory;
        memory.write(0x0040_2188, &numerator.to_le_bytes()).unwrap();
        memory.write(0x0040_2190, &divisor.to_le_bytes()).unwrap();
        let run = cpu.run(&mut memory, 40);
        assert_eq!(run.reason, StopReason::Breakpoint);
        assert_eq!(run.instructions, 9);
        assert_eq!(cpu.register(Register32::Esi), 0x3800);
        assert_eq!(cpu.register(Register32::Eax), status);
        let mut bytes = [0; 8];
        memory.read(0x0040_21a0, &mut bytes).unwrap();
        assert_eq!(u64::from_le_bytes(bytes), expected);
    }
}

fn execute_x87_memory_sum() {
    use ring3_core::execution::{Cpu32, Register32, StopReason, load_pe32};
    for (mode, status) in [(0x25, 0x220), (0x05, 0x20)] {
        let code = [
            0xdd, 0x05, 0x00, 0x22, 0x40, 0x00, 0xd8, mode, 0x10, 0x22, 0x40, 0x00, 0xdf, 0xe0,
            0xdd, 0x1d, 0x20, 0x22, 0x40, 0x00,
        ];
        let image = load_pe32(&executable::pe32(&code), 16).unwrap();
        let mut memory = image.memory;
        memory.write(0x0040_2200, &1_f64.to_le_bytes()).unwrap();
        memory
            .write(0x0040_2210, &f32::MIN_POSITIVE.to_le_bytes())
            .unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        cpu.set_x87_control_word(0x027f);
        assert_eq!(cpu.run(&mut memory, 3).reason, StopReason::InstructionLimit);
        assert_eq!(cpu.register(Register32::Eax) & 0x220, status);
        assert_eq!(cpu.run(&mut memory, 1).reason, StopReason::InstructionLimit);
        let mut bytes = [0; 8];
        memory.read(0x0040_2220, &mut bytes).unwrap();
        assert_eq!(f64::from_le_bytes(bytes), 1.0);
    }
}

fn execute_x87_register_add() {
    use ring3_core::execution::{Cpu32, Register32, StopReason, load_pe32};
    let code = [
        0xdd, 0x05, 0x00, 0x22, 0x40, 0x00, 0xdc, 0xc0, 0xdf, 0xe0, 0xdd, 0x1d, 0x20, 0x22, 0x40,
        0x00,
    ];
    let image = load_pe32(&executable::pe32(&code), 16).unwrap();
    let mut memory = image.memory;
    memory.write(0x0040_2200, &1.5_f64.to_le_bytes()).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_x87_control_word(0x027f);
    assert_eq!(cpu.run(&mut memory, 3).reason, StopReason::InstructionLimit);
    assert_eq!(cpu.register(Register32::Eax) & 0x220, 0);
    assert_eq!(cpu.run(&mut memory, 1).reason, StopReason::InstructionLimit);
    let mut bytes = [0; 8];
    memory.read(0x0040_2220, &mut bytes).unwrap();
    assert_eq!(u64::from_le_bytes(bytes), 3.0_f64.to_bits());
}

fn execute_x87_divide_pop() {
    use ring3_core::execution::{Cpu32, Register32, StopReason, load_pe32};
    for (mode, indexed, top) in [(0xf1, 2.0_f64, 8.0_f64), (0xf9, 8.0, 2.0)] {
        let code = [
            0xdd, 0x05, 0x00, 0x22, 0x40, 0x00, 0xdd, 0x05, 0x08, 0x22, 0x40, 0x00, 0xde, mode,
            0xdf, 0xe0, 0xdd, 0x1d, 0x20, 0x22, 0x40, 0x00,
        ];
        let image = load_pe32(&executable::pe32(&code), 16).unwrap();
        let mut memory = image.memory;
        memory.write(0x0040_2200, &indexed.to_le_bytes()).unwrap();
        memory.write(0x0040_2208, &top.to_le_bytes()).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        cpu.set_x87_control_word(0x027f);
        assert_eq!(cpu.run(&mut memory, 4).reason, StopReason::InstructionLimit);
        assert_eq!(cpu.register(Register32::Eax) & 0x3a20, 0x3800);
        assert_eq!(cpu.run(&mut memory, 1).reason, StopReason::InstructionLimit);
        let mut bytes = [0; 8];
        memory.read(0x0040_2220, &mut bytes).unwrap();
        assert_eq!(u64::from_le_bytes(bytes), 4.0_f64.to_bits());
    }
}

fn execute_x87_status() {
    use ring3_core::execution::{Cpu32, Register32, StopReason, load_pe32};
    for (denominator, numerator, first, last) in [
        (10_f64, 1_f64, 0x3a20, 0x20),
        (3., -1., 0x3820, 0x120),
        (1., 0., 0x3800, 0x4000),
    ] {
        let image = load_pe32(&x87_status_executable::pe32(), 32).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        let mut memory = image.memory;
        memory
            .write(0x0040_2188, &denominator.to_le_bytes())
            .unwrap();
        memory.write(0x0040_2190, &numerator.to_le_bytes()).unwrap();
        let run = cpu.run(&mut memory, 40);
        assert_eq!(run.reason, StopReason::Breakpoint);
        assert_eq!(run.instructions, 8);
        assert_eq!(cpu.register(Register32::Esi), first);
        assert_eq!(cpu.register(Register32::Eax), last);
    }
    #[cfg(windows_demo)]
    {
        use ring3_core::execution::{Process32, ProcessStop};
        let mut process = Process32::load(
            include_bytes!("../../target/windows-api/floating-status.exe"),
            64,
        )
        .unwrap();
        let run = process.run(1000);
        assert_eq!(run.reason, ProcessStop::Exited(42));
        assert_eq!((run.instructions, run.api_calls), (70, 1));
    }
}

#[path = "../../core/tests/support/lowercase_executable.rs"]
mod lowercase_executable;

#[path = "../../core/tests/support/argument_pointer_executable.rs"]
mod argument_pointer_executable;

#[path = "../../core/tests/support/file_remove_executable.rs"]
mod file_remove_executable;

fn execute_file_removal() {
    use ring3_core::execution::{
        FileMetadata, Process32, ProcessOptions, ProcessStop, Register32, StopReason,
    };
    let files = [FileMetadata {
        path: b"C:\\folder\\erase.tmp",
        size: 42,
    }];
    for (files, expected) in [(&files[..], 0), (&[][..], u32::MAX)] {
        let mut process = Process32::load_with_options(
            &file_remove_executable::pe32(),
            64,
            ProcessOptions {
                files,
                ..ProcessOptions::default()
            },
        )
        .unwrap();
        let run = process.run(100);
        assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
        assert_eq!((run.instructions, run.api_calls), (4, 1));
        assert_eq!(process.cpu.register(Register32::Eax), expected);
        assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    }
}

fn execute_argument_pointers() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&argument_pointer_executable::pe32(), 32).unwrap();
    let run = process.run(100);
    assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((run.instructions, run.api_calls), (4, 2));
    assert_eq!(process.cpu.register(Register32::Ebx), 0x7000_2010);
    assert_eq!(process.cpu.register(Register32::Eax), 0x7000_2014);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
}

fn execute_crt_lowercase() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    for (character, expected) in [(0x54_u32, 0x74), (0xff, 0xff), (0, 0)] {
        let mut bytes = lowercase_executable::pe32();
        bytes[0x201..0x205].copy_from_slice(&character.to_le_bytes());
        let mut process = Process32::load(&bytes, 32).unwrap();
        let run = process.run(40);
        assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
        assert_eq!((run.instructions, run.api_calls), (8, 2));
        assert_eq!(process.cpu.register(Register32::Esi), expected);
        assert_eq!(process.cpu.register(Register32::Eax), u32::MAX);
        assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    }
}

fn execute_crt_formatting() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    for (capacity, result, expected) in [
        (32_u8, 13_i32, b"ok:-42:ABCD:%\0!".as_slice()),
        (13, 13, b"ok:-42:ABCD:%!"),
        (5, -1, b"ok:-4!"),
    ] {
        let mut executable = formatting_executable::pe32();
        executable[0x20b] = capacity;
        let mut p = Process32::load(&executable, 32).unwrap();
        p.memory.write(0x0040_2200, &[b'!'; 32]).unwrap();
        let run = p.run(40);
        assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
        assert_eq!((run.instructions, run.api_calls), (7, 1));
        assert_eq!(p.cpu.register(Register32::Eax), result.cast_unsigned());
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
        let mut bytes = vec![0; expected.len()];
        p.memory.read(0x0040_2200, &mut bytes).unwrap();
        assert_eq!(bytes, expected);
    }
}

fn execute_crt_scanning() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let bytes = imported_executable::pe32(&[0xcc], "MSVCRT.dll", &["sscanf"]);
    for (input, format, expected) in [
        (
            b" -42tail\0".as_slice(),
            b"%d\0".as_slice(),
            (-42_i32).cast_unsigned(),
        ),
        (b" 1.25tail\0", b"%f\0".as_slice(), 1.25_f32.to_bits()),
        (b" 4294967295tail\0", b"%u\0".as_slice(), u32::MAX),
        (b" 0xabcdef12tail\0", b"%x\0".as_slice(), 0xabcd_ef12),
    ] {
        let mut process = Process32::load(&bytes, 32).unwrap();
        process.memory.write(0x0040_2300, input).unwrap();
        process.memory.write(0x0040_2180, format).unwrap();
        let frame: Vec<_> = [0x0040_1000_u32, 0x0040_2300, 0x0040_2180, 0x0040_2400]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        process.memory.write(0x1000_ef00, &frame).unwrap();
        process.cpu.eip = 0x7000_01b8;
        process.cpu.set_register(Register32::Esp, 0x1000_ef00);
        let result = process.run(1);
        assert_eq!(
            result.reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
        assert_eq!((result.instructions, result.api_calls), (0, 1));
        assert_eq!(process.cpu.register(Register32::Eax), 1);
        assert_eq!(process.cpu.register(Register32::Esp), 0x1000_ef04);
        let mut output = [0; 4];
        process.memory.read(0x0040_2400, &mut output).unwrap();
        assert_eq!(u32::from_le_bytes(output), expected);
    }
}

fn execute_crt_case_comparison() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    for (left, right, expected) in [
        (b"MiXeD\0".as_slice(), b"mixed\0".as_slice(), 0_i32),
        (b"Z\0", b"[\0", 31),
        (b"\xc4\0", b"\xe4\0", -32),
    ] {
        let mut p = Process32::load(&case_compare_executable::pe32(), 32).unwrap();
        p.memory.write(0x0040_2180, left).unwrap();
        p.memory.write(0x0040_2190, right).unwrap();
        let run = p.run(40);
        assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
        assert_eq!((run.instructions, run.api_calls), (10, 2));
        assert_eq!(p.cpu.register(Register32::Esi), expected.cast_unsigned());
        assert_eq!(p.cpu.register(Register32::Eax), 2);
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
    }
}

fn execute_crt_string_prefix() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    for (left, right, expected) in [
        (b"abcD\0".as_slice(), b"abcZ\0".as_slice(), -22_i32),
        (b"abc\0D", b"abc\0Z", 0),
        (b"abc\x80\0", b"abc\x7f\0", 1),
    ] {
        let mut p = Process32::load(&string_prefix_executable::pe32(), 32).unwrap();
        p.memory.write(0x0040_2180, left).unwrap();
        p.memory.write(0x0040_2190, right).unwrap();
        let run = p.run(40);
        assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
        assert_eq!((run.instructions, run.api_calls), (12, 2));
        assert_eq!(p.cpu.register(Register32::Esi), 0);
        assert_eq!(p.cpu.register(Register32::Eax), expected.cast_unsigned());
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
    }
}

fn execute_crt_type_names() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    for (raw, expected) in [
        (".?AVWidget@engine@@", "class engine::Widget"),
        (".?AUNode@inner@outer@@", "struct outer::inner::Node"),
        (".?AT_Value2@@", "union _Value2"),
        (".?AW4Mode@engine@@", "enum engine::Mode"),
    ] {
        let mut p = Process32::load(&type_name_executable::pe32(), 32).unwrap();
        p.memory
            .write(0x0040_2188, format!("{raw}\0").as_bytes())
            .unwrap();
        let result = p.run(40);
        assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
        assert_eq!((result.instructions, result.api_calls), (5, 2));
        let pointer = p.cpu.register(Register32::Eax);
        assert_eq!(pointer, p.cpu.register(Register32::Esi));
        let mut cache = [0; 4];
        p.memory.read(0x0040_2184, &mut cache).unwrap();
        assert_eq!(cache, pointer.to_le_bytes());
        let mut name = vec![0; expected.len() + 1];
        p.memory.read(u64::from(pointer), &mut name).unwrap();
        assert_eq!(name, format!("{expected}\0").as_bytes());
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
    }
}

fn execute_crt_float_to_integer() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    for (first, low, high) in [
        (0x41f0_0000_001c_0000_u64, 1_u32, 1_u32),
        (0x43df_ffff_ffff_ffff, 0xffff_fc00, 0x7fff_ffff),
        (0xc3e0_0000_0000_0000, 0, 0x8000_0000),
    ] {
        let mut p = Process32::load(&float_to_integer_executable::pe32(), 32).unwrap();
        p.memory.write(0x0040_2180, &first.to_le_bytes()).unwrap();
        let result = p.run(40);
        assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
        assert_eq!((result.instructions, result.api_calls), (7, 2));
        assert_eq!(p.cpu.register(Register32::Esi), low);
        assert_eq!(p.cpu.register(Register32::Edi), high);
        assert_eq!(p.cpu.register(Register32::Eax), u32::MAX);
        assert_eq!(p.cpu.register(Register32::Edx), u32::MAX);
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
    }
}

fn execute_crt_random() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&random_executable::pe32(), 32).unwrap();
    let result = process.run(40);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (9, 4));
    assert_eq!(process.cpu.register(Register32::Ebx), 41);
    assert_eq!(process.cpu.register(Register32::Esi), 5890);
    assert_eq!(process.cpu.register(Register32::Eax), 1279);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
}

fn execute_millisecond_clock() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    use std::time::Duration;
    let mut process = Process32::load(&millisecond_clock_executable::pe32(), 32).unwrap();
    process
        .set_elapsed_time(Duration::from_nanos(4_294_968_530_999_999))
        .unwrap();
    let result = process.run(20);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (4, 2));
    assert_eq!(process.cpu.register(Register32::Eax), 1234);
    assert_eq!(process.cpu.register(Register32::Ebx), 1234);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    #[cfg(windows_demo)]
    {
        let mut process = Process32::load(
            include_bytes!("../../target/windows-api/millisecond-clock.exe"),
            64,
        )
        .unwrap();
        process
            .set_elapsed_time(Duration::from_millis(5_250))
            .unwrap();
        let result = process.run(1000);
        assert_eq!(result.reason, ProcessStop::Exited(42));
        assert_eq!((result.instructions, result.api_calls), (19, 5));
    }
}

#[path = "../../core/tests/support/x87_executable.rs"]
mod x87_executable;

#[path = "../../core/tests/support/x87_scaling_executable.rs"]
mod x87_scaling_executable;

#[path = "../../core/tests/support/fpu_wait_executable.rs"]
mod fpu_wait_executable;

#[path = "../../core/tests/support/integer_store_executable.rs"]
mod integer_store_executable;

fn execute_integer_stores() {
    use ring3_core::execution::{Cpu32, StopReason, load_pe32};
    for (input, expected) in [
        (1.5_f64, [2_i64, 1, 2, 1]),
        (2.5, [2, 2, 3, 2]),
        (-1.5, [-2, -2, -1, -1]),
        (-2.5, [-2, -3, -2, -2]),
    ] {
        for (rc, integer) in expected.into_iter().enumerate() {
            let mut image = load_pe32(&integer_store_executable::pe32(), 4).unwrap();
            image
                .memory
                .write(0x0040_2180, &input.to_le_bytes())
                .unwrap();
            let control = 0x027f | (u16::try_from(rc).unwrap() << 10);
            image
                .memory
                .write(0x0040_2190, &control.to_le_bytes())
                .unwrap();
            let mut cpu = Cpu32::new(image.entry_point);
            let result = cpu.run(&mut image.memory, 20);
            assert_eq!(result.reason, StopReason::Breakpoint);
            assert_eq!(result.instructions, 6);
            assert_eq!(cpu.x87_control_word(), control);
            let mut bytes = [0; 16];
            image.memory.read(0x0040_21a0, &mut bytes).unwrap();
            let expected = integer.to_le_bytes();
            assert_eq!(&bytes[..2], &expected[..2]);
            assert_eq!(&bytes[2..4], &[0xaa, 0xaa]);
            assert_eq!(&bytes[4..8], &expected[..4]);
            assert_eq!(&bytes[8..], &expected);
        }
    }
}

fn execute_fpu_wait() {
    use ring3_core::execution::{Cpu32, StopReason, load_pe32};
    let mut image = load_pe32(&fpu_wait_executable::pe32(), 4).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    let result = cpu.run(&mut image.memory, 20);
    assert_eq!(result.reason, StopReason::Breakpoint);
    assert_eq!(result.instructions, 6);
    assert_eq!(cpu.x87_control_word(), 0x0f7f);
    let mut bytes = [0; 4];
    image.memory.read(0x0040_2180, &mut bytes).unwrap();
    assert_eq!(bytes, [0x7f, 3, 0xaa, 0xaa]);
}

fn execute_x87_scaling() {
    use ring3_core::execution::{Cpu32, StopReason, load_pe32};
    for (input, factor, expected) in [
        (-7_i64, 0x3fc0_0000_u32, 0xc025_0000_0000_0000_u64),
        (3, 0x3eaa_aaab, 0x3ff0_0000_0800_0000),
        (0, 0xbf80_0000, 0x8000_0000_0000_0000),
        (1_i64 << 53, 0x3fc0_0000, 0x4348_0000_0000_0000),
        ((1_i64 << 52) + 1, 0x3fc0_0000, 0x4338_0000_0000_0002),
        ((1_i64 << 52) + 3, 0x3fc0_0000, 0x4338_0000_0000_0004),
    ] {
        let mut image = load_pe32(&x87_scaling_executable::pe32(), 16).unwrap();
        image
            .memory
            .write(0x0040_2180, &input.to_le_bytes())
            .unwrap();
        image
            .memory
            .write(0x0040_2188, &factor.to_le_bytes())
            .unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        let run = cpu.run(&mut image.memory, 20);
        assert_eq!(run.reason, StopReason::Breakpoint);
        assert_eq!(run.instructions, 5);
        let mut bytes = [0; 8];
        image.memory.read(0x0040_21a0, &mut bytes).unwrap();
        assert_eq!(u64::from_le_bytes(bytes), expected);
    }
    let mut image = load_pe32(&x87_scaling_executable::pe32(), 16).unwrap();
    image
        .memory
        .write(0x0040_2180, &1_000_000_000_i64.to_le_bytes())
        .unwrap();
    image
        .memory
        .write(0x0040_2188, &1_000_000_000.0_f32.to_le_bytes())
        .unwrap();
    image
        .memory
        .write(0x0040_2190, &0x007f_u16.to_le_bytes())
        .unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    let run = cpu.run(&mut image.memory, 3);
    assert_eq!(run.reason, StopReason::InstructionLimit);
    assert_eq!(run.instructions, 3);
    assert_eq!(cpu.x87_control_word(), 0x007f);
    cpu.set_x87_control_word(0x027f);
    let run = cpu.run(&mut image.memory, 2);
    assert_eq!(run.reason, StopReason::Breakpoint);
    assert_eq!(run.instructions, 2);
    let mut bytes = [0; 8];
    image.memory.read(0x0040_21a0, &mut bytes).unwrap();
    assert_eq!(u64::from_le_bytes(bytes), 0x43ab_c16d_6000_0000);
}

fn execute_x87_data() {
    use ring3_core::execution::{Cpu32, StopReason, load_pe32};
    for (input, dividend, expected) in [
        (0x4080_0000_u32, 0x4100_0000_u32, 0x4080_0000_u32),
        (0x4110_0000, 0x3f80_0000, 0x3eaa_aaab),
        (0x4000_0000, 0x3f80_0000, 0x3f35_04f3),
    ] {
        let mut image = load_pe32(&x87_executable::pe32(), 16).unwrap();
        image
            .memory
            .write(0x0040_2180, &input.to_le_bytes())
            .unwrap();
        image
            .memory
            .write(0x0040_2184, &dividend.to_le_bytes())
            .unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        let result = cpu.run(&mut image.memory, 20);
        assert_eq!(result.reason, StopReason::Breakpoint);
        assert_eq!(result.instructions, 6);
        let mut bytes = [0; 4];
        image.memory.read(0x0040_2188, &mut bytes).unwrap();
        assert_eq!(u32::from_le_bytes(bytes), expected);
    }
    #[cfg(windows_demo)]
    {
        use ring3_core::execution::{Process32, ProcessStop};
        let mut process = Process32::load(
            include_bytes!("../../target/windows-api/floating-point.exe"),
            64,
        )
        .unwrap();
        let result = process.run(1000);
        assert_eq!(result.reason, ProcessStop::Exited(42));
        assert_eq!((result.instructions, result.api_calls), (288, 1));
    }
}

#[path = "../../core/tests/support/environment_executable.rs"]
mod environment_executable;

#[path = "../../core/tests/support/startup_info_executable.rs"]
mod startup_info_executable;

#[path = "../../core/tests/support/hook_executable.rs"]
mod hook_executable;
#[path = "../../core/tests/support/procedure_executable.rs"]
mod procedure_executable;
#[path = "../../core/tests/support/registry_executable.rs"]
mod registry_executable;
#[path = "../../core/tests/support/registry_value_executable.rs"]
mod registry_value_executable;

#[path = "../../core/tests/support/thread_priority_executable.rs"]
mod thread_priority_executable;

#[path = "../../core/tests/support/windows_format_executable.rs"]
mod windows_format_executable;

#[path = "../../core/tests/support/window_class_executable.rs"]
mod window_class_executable;

#[path = "../../core/tests/support/window_proc_executable.rs"]
mod window_proc_executable;

#[path = "../../core/tests/support/window_creation_executable.rs"]
mod window_creation_executable;

#[path = "../../core/tests/support/window_property_cases.rs"]
mod window_property_cases;

#[path = "../../core/tests/support/hook_chain_cases.rs"]
mod hook_chain_cases;

#[path = "../../core/tests/support/icon_executable.rs"]
mod icon_executable;

#[path = "../../core/tests/support/window_message_cases.rs"]
mod window_message_cases;

#[path = "../../core/tests/support/path_component_cases.rs"]
mod path_component_cases;

#[path = "../../core/tests/support/file_stream_cases.rs"]
mod file_stream_cases;

#[cfg(windows_demo)]
fn verify_compiled_file_streams() -> (u64, u64) {
    use file_stream_cases::PAYLOAD;
    use ring3_core::execution::{
        FileContents, FileMetadata, Process32, ProcessOptions, ProcessStop, StopReason,
    };
    let mut prior = None;
    for budget in [1, 10000] {
        let mut p = Process32::load_with_options(
            include_bytes!("../../target/windows-api/file-streams.exe"),
            128,
            ProcessOptions {
                files: &[FileMetadata {
                    path: b"C:\\sample.bin",
                    size: PAYLOAD.len() as u64,
                }],
                file_contents: &[FileContents {
                    path: b"C:\\sample.bin",
                    bytes: PAYLOAD,
                }],
                ..ProcessOptions::default()
            },
        )
        .unwrap();
        let pages = p.memory.mapped_pages();
        let mut counts = (0, 0);
        loop {
            let result = p.run(budget);
            counts.0 += result.instructions;
            counts.1 += result.api_calls;
            if result.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(result.reason, ProcessStop::Exited(42));
                break;
            }
            assert!(counts.0 + counts.1 < 10000);
        }
        assert_eq!(p.memory.mapped_pages(), pages);
        assert_eq!(p.last_error().unwrap(), 77);
        if let Some(previous) = prior {
            assert_eq!((p.cpu, counts), previous);
        }
        prior = Some((p.cpu, counts));
    }
    prior.unwrap().1
}

fn execute_file_streams() {
    file_stream_cases::verify();
    #[cfg(windows_demo)]
    verify_compiled_file_streams();
}

fn execute_path_components() {
    path_component_cases::verify();
    #[cfg(windows_demo)]
    {
        use ring3_core::execution::{Process32, ProcessStop, StopReason};
        let mut final_state = None;
        for budget in [1, 100000] {
            let mut process = Process32::load(
                include_bytes!("../../target/windows-api/path-components.exe"),
                64,
            )
            .unwrap();
            let mut counts = (0, 0);
            loop {
                let result = process.run(budget);
                counts.0 += result.instructions;
                counts.1 += result.api_calls;
                if result.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                    assert_eq!(result.reason, ProcessStop::Exited(42));
                    break;
                }
                assert!(counts.0 + counts.1 < 100000);
            }
            assert_eq!(process.last_error().unwrap(), 77);
            if let Some(prior) = final_state {
                assert_eq!((process.cpu, counts), prior);
            }
            final_state = Some((process.cpu, counts));
        }
    }
}

fn execute_window_messages() {
    window_message_cases::verify();
    #[cfg(windows_demo)]
    {
        use ring3_core::execution::{Process32, ProcessStop, StopReason};
        let mut final_state = None;
        for budget in [1, 10000] {
            let mut process = Process32::load(
                include_bytes!("../../target/windows-api/window-messages.exe"),
                64,
            )
            .unwrap();
            let mut counts = (0, 0);
            loop {
                let result = process.run(budget);
                counts.0 += result.instructions;
                counts.1 += result.api_calls;
                if result.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                    assert_eq!(result.reason, ProcessStop::Exited(42));
                    break;
                }
                assert!(counts.0 + counts.1 < 10000);
            }
            assert_eq!(process.last_error().unwrap(), 77);
            if let Some(prior) = final_state {
                assert_eq!((process.cpu, counts), prior);
            }
            final_state = Some((process.cpu, counts));
        }
    }
}

#[cfg(windows_demo)]
fn execute_get_message_wait() {
    use ring3_core::execution::{PostedMessage, Process32, ProcessStop, Register32};
    let mut process = Process32::load(
        include_bytes!("../../target/windows-api/get-message-wait.exe"),
        64,
    )
    .unwrap();
    let first = process.run(1000);
    assert_eq!(first.reason, ProcessStop::WaitingForMessage);
    assert_eq!(first.api_calls, 1);
    assert_eq!(process.cpu.eip, 0x7000_0448);
    let cpu = process.cpu;
    let stack = process.cpu.register(Register32::Esp);
    let mut pointer = [0; 4];
    process
        .memory
        .read(u64::from(stack + 4), &mut pointer)
        .unwrap();
    let mut message = [0; 32];
    process
        .memory
        .read(u64::from(u32::from_le_bytes(pointer)), &mut message)
        .unwrap();
    assert_eq!(message, [0x5a; 32]);
    assert_eq!(process.last_error().unwrap(), 77);
    let repeated = process.run(1);
    assert_eq!(repeated.reason, ProcessStop::WaitingForMessage);
    assert_eq!((repeated.instructions, repeated.api_calls), (0, 0));
    assert_eq!(process.cpu, cpu);
    process
        .post_message(PostedMessage {
            hwnd: 0,
            message: 0x401,
            wparam: 42,
            lparam: 0x1020_3040,
            time: 1234,
            point: [12, -5],
        })
        .unwrap();
    let resumed = process.run(1000);
    assert_eq!(resumed.reason, ProcessStop::Exited(42));
    assert_eq!(process.last_error().unwrap(), 77);
}

#[cfg(windows_demo)]
fn execute_translate_message() {
    use ring3_core::execution::{PostedMessage, Process32, ProcessStop};
    let mut process = Process32::load(
        include_bytes!("../../target/windows-api/translate-message.exe"),
        64,
    )
    .unwrap();
    assert_eq!(process.run(1000).reason, ProcessStop::WaitingForMessage);
    process
        .post_message(PostedMessage {
            hwnd: 0,
            message: 0x100,
            wparam: 13,
            lparam: 0x001c_0001,
            time: 1234,
            point: [12, -5],
        })
        .unwrap();
    assert_eq!(process.run(1000).reason, ProcessStop::Exited(42));
    assert_eq!(process.last_error().unwrap(), 77);
}

#[cfg(windows_demo)]
fn execute_dispatch_message() {
    use ring3_core::execution::{PostedMessage, Process32, ProcessStop};
    let mut process = Process32::load(
        include_bytes!("../../target/windows-api/dispatch-message.exe"),
        64,
    )
    .unwrap();
    let first = process.run(10000);
    assert_eq!(first.reason, ProcessStop::WaitingForMessage);
    assert_eq!((first.instructions, first.api_calls), (144, 7));
    process
        .post_message(PostedMessage {
            hwnd: 0x7500_0004,
            message: 0x401,
            wparam: 10,
            lparam: 32,
            time: 1234,
            point: [12, -5],
        })
        .unwrap();
    let resumed = process.run(1000);
    assert_eq!(resumed.reason, ProcessStop::Exited(42));
    assert_eq!((resumed.instructions, resumed.api_calls), (77, 7));
    assert_eq!(process.last_error().unwrap(), 77);
}

#[cfg(windows_demo)]
fn execute_dialog_cbt() {
    use ring3_core::execution::{Process32, ProcessStop};
    let mut process = Process32::load(
        include_bytes!("../../target/windows-api/dialog-cbt.exe"),
        64,
    )
    .unwrap();
    let run = process.run(1000);
    assert_eq!(run.reason, ProcessStop::Exited(42));
    assert_eq!((run.instructions, run.api_calls), (201, 11));
}

fn execute_icons() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let bytes = icon_executable::guest();
    let fixtures = std::iter::once((
        bytes.as_slice(),
        ProcessStop::Stopped(StopReason::Breakpoint),
    ));
    #[cfg(windows_demo)]
    let fixtures = fixtures.chain(std::iter::once((
        include_bytes!("../../target/windows-api/icons.exe").as_slice(),
        ProcessStop::Exited(42),
    )));
    for (bytes, stop) in fixtures {
        let mut final_state = None;
        for budget in [1, 10000] {
            let mut process = Process32::load(bytes, 64).unwrap();
            let mut counts = (0, 0);
            loop {
                let run = process.run(budget);
                counts.0 += run.instructions;
                counts.1 += run.api_calls;
                if run.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                    assert_eq!(run.reason, stop);
                    break;
                }
                assert!(counts.0 + counts.1 < 10000);
            }
            if matches!(stop, ProcessStop::Stopped(_)) {
                assert_eq!(process.cpu.register(Register32::Eax), 0x7800_0004);
                assert_eq!(process.cpu.register(Register32::Ebx), 0x7800_0004);
            } else {
                assert_eq!(process.last_error().unwrap(), 1814);
            }
            if let Some(expected) = final_state {
                assert_eq!((process.cpu, counts), expected);
            }
            final_state = Some((process.cpu, counts));
        }
    }
}

fn execute_hook_chain() {
    hook_chain_cases::verify();
    #[cfg(windows_demo)]
    {
        use ring3_core::execution::{Process32, ProcessStop, StopReason};
        let mut final_state = None;
        for budget in [1, 10000] {
            let mut process = Process32::load(
                include_bytes!("../../target/windows-api/hook-chain.exe"),
                64,
            )
            .unwrap();
            let mut counts = (0, 0);
            loop {
                let run = process.run(budget);
                counts.0 += run.instructions;
                counts.1 += run.api_calls;
                if run.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                    assert_eq!(run.reason, ProcessStop::Exited(42));
                    break;
                }
                assert!(counts.0 + counts.1 < 10000);
            }
            assert_eq!(process.last_error().unwrap(), 77);
            if let Some(expected) = final_state {
                assert_eq!((process.cpu, counts), expected);
            }
            final_state = Some((process.cpu, counts));
        }
    }
}

fn execute_window_properties() {
    window_property_cases::ownership();
    #[cfg(windows_demo)]
    {
        use ring3_core::execution::{Process32, ProcessStop, StopReason};
        let mut final_state = None;
        for budget in [1, 10000] {
            let mut process = Process32::load(
                include_bytes!("../../target/windows-api/window-properties.exe"),
                64,
            )
            .unwrap();
            let mut counts = (0, 0);
            loop {
                let run = process.run(budget);
                counts.0 += run.instructions;
                counts.1 += run.api_calls;
                if run.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                    assert_eq!(run.reason, ProcessStop::Exited(42));
                    break;
                }
                assert!(counts.0 + counts.1 < 10000);
            }
            assert_eq!(process.last_error().unwrap(), 77);
            if let Some(expected) = final_state {
                assert_eq!((process.cpu, counts), expected);
            }
            final_state = Some((process.cpu, counts));
        }
    }
}

fn execute_window_creation() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut final_state = None;
    for budget in [1, 1000] {
        let mut process = Process32::load(&window_creation_executable::guest(), 32).unwrap();
        let mut counts = (0, 0);
        loop {
            let run = process.run(budget);
            counts.0 += run.instructions;
            counts.1 += run.api_calls;
            if run.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                break;
            }
            assert!(counts.0 + counts.1 < 1000);
        }
        assert_eq!(process.cpu.register(Register32::Eax), 1);
        assert_eq!(process.cpu.register(Register32::Ebx), 0x7500_0004);
        assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
        let windows = process.window_snapshots();
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].hwnd, 0x7500_0004);
        assert_eq!(windows[0].title, "title");
        assert_eq!(windows[0].parent, 0);
        if let Some(expected) = final_state {
            assert_eq!((process.cpu, counts), expected);
        }
        final_state = Some((process.cpu, counts));
        let rectangle = windows[0].rectangle;
        let frame: Vec<_> = [0x0040_1000_u32, 0x7500_0004, 5]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        process.memory.write(0x1000_ef00, &frame).unwrap();
        process.cpu.eip = 0x7000_0440;
        process.cpu.set_register(Register32::Esp, 0x1000_ef00);
        let run = process.run(1);
        assert_eq!(
            run.reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
        assert_eq!((run.instructions, run.api_calls), (0, 1));
        assert_eq!(process.cpu.register(Register32::Eax), 0);
        let shown = process.window_snapshots();
        assert_eq!(shown[0].rectangle, rectangle);
        assert_ne!(shown[0].style & 0x1000_0000, 0);
        assert!(shown[0].active);
    }
    #[cfg(windows_demo)]
    {
        let mut final_state = None;
        for budget in [1, 10000] {
            let mut process = Process32::load(
                include_bytes!("../../target/windows-api/window-creation.exe"),
                64,
            )
            .unwrap();
            let mut counts = (0, 0);
            loop {
                let run = process.run(budget);
                counts.0 += run.instructions;
                counts.1 += run.api_calls;
                if run.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                    assert_eq!(run.reason, ProcessStop::Exited(42));
                    break;
                }
                assert!(counts.0 + counts.1 < 10000);
            }
            assert_eq!(process.last_error().unwrap(), 77);
            if let Some(expected) = final_state {
                assert_eq!((process.cpu, counts), expected);
            }
            final_state = Some((process.cpu, counts));
        }
    }
    #[cfg(windows_demo)]
    {
        let mut process =
            Process32::load(include_bytes!("../../target/windows-api/dialog.exe"), 64).unwrap();
        let result = process.run(1000);
        assert_eq!(result.reason, ProcessStop::Exited(42));
        assert_eq!((result.instructions, result.api_calls), (798, 95));
    }
}

fn execute_window_procedures() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    for (bytes, expected, value) in [
        (window_proc_executable::guest(), (12, 1), 60),
        (window_proc_executable::nested(), (23, 3), 61),
    ] {
        let mut final_cpu = None;
        for budget in [1, 100] {
            let mut process = Process32::load(&bytes, 32).unwrap();
            let mut counts = (0, 0);
            loop {
                let run = process.run(budget);
                counts.0 += run.instructions;
                counts.1 += run.api_calls;
                if run.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                    assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                    break;
                }
                assert!(counts.0 + counts.1 < 100);
            }
            assert_eq!(counts, expected);
            assert_eq!(process.cpu.register(Register32::Eax), value);
            assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
            if let Some(expected) = final_cpu {
                assert_eq!(process.cpu, expected);
            }
            final_cpu = Some(process.cpu);
        }
    }
    #[cfg(windows_demo)]
    for budget in [1, 1000] {
        let mut process = Process32::load(
            include_bytes!("../../target/windows-api/window-procedures.exe"),
            64,
        )
        .unwrap();
        let mut counts = (0, 0);
        loop {
            let run = process.run(budget);
            counts.0 += run.instructions;
            counts.1 += run.api_calls;
            if run.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(run.reason, ProcessStop::Exited(42));
                break;
            }
            assert!(counts.0 + counts.1 < 1000);
        }
        assert_eq!(counts.1, 11);
        assert_eq!(process.last_error().unwrap(), 55);
    }
}

#[path = "../../core/tests/support/desktop_query_executable.rs"]
mod desktop_query_executable;

fn execute_desktop_queries() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&desktop_query_executable::pe32(), 32).unwrap();
    let run = process.run(100);
    assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((run.instructions, run.api_calls), (8, 3));
    assert_eq!(process.cpu.register(Register32::Eax), 0);
    assert_eq!(process.cpu.register(Register32::Ebx), 1);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    #[cfg(windows_demo)]
    {
        let mut process = Process32::load(
            include_bytes!("../../target/windows-api/desktop-queries.exe"),
            64,
        )
        .unwrap();
        let run = process.run(1000);
        assert_eq!(run.reason, ProcessStop::Exited(42));
        assert_eq!((run.instructions, run.api_calls), (79, 13));
    }
}

fn execute_window_classes() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&window_class_executable::pe32(), 32).unwrap();
    let run = process.run(100);
    assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((run.instructions, run.api_calls), (11, 3));
    assert_eq!(process.cpu.register(Register32::Eax), 1);
    assert_eq!(process.cpu.register(Register32::Ebx), 0xc000);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    let mut record = [0; 40];
    process.memory.read(0x0040_2280, &mut record).unwrap();
    assert_eq!(record[..4], 3_u32.to_le_bytes());
    assert_eq!(record[36..], 0x0040_2180_u32.to_le_bytes());
    #[cfg(windows_demo)]
    {
        let mut process = Process32::load(
            include_bytes!("../../target/windows-api/window-classes.exe"),
            64,
        )
        .unwrap();
        let run = process.run(1000);
        assert_eq!(run.reason, ProcessStop::Exited(42));
        assert_eq!((run.instructions, run.api_calls), (132, 18));
    }
}

fn execute_windows_formatting() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&windows_format_executable::pe32(), 32).unwrap();
    let run = process.run(100);
    assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((run.instructions, run.api_calls), (7, 1));
    assert_eq!(process.cpu.register(Register32::Eax), 10);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    let mut text = [0xff; 11];
    process.memory.read(0x0040_2400, &mut text).unwrap();
    assert_eq!(&text, b"-42:ABCD:%\0");
    #[cfg(windows_demo)]
    {
        let mut process = Process32::load(
            include_bytes!("../../target/windows-api/formatting.exe"),
            64,
        )
        .unwrap();
        let run = process.run(2000);
        assert_eq!(run.reason, ProcessStop::Exited(42));
        assert_eq!((run.instructions, run.api_calls), (1594, 9));
    }
}

fn execute_buffer_move() {
    #[cfg(windows_demo)]
    {
        use ring3_core::execution::{Process32, ProcessStop};
        let mut process = Process32::load(
            include_bytes!("../../target/windows-api/buffer-move.exe"),
            64,
        )
        .unwrap();
        let run = process.run(2000);
        assert_eq!(run.reason, ProcessStop::Exited(42));
        assert_eq!((run.instructions, run.api_calls), (572, 9));
    }
}

fn execute_thread_priority() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&thread_priority_executable::pe32(), 32).unwrap();
    let run = process.run(100);
    assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((run.instructions, run.api_calls), (8, 4));
    assert_eq!(process.cpu.register(Register32::Eax), 1);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    #[cfg(windows_demo)]
    {
        let mut process = Process32::load(
            include_bytes!("../../target/windows-api/thread-priority.exe"),
            64,
        )
        .unwrap();
        let run = process.run(1000);
        assert_eq!(run.reason, ProcessStop::Exited(42));
        assert_eq!((run.instructions, run.api_calls), (208, 24));
    }
}

fn execute_x87_register_stores() {
    use ring3_core::execution::{Cpu32, Register32, StopReason, load_pe32};
    let image = load_pe32(&x87_register_executable::pe32(), 32).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    let mut memory = image.memory;
    let run = cpu.run(&mut memory, 40);
    assert_eq!(run.reason, StopReason::Breakpoint);
    assert_eq!(run.instructions, 10);
    assert_eq!(cpu.register(Register32::Eax), 0);
    let mut output = [0; 8];
    memory.read(0x0040_21a0, &mut output).unwrap();
    assert_eq!(output, (-0_f64).to_le_bytes());
    #[cfg(windows_demo)]
    {
        use ring3_core::execution::{Process32, ProcessStop};
        let mut process = Process32::load(
            include_bytes!("../../target/windows-api/floating-registers.exe"),
            64,
        )
        .unwrap();
        let run = process.run(1000);
        assert_eq!(run.reason, ProcessStop::Exited(42));
        assert_eq!((run.instructions, run.api_calls), (44, 1));
    }
}

fn execute_procedure_lookup() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&procedure_executable::pe32(), 32).unwrap();
    let run = process.run(100);
    assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((run.instructions, run.api_calls), (8, 3));
    assert_eq!(process.cpu.register(Register32::Ebx), 0x7000_0030);
    assert_eq!(process.cpu.register(Register32::Eax), 0x0a28_0105);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    #[cfg(windows_demo)]
    {
        let mut process = Process32::load(
            include_bytes!("../../target/windows-api/procedures.exe"),
            64,
        )
        .unwrap();
        let run = process.run(1000);
        assert_eq!(run.reason, ProcessStop::Exited(42));
        assert_eq!((run.instructions, run.api_calls), (78, 14));
    }
}

fn execute_registry_keys() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&registry_executable::pe32(), 32).unwrap();
    let run = process.run(100);
    assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((run.instructions, run.api_calls), (22, 3));
    assert_eq!(process.cpu.register(Register32::Eax), 1);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    #[cfg(windows_demo)]
    {
        let mut process =
            Process32::load(include_bytes!("../../target/windows-api/registry.exe"), 64).unwrap();
        let run = process.run(1000);
        assert_eq!(run.reason, ProcessStop::Exited(42));
        assert_eq!((run.instructions, run.api_calls), (152, 16));
    }
}

fn execute_registry_values() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&registry_value_executable::pe32(), 32).unwrap();
    let run = process.run(100);
    assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((run.instructions, run.api_calls), (16, 2));
    assert_eq!(process.cpu.register(Register32::Eax), 0x7856_3412);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    #[cfg(windows_demo)]
    {
        let mut process = Process32::load(
            include_bytes!("../../target/windows-api/registry-values.exe"),
            64,
        )
        .unwrap();
        let run = process.run(1000);
        assert_eq!(run.reason, ProcessStop::Exited(42));
        assert_eq!((run.instructions, run.api_calls), (202, 15));
    }
}

fn execute_windows_string_length() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&windows_string_length_executable::pe32(), 32).unwrap();
    let run = process.run(100);
    assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((run.instructions, run.api_calls), (9, 3));
    assert_eq!(process.cpu.register(Register32::Eax), 0);
    assert_eq!(process.cpu.register(Register32::Ebx), 3);
    assert_eq!(process.cpu.register(Register32::Ecx), 2);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    #[cfg(windows_demo)]
    {
        let mut process = Process32::load(
            include_bytes!("../../target/windows-api/string-length.exe"),
            64,
        )
        .unwrap();
        let run = process.run(1000);
        assert_eq!(run.reason, ProcessStop::Exited(42));
        assert_eq!((run.instructions, run.api_calls), (44, 8));
    }
}

fn execute_file_attributes() {
    use ring3_core::execution::{
        FileMetadata, Process32, ProcessOptions, ProcessStop, Register32, StopReason,
    };
    let files = [
        FileMetadata {
            path: b"C:\\sample.bin",
            size: 42,
        },
        FileMetadata {
            path: b"C:\\Folder\\leaf.bin",
            size: 7,
        },
    ];
    for (files, value) in [(&[][..], u32::MAX), (&files[..], 128)] {
        let options = ProcessOptions {
            files,
            ..ProcessOptions::default()
        };
        let mut process =
            Process32::load_with_options(&file_attributes_executable::pe32(), 64, options).unwrap();
        let run = process.run(100);
        assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
        assert_eq!((run.instructions, run.api_calls), (6, 2));
        assert_eq!(process.cpu.register(Register32::Eax), value);
        assert_eq!(process.cpu.register(Register32::Ebx), 16);
        assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
        #[cfg(windows_demo)]
        {
            let mut process = Process32::load_with_options(
                include_bytes!("../../target/windows-api/file-attributes.exe"),
                64,
                options,
            )
            .unwrap();
            let run = process.run(1000);
            assert_eq!(run.reason, ProcessStop::Exited(42));
            let counts = if files.is_empty() { (36, 8) } else { (78, 16) };
            assert_eq!((run.instructions, run.api_calls), counts);
        }
    }
}

fn execute_short_path() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&short_path_executable::pe32(), 32).unwrap();
    let run = process.run(50);
    assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((run.instructions, run.api_calls), (10, 2));
    assert_eq!(process.cpu.register(Register32::Eax), 4);
    assert_eq!(process.cpu.register(Register32::Ebx), 3);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    let mut bytes = [0; 4];
    process.memory.read(0x0040_21a0, &mut bytes).unwrap();
    assert_eq!(&bytes, b"c:\\\0");
    #[cfg(windows_demo)]
    {
        use ring3_core::execution::{FileMetadata, ProcessOptions};
        let files = [FileMetadata {
            path: b"C:\\Long Folder\\Long File.bin",
            size: 42,
        }];
        for files in [&[][..], &files[..]] {
            let mut process = Process32::load_with_options(
                include_bytes!("../../target/windows-api/short-path.exe"),
                64,
                ProcessOptions {
                    files,
                    ..ProcessOptions::default()
                },
            )
            .unwrap();
            let run = process.run(5000);
            assert_eq!(run.reason, ProcessStop::Exited(42));
            assert_eq!(run.api_calls, 8);
            assert_eq!(run.instructions, if files.is_empty() { 67 } else { 834 });
        }
    }
}

fn execute_registry_defaults() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&registry_default_executable::pe32(), 64).unwrap();
    let run = process.run(100);
    assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((run.instructions, run.api_calls), (20, 3));
    assert_eq!(process.cpu.register(Register32::Eax), 0);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    let mut bytes = [0; 6];
    process.memory.read(0x0040_2220, &mut bytes).unwrap();
    assert_eq!(&bytes, b"hello\0");
    #[cfg(windows_demo)]
    {
        let mut process = Process32::load(
            include_bytes!("../../target/windows-api/registry-defaults.exe"),
            64,
        )
        .unwrap();
        let run = process.run(5000);
        assert_eq!(run.reason, ProcessStop::Exited(42));
        assert_eq!((run.instructions, run.api_calls), (1111, 23));
    }
}

fn execute_hook_registration() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    for bytes in [
        hook_executable::pe32(),
        hook_executable::keyboard(),
        hook_executable::cbt(),
    ] {
        let mut process = Process32::load(&bytes, 32).unwrap();
        let run = process.run(100);
        assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
        assert_eq!((run.instructions, run.api_calls), (11, 3));
        assert_eq!(process.cpu.register(Register32::Eax), 0);
        assert_eq!(process.cpu.register(Register32::Ebx), 0x7400_0004);
        assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
        assert_eq!(process.last_error().unwrap(), 1404);
    }
    #[cfg(windows_demo)]
    {
        let mut process =
            Process32::load(include_bytes!("../../target/windows-api/hooks.exe"), 64).unwrap();
        let run = process.run(1000);
        assert_eq!(run.reason, ProcessStop::Exited(42));
        assert_eq!((run.instructions, run.api_calls), (277, 40));
    }
}

fn execute_startup_information() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&startup_info_executable::pe32(), 32).unwrap();
    let run = process.run(40);
    assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((run.instructions, run.api_calls), (4, 1));
    assert_eq!(process.cpu.register(Register32::Eax), 0x1234_5678);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    let mut bytes = [0xff; 68];
    process.memory.read(0x0040_2180, &mut bytes).unwrap();
    let mut expected = [0; 68];
    expected[0] = 68;
    assert_eq!(bytes, expected);
    #[cfg(windows_demo)]
    {
        let mut process = Process32::load(
            include_bytes!("../../target/windows-api/startup-info.exe"),
            64,
        )
        .unwrap();
        let run = process.run(10000);
        assert_eq!(run.reason, ProcessStop::Exited(42));
        assert_eq!((run.instructions, run.api_calls), (1429, 5));
    }
}

fn execute_environment_query() {
    use ring3_core::execution::{Process32, ProcessOptions, ProcessStop, Register32, StopReason};
    let options = ProcessOptions {
        environment: &[b"demo=hello", b"empty="],
        ..ProcessOptions::default()
    };
    let mut process =
        Process32::load_with_options(&environment_executable::pe32(), 64, options).unwrap();
    let result = process.run(50);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (5, 1));
    assert_eq!(process.cpu.register(Register32::Eax), 5);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    let mut bytes = [0; 6];
    process.memory.read(0x0040_2280, &mut bytes).unwrap();
    assert_eq!(&bytes, b"hello\0");
    #[cfg(windows_demo)]
    for (options, steps, calls) in [(ProcessOptions::default(), 36, 6), (options, 115, 13)] {
        let mut process = Process32::load_with_options(
            include_bytes!("../../target/windows-api/environment.exe"),
            64,
            options,
        )
        .unwrap();
        let result = process.run(1000);
        assert_eq!(result.reason, ProcessStop::Exited(42));
        assert_eq!((result.instructions, result.api_calls), (steps, calls));
    }
}

#[path = "../../core/tests/support/file_status_executable.rs"]
mod file_status_executable;

fn execute_file_status() {
    use ring3_core::execution::{
        FileMetadata, Process32, ProcessOptions, ProcessStop, Register32, StopReason,
    };
    let mut process = Process32::load_with_options(
        &file_status_executable::pe32(),
        64,
        ProcessOptions {
            current_directory: b"D:\\Apps",
            files: &[FileMetadata {
                path: b"D:\\Apps\\RUN.ExE",
                size: 123,
            }],
            ..ProcessOptions::default()
        },
    )
    .unwrap();
    process.memory.write(0x0040_2180, b"run.exe\0").unwrap();
    let result = process.run(50);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (5, 1));
    assert_eq!(process.cpu.register(Register32::Eax), 0);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    let mut record = [0; 36];
    process.memory.read(0x0040_2280, &mut record).unwrap();
    let mut expected = [0; 36];
    expected[0] = 3;
    expected[16] = 3;
    expected[8] = 1;
    expected[6..8].copy_from_slice(&0x81ff_u16.to_le_bytes());
    expected[20] = 123;
    assert_eq!(record, expected);
}

fn execute_current_directory() {
    use ring3_core::execution::{Process32, ProcessOptions, ProcessStop, Register32, StopReason};
    let mut process = Process32::load_with_options(
        &current_directory_executable::pe32(),
        32,
        ProcessOptions {
            current_directory: b"Q:\\Data",
            ..ProcessOptions::default()
        },
    )
    .unwrap();
    let result = process.run(50);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (8, 2));
    assert_eq!(process.cpu.register(Register32::Eax), 7);
    assert_eq!(process.cpu.register(Register32::Ebx), 8);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    let mut output = [0; 8];
    process.memory.read(0x0040_2280, &mut output).unwrap();
    assert_eq!(&output, b"Q:\\Data\0");
    #[cfg(windows_demo)]
    {
        let mut process = Process32::load(
            include_bytes!("../../target/windows-api/current-directory.exe"),
            64,
        )
        .unwrap();
        assert_eq!(process.run(1000).reason, ProcessStop::Exited(42));
    }
}

fn execute_performance_clock() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    use std::time::Duration;
    let mut process = Process32::load(&performance_clock_executable::pe32(), 32).unwrap();
    process
        .set_elapsed_time(Duration::from_nanos(5_000_000_123))
        .unwrap();
    let result = process.run(50);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (8, 3));
    assert_eq!(process.cpu.register(Register32::Eax), 1);
    assert_eq!(process.cpu.register(Register32::Ebx), 1);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    let mut output = [0; 24];
    process.memory.read(0x0040_2280, &mut output).unwrap();
    for (bytes, value) in
        output
            .chunks_exact(8)
            .zip([1_000_000_000_u64, 5_000_000_123, 5_000_000_123])
    {
        assert_eq!(bytes, value.to_le_bytes());
    }
    #[cfg(windows_demo)]
    {
        let mut process = Process32::load(
            include_bytes!("../../target/windows-api/performance-clock.exe"),
            64,
        )
        .unwrap();
        process
            .set_elapsed_time(Duration::from_nanos(5_000_000_123))
            .unwrap();
        assert_eq!(process.run(1000).reason, ProcessStop::Exited(42));
    }
}

fn execute_mutex_lifecycle() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&mutex_executable::pe32(), 32).unwrap();
    let result = process.run(50);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (15, 5));
    assert_eq!(process.cpu.register(Register32::Eax), 1);
    assert_eq!(process.cpu.register(Register32::Ebx), 0x7200_0004);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    let mut process = Process32::load(&mutex_executable::named_pe32(), 32).unwrap();
    let result = process.run(100);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (32, 10));
    assert_eq!(process.cpu.register(Register32::Eax), 0);
    assert_eq!(process.cpu.register(Register32::Ebx), 0x7200_0004);
    assert_eq!(process.cpu.register(Register32::Esi), 0x7200_0008);
    assert_eq!(process.cpu.register(Register32::Edi), 0x7200_000c);
    assert_eq!(process.cpu.register(Register32::Ebp), 1);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    assert_eq!(process.last_error().unwrap(), 288);
    #[cfg(windows_demo)]
    {
        let mut process =
            Process32::load(include_bytes!("../../target/windows-api/mutex.exe"), 64).unwrap();
        assert_eq!(process.run(1000).reason, ProcessStop::Exited(42));
    }
}

fn execute_suspended_thread() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut p = Process32::load(&suspended_thread_executable::pe32(), 64).unwrap();
    let pages = p.memory.mapped_pages();
    let result = p.run(100);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (9, 1));
    assert_eq!(p.cpu.register(Register32::Eax), 0x7200_0004);
    assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
    assert_eq!(p.memory.mapped_pages(), pages + 17);
    for (address, expected) in [
        (0x0040_2200, 2_u32),
        (0x1101_0024, 2),
        (0x1100_fffc, 0x1234_5678),
    ] {
        let mut bytes = [0; 4];
        p.memory.read(address, &mut bytes).unwrap();
        assert_eq!(u32::from_le_bytes(bytes), expected);
    }
}

fn execute_event_lifecycle() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    for manual in [false, true] {
        let mut process = Process32::load(&event_executable::pe32(manual), 32).unwrap();
        let result = process.run(100);
        assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
        assert_eq!((result.instructions, result.api_calls), (28, 8));
        for (register, expected) in [
            (Register32::Eax, 258),
            (Register32::Ebx, 0x7200_0004),
            (Register32::Esi, 258),
            (Register32::Edi, 0),
            (Register32::Ebp, if manual { 0 } else { 258 }),
            (Register32::Esp, 0x1001_0000),
        ] {
            assert_eq!(process.cpu.register(register), expected);
        }
        assert_eq!(process.last_error().unwrap(), 0);
    }
}

fn execute_computer_name() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&computer_name_executable::pe32(), 32).unwrap();
    let result = process.run(50);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (12, 2));
    for (register, expected) in [
        (Register32::Eax, 1),
        (Register32::Ebx, 0),
        (Register32::Ecx, 6),
        (Register32::Edx, 5),
        (Register32::Esi, 0x474e_4952),
        (Register32::Esp, 0x1001_0000),
    ] {
        assert_eq!(process.cpu.register(register), expected);
    }
    #[cfg(windows_demo)]
    {
        let mut process = Process32::load(
            include_bytes!("../../target/windows-api/computer-name.exe"),
            64,
        )
        .unwrap();
        let result = process.run(1000);
        assert_eq!(result.reason, ProcessStop::Exited(42));
    }
}

fn execute_system_directory() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&system_directory_executable::pe32(), 25).unwrap();
    let result = process.run(50);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (9, 2));
    assert_eq!(process.cpu.register(Register32::Eax), 19);
    assert_eq!(process.cpu.register(Register32::Ebx), 20);
    assert_eq!(process.cpu.register(Register32::Ecx), 0x0032_336d);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    let mut bytes = [0; 20];
    process.memory.read(0x0040_2180, &mut bytes).unwrap();
    assert_eq!(&bytes, b"C:\\Windows\\System32\0");
    #[cfg(windows_demo)]
    {
        let mut process = Process32::load(
            include_bytes!("../../target/windows-api/system-directory.exe"),
            64,
        )
        .unwrap();
        let result = process.run(1000);
        assert_eq!(result.reason, ProcessStop::Exited(42));
        assert_eq!((result.instructions, result.api_calls), (569, 8));
    }
}

fn execute_cursor_position() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&cursor_position_executable::pe32(), 32).unwrap();
    let result = process.run(30);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (8, 2));
    assert_eq!(process.cpu.register(Register32::Eax), 1);
    assert_eq!(process.cpu.register(Register32::Ecx), 210);
    assert_eq!(process.cpu.register(Register32::Edx), 120);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    #[cfg(windows_demo)]
    {
        let mut process = Process32::load(
            include_bytes!("../../target/windows-api/cursor-position.exe"),
            64,
        )
        .unwrap();
        let result = process.run(1000);
        assert_eq!(result.reason, ProcessStop::Exited(42));
        assert_eq!(result.api_calls, 10);
    }
}

fn execute_cursors() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&cursors_executable::pe32(), 32).unwrap();
    let result = process.run(50);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (13, 5));
    assert_eq!(process.cpu.register(Register32::Eax), 0);
    assert_ne!(process.cpu.register(Register32::Ebx), 0);
    assert_eq!(
        process.cpu.register(Register32::Ebx),
        process.cpu.register(Register32::Esi)
    );
    assert_eq!(
        process.cpu.register(Register32::Ebx),
        process.cpu.register(Register32::Edi)
    );
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    #[cfg(windows_demo)]
    {
        let mut process =
            Process32::load(include_bytes!("../../target/windows-api/cursors.exe"), 64).unwrap();
        let result = process.run(1000);
        assert_eq!(result.reason, ProcessStop::Exited(42));
        assert_eq!(result.api_calls, 16);
    }
}

fn execute_brushes() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&brushes_executable::lifecycle(), 32).unwrap();
    let result = process.run(50);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (15, 4));
    assert_eq!(
        process.cpu.register(Register32::Eax),
        process.cpu.register(Register32::Ebx)
    );
    assert_ne!(process.cpu.register(Register32::Eax), 0);
    assert_eq!(process.cpu.register(Register32::Edx), 0x00c8_d0d4);
    assert_eq!(process.cpu.register(Register32::Esi), 12);
    assert_eq!(process.cpu.register(Register32::Edi), 1);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    #[cfg(windows_demo)]
    {
        let mut process =
            Process32::load(include_bytes!("../../target/windows-api/brushes.exe"), 64).unwrap();
        let result = process.run(1000);
        assert_eq!(result.reason, ProcessStop::Exited(42));
        assert_eq!(result.api_calls, 17);
    }
}

fn execute_gdi() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&gdi_executable::lifecycle(), 32).unwrap();
    let result = process.run(50);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (19, 5));
    assert_eq!(process.cpu.register(Register32::Eax), 0);
    assert_eq!(process.cpu.register(Register32::Edx), 1);
    assert_eq!(process.cpu.register(Register32::Esi), 640);
    assert_eq!(process.cpu.register(Register32::Edi), 480);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    #[cfg(windows_demo)]
    {
        let mut process =
            Process32::load(include_bytes!("../../target/windows-api/gdi.exe"), 64).unwrap();
        let result = process.run(1000);
        assert_eq!(result.reason, ProcessStop::Exited(42));
        assert_eq!(result.api_calls, 32);
    }
}

#[path = "../../core/tests/support/colors_executable.rs"]
mod colors_executable;

fn execute_colors() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&colors_executable::pe32(), 32).unwrap();
    let result = process.run(30);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (12, 4));
    assert_eq!(process.cpu.register(Register32::Eax), 0);
    assert_eq!(process.cpu.register(Register32::Ebx), 0x00c8_d0d4);
    assert_eq!(process.cpu.register(Register32::Ecx), 0x00a5_6e3a);
    assert_eq!(process.cpu.register(Register32::Edx), 0x00ff_ffff);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    #[cfg(windows_demo)]
    {
        let mut process =
            Process32::load(include_bytes!("../../target/windows-api/colors.exe"), 64).unwrap();
        let result = process.run(1000);
        assert_eq!(result.reason, ProcessStop::Exited(42));
        assert_eq!(result.api_calls, 35);
    }
}

fn execute_metrics() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    for (bytes, values) in [
        (metrics_executable::pe32(), [16, 32, 32, 16]),
        (metrics_executable::fullscreen(), [461, 640, 480, 19]),
    ] {
        let mut process = Process32::load(&bytes, 32).unwrap();
        let result = process.run(30);
        assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
        assert_eq!((result.instructions, result.api_calls), (12, 4));
        assert_eq!(process.cpu.register(Register32::Eax), values[0]);
        assert_eq!(process.cpu.register(Register32::Ebx), values[1]);
        assert_eq!(process.cpu.register(Register32::Ecx), values[2]);
        assert_eq!(process.cpu.register(Register32::Edx), values[3]);
        assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    }
    #[cfg(windows_demo)]
    {
        let mut process =
            Process32::load(include_bytes!("../../target/windows-api/metrics.exe"), 64).unwrap();
        let result = process.run(1000);
        assert_eq!(result.reason, ProcessStop::Exited(42));
        assert_eq!(result.api_calls, 19);
    }
}

fn execute_process_version() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&process_version_executable::pe32(9, 2), 32).unwrap();
    let result = process.run(10);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (3, 1));
    assert_eq!(process.cpu.register(Register32::Eax), 0x0009_0002);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    #[cfg(windows_demo)]
    {
        let mut process = Process32::load(
            include_bytes!("../../target/windows-api/process-version.exe"),
            64,
        )
        .unwrap();
        let result = process.run(1000);
        assert_eq!(result.reason, ProcessStop::Exited(42));
        assert_eq!(result.api_calls, 6);
    }
}

fn execute_string_moves() {
    use ring3_core::execution::{Cpu32, Register32, StopReason, load_pe32};
    let mut image = load_pe32(&string_moves_executable::pe32(), 3).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.eflags = 0xcad7;
    let result = cpu.run(&mut image.memory, 20);
    assert_eq!(result.reason, StopReason::Breakpoint);
    assert_eq!(result.instructions, 6);
    assert_eq!(cpu.register(Register32::Esi), 0x0040_2187);
    assert_eq!(cpu.register(Register32::Edi), 0x0040_21c7);
    assert_eq!(cpu.eflags, 0xcad7);
    let mut bytes = [0; 7];
    image.memory.read(0x0040_21c0, &mut bytes).unwrap();
    assert_eq!(bytes, [1, 2, 3, 4, 5, 6, 7]);
}

fn execute_condition_bytes() {
    use ring3_core::execution::{Cpu32, Register32, StopReason, load_pe32};
    let mut image = load_pe32(&condition_bytes_executable::pe32(), 3).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_fs_base(0x0040_2000);
    let result = cpu.run(&mut image.memory, 20);
    assert_eq!(result.reason, StopReason::Breakpoint);
    assert_eq!(result.instructions, 10);
    assert_eq!(cpu.register(Register32::Eax), 0xaabb_0100);
    assert_eq!(cpu.register(Register32::Ebx), 0x100);
    assert_eq!(cpu.register(Register32::Ecx), 0x100);
    assert_eq!(cpu.eflags, 0x46);
    let mut bytes = [0; 2];
    image.memory.read(0x0040_2180, &mut bytes).unwrap();
    assert_eq!(bytes, [0, 1]);
}

fn execute_zero_extend() {
    use ring3_core::execution::{Cpu32, Register32, StopReason, load_pe32};
    let mut image = load_pe32(&zero_extend_executable::pe32(), 3).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.eflags = 0xced7;
    cpu.set_fs_base(0x0040_2000);
    let result = cpu.run(&mut image.memory, 20);
    assert_eq!(result.reason, StopReason::Breakpoint);
    assert_eq!(result.instructions, 7);
    assert_eq!(cpu.register(Register32::Eax), 0xff);
    assert_eq!(cpu.register(Register32::Ebx), 0x1234_00ff);
    assert_eq!(cpu.register(Register32::Ecx), 0xff);
    assert_eq!(cpu.register(Register32::Edx), 0xff80);
    assert_eq!(cpu.eflags, 0xced7);
}

fn execute_clipboard_formats() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&clipboard_formats_executable::pe32(), 32).unwrap();
    let result = process.run(30);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (9, 3));
    assert_eq!(process.cpu.register(Register32::Eax), 0xc001);
    assert_eq!(process.cpu.register(Register32::Ebx), 0xc000);
    assert_eq!(process.cpu.register(Register32::Ecx), 0xc000);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    #[cfg(windows_demo)]
    {
        let mut process = Process32::load(
            include_bytes!("../../target/windows-api/clipboard-formats.exe"),
            64,
        )
        .unwrap();
        let result = process.run(1000);
        assert_eq!(result.reason, ProcessStop::Exited(42));
        assert_eq!(result.api_calls, 8);
    }
}

fn execute_messages() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&messages_executable::pe32(), 32).unwrap();
    let result = process.run(30);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (9, 3));
    assert_eq!(process.cpu.register(Register32::Eax), 0xc001);
    assert_eq!(process.cpu.register(Register32::Ebx), 0xc000);
    assert_eq!(process.cpu.register(Register32::Ecx), 0xc000);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    #[cfg(windows_demo)]
    {
        let mut process =
            Process32::load(include_bytes!("../../target/windows-api/messages.exe"), 64).unwrap();
        let result = process.run(1000);
        assert_eq!(result.reason, ProcessStop::Exited(42));
        assert_eq!(result.api_calls, 7);
    }
}

fn execute_cpinfo() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&cpinfo_executable::pe32(), 32).unwrap();
    let result = process.run(30);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (7, 2));
    assert_eq!(process.cpu.register(Register32::Eax), 1);
    assert_eq!(process.cpu.register(Register32::Ebx), 1);
    assert_eq!(process.cpu.register(Register32::Edx), 63);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    let mut bytes = [0; 18];
    process.memory.read(0x0040_2180, &mut bytes).unwrap();
    let mut expected = [0; 18];
    expected[0] = 1;
    expected[4] = b'?';
    assert_eq!(bytes, expected);
    #[cfg(windows_demo)]
    {
        let mut process =
            Process32::load(include_bytes!("../../target/windows-api/cpinfo.exe"), 64).unwrap();
        let result = process.run(1000);
        assert_eq!(result.reason, ProcessStop::Exited(42));
        assert_eq!(result.api_calls, 12);
    }
}

fn execute_code_pages() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&code_pages_executable::pe32(), 32).unwrap();
    let result = process.run(20);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (4, 2));
    assert_eq!(process.cpu.register(Register32::Eax), 437);
    assert_eq!(process.cpu.register(Register32::Ebx), 1252);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    #[cfg(windows_demo)]
    {
        let mut process = Process32::load(
            include_bytes!("../../target/windows-api/code-pages.exe"),
            64,
        )
        .unwrap();
        let result = process.run(1000);
        assert_eq!(result.reason, ProcessStop::Exited(42));
        assert_eq!(result.api_calls, 5);
    }
    #[cfg(windows_demo)]
    {
        let mut process = Process32::load(
            include_bytes!("../../target/windows-api/wide-character.exe"),
            64,
        )
        .unwrap();
        let result = process.run(1000);
        assert_eq!(result.reason, ProcessStop::Exited(42));
        assert_eq!((result.instructions, result.api_calls), (126, 8));
    }
}

fn execute_multibyte_to_wide() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let bytes = imported_executable::pe32(&[0xcc], "KERNEL32.dll", &["MultiByteToWideChar"]);
    let mut process = Process32::load(&bytes, 32).unwrap();
    process.memory.write(0x0040_2300, b"A\x80\0").unwrap();
    let frame: Vec<_> = [
        0x0040_1000_u32,
        0,
        0,
        0x0040_2300,
        u32::MAX,
        0x0040_2400,
        260,
    ]
    .into_iter()
    .flat_map(u32::to_le_bytes)
    .collect();
    process.memory.write(0x1000_ef00, &frame).unwrap();
    process.cpu.eip = 0x7000_033c;
    process.cpu.set_register(Register32::Esp, 0x1000_ef00);
    let result = process.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    assert_eq!(process.cpu.register(Register32::Eax), 3);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1000_ef1c);
    let mut output = [0; 6];
    process.memory.read(0x0040_2400, &mut output).unwrap();
    assert_eq!(output, [b'A', 0, 0xac, 0x20, 0, 0]);
}

fn execute_com_apartment() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let bytes =
        imported_executable::pe32(&[0xcc], "ole32.dll", &["CoInitialize", "CoUninitialize"]);
    let mut process = Process32::load(&bytes, 32).unwrap();
    let mut call = |api: u32, argument: Option<u32>| {
        let mut frame = 0x0040_1000_u32.to_le_bytes().to_vec();
        if let Some(argument) = argument {
            frame.extend_from_slice(&argument.to_le_bytes());
        }
        process.memory.write(0x1000_ef00, &frame).unwrap();
        process.cpu.eip = api;
        process.cpu.set_register(Register32::Esp, 0x1000_ef00);
        let result = process.run(1);
        assert_eq!(
            result.reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
        assert_eq!((result.instructions, result.api_calls), (0, 1));
        assert_eq!(
            process.cpu.register(Register32::Esp),
            0x1000_ef04 + u32::from(argument.is_some()) * 4
        );
        process.cpu.register(Register32::Eax)
    };
    assert_eq!(call(0x7000_0474, Some(0)), 0);
    assert_eq!(call(0x7000_0474, Some(0)), 1);
    assert_eq!(call(0x7000_0478, None), 1);
    assert_eq!(call(0x7000_0478, None), 1);
    assert_eq!(call(0x7000_0474, Some(0)), 0);
}

fn execute_borrow() {
    use ring3_core::execution::{Cpu32, Register32, StopReason, load_pe32};
    let mut image = load_pe32(&borrow_executable::pe32(), 3).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_fs_base(0x0040_2000);
    cpu.eflags = 0xced7;
    let result = cpu.run(&mut image.memory, 100);
    assert_eq!(result.reason, StopReason::Breakpoint);
    assert_eq!(result.instructions, 12);
    assert_eq!(cpu.register(Register32::Eax), 1);
    assert_eq!(cpu.register(Register32::Edx), 0);
    assert_eq!(cpu.register(Register32::Ebx), 0xffff_fdff);
    assert_eq!(cpu.eflags, (0xced7 & !0x8d5) | 0x44);
    let mut bytes = [0; 4];
    image.memory.read(0x0040_2000, &mut bytes).unwrap();
    assert_eq!(u32::from_le_bytes(bytes), 16);
}

fn execute_dllonexit() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&dllonexit_executable::pe32(), 26).unwrap();
    let stack = process.cpu.register(Register32::Esp);
    let pages = process.memory.mapped_pages();
    let result = process.run(100);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!(result.api_calls, 4);
    assert_eq!(process.cpu.register(Register32::Esp), stack);
    assert_eq!(process.cpu.register(Register32::Edx), 0);
    assert_eq!(process.cpu.register(Register32::Ebx), 21);
    assert_eq!(process.memory.mapped_pages(), pages);
    let mut bytes = [0; 4];
    process.memory.read(0x0040_2190, &mut bytes).unwrap();
    assert_eq!(u32::from_le_bytes(bytes), 21);
}

fn execute_crt_heap() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    for cpp in [false, true] {
        let mut process = Process32::load(&crt_heap_executable::pe32(cpp), 27).unwrap();
        let stack = process.cpu.register(Register32::Esp);
        let pages = process.memory.mapped_pages();
        let result = process.run(100);
        assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
        assert_eq!(result.api_calls, 3);
        assert_eq!(process.cpu.register(Register32::Esp), stack);
        assert_eq!(process.cpu.register(Register32::Eax), 123);
        assert_eq!(process.cpu.register(Register32::Ebx), 42);
        assert_eq!(process.memory.mapped_pages(), pages);
        assert!(
            process
                .memory
                .read(u64::from(process.cpu.register(Register32::Esi)), &mut [0])
                .is_err()
        );
    }
}

fn execute_exception_frame() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&exception_frame_executable::pe32(), 32).unwrap();
    let stack = process.cpu.register(Register32::Esp);
    process.cpu.set_register(Register32::Ebp, 0x8765_4321);
    process.cpu.eflags = 0xced7;
    let result = process.run(100);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!(result.api_calls, 2);
    assert_eq!(process.cpu.register(Register32::Esp), stack);
    assert_eq!(process.cpu.register(Register32::Ebp), 0x8765_4321);
    assert_eq!(process.cpu.register(Register32::Eax), 42);
    assert_eq!(process.cpu.eflags, 0xced7);
    let mut chain = [0; 4];
    process.memory.read(0x7ffd_e000, &mut chain).unwrap();
    assert_eq!(chain, [0xff; 4]);
    let mut data = [0; 24];
    process.memory.read(0x0040_2180, &mut data).unwrap();
    for (word, value) in data.chunks_exact(4).zip([
        stack - 20,
        stack - 20,
        stack - 40,
        stack - 20,
        0x2222_2222,
        u32::MAX,
    ]) {
        assert_eq!(word, value.to_le_bytes());
    }
}

fn execute_memset() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&memset_executable::pe32(), 32).unwrap();
    let stack = process.cpu.register(Register32::Esp);
    let result = process.run(100);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!(result.api_calls, 1);
    assert_eq!(process.cpu.register(Register32::Esp), stack);
    assert_eq!(process.cpu.register(Register32::Eax), 0x0040_2180);
    assert_eq!(process.cpu.register(Register32::Ebx), 0xa5a5_a5a5);
    let mut bytes = [0; 8];
    process.memory.read(0x0040_2180, &mut bytes).unwrap();
    assert_eq!(bytes, [0xa5; 8]);
}

fn execute_global_memory() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&global_memory_executable::pe32(), 27).unwrap();
    let pages = process.memory.mapped_pages();
    let result = process.run(100);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!(result.api_calls, 4);
    assert_eq!(process.cpu.register(Register32::Ebx), 42);
    assert_eq!(process.cpu.register(Register32::Eax), 0);
    assert_eq!(process.last_error().unwrap(), 0);
    assert_eq!(process.memory.mapped_pages(), pages);
    #[cfg(windows_demo)]
    {
        let mut process = Process32::load(
            include_bytes!("../../target/windows-api/global-memory.exe"),
            64,
        )
        .unwrap();
        let result = process.run(1000);
        assert_eq!(result.reason, ProcessStop::Exited(42));
        assert_eq!(result.api_calls, 21);
    }
}

fn execute_thread_state() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    fn call(process: &mut Process32, api: u32, args: &[u32]) -> u32 {
        let stack = 0x1000_ff00;
        process.cpu.eip = api;
        process.cpu.set_register(Register32::Esp, stack);
        for (index, value) in std::iter::once(&0x0040_1000_u32).chain(args).enumerate() {
            process
                .memory
                .write(u64::from(stack) + index as u64 * 4, &value.to_le_bytes())
                .unwrap();
        }
        assert_eq!(
            process.run(1).reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
        process.cpu.register(Register32::Eax)
    }
    let mut process = Process32::load(&suspended_thread_executable::pe32(), 64).unwrap();
    assert_eq!(
        process.run(100).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    let handle = process.cpu.register(Register32::Eax);
    assert_eq!(call(&mut process, 0x7000_0068, &[]), 0);
    assert_eq!(call(&mut process, 0x7000_0074, &[0, 111]), 1);
    process.cpu.set_fs_base(0x1101_0000);
    assert_eq!(call(&mut process, 0x7000_0070, &[0]), 0);
    process.cpu.set_fs_base(0x7ffd_e000);
    let mutex = call(&mut process, 0x7000_0210, &[0, 1, 0]);
    call(&mut process, 0x7000_0034, &[0x0040_2280]);
    call(&mut process, 0x7000_0038, &[0x0040_2280]);
    process.cpu.set_fs_base(0x1101_0000);
    process
        .memory
        .write(0x1101_0024, &1_u32.to_le_bytes())
        .unwrap();
    assert_eq!(call(&mut process, 0x7000_0214, &[mutex, 0]), 258);
    assert_eq!(call(&mut process, 0x7000_0218, &[mutex]), 0);
    assert_eq!(process.last_error().unwrap(), 288);
    assert_eq!(call(&mut process, 0x7000_003c, &[0x0040_2280]), 0);
    process.cpu.set_fs_base(0x7ffd_e000);
    assert_eq!(call(&mut process, 0x7000_0218, &[mutex]), 1);
    call(&mut process, 0x7000_0060, &[0x0040_2280]);
    process.cpu.set_fs_base(0x1101_0000);
    assert_eq!(call(&mut process, 0x7000_0214, &[mutex, 0]), 0);
    assert_eq!(call(&mut process, 0x7000_003c, &[0x0040_2280]), 1);
    let mut owner = [0; 4];
    process.memory.read(0x0040_228c, &mut owner).unwrap();
    assert_eq!(u32::from_le_bytes(owner), 2);
    assert_eq!(call(&mut process, 0x7000_0074, &[0, 222]), 1);
    assert_eq!(call(&mut process, 0x7000_021c, &[handle]), 1);
    assert_eq!(call(&mut process, 0x7000_0070, &[0]), 222);
    process.cpu.set_fs_base(0x7ffd_e000);
    assert_eq!(call(&mut process, 0x7000_0070, &[0]), 111);
    assert_eq!(call(&mut process, 0x7000_006c, &[0]), 1);
    assert_eq!(call(&mut process, 0x7000_0068, &[]), 0);
    process.cpu.set_fs_base(0x1101_0000);
    assert_eq!(call(&mut process, 0x7000_0070, &[0]), 0);
    process.cpu.set_fs_base(0x7ffd_e000);
    call(&mut process, 0x7000_0000, &[77]);
    process.cpu.set_fs_base(0x1101_0000);
    process.memory.write(0x0040_2300, b"absent\0").unwrap();
    for (api, args, error) in [
        (0x7000_0240, vec![0x0040_2300, 0, 0], 203),
        (0x7000_020c, vec![0, 0x0040_2320], 111),
        (0x7000_0090, vec![1252, 0], 87),
        (0x7000_00e4, vec![0x0040_2340, 0x5000_0000, 4], 87),
    ] {
        assert_eq!(call(&mut process, api, &args), 0);
        assert_eq!(process.last_error().unwrap(), error);
    }
    process.cpu.set_fs_base(0x7ffd_e000);
    assert_eq!(process.last_error().unwrap(), 77);
}

fn execute_tls() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&tls_executable::pe32(), 32).unwrap();
    let result = process.run(100);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!(result.api_calls, 6);
    assert_eq!(process.cpu.register(Register32::Esi), 0x1234_5678);
    assert_eq!(process.cpu.register(Register32::Eax), 0);
    assert_eq!(process.last_error().unwrap(), 0);
    #[cfg(windows_demo)]
    {
        let mut process =
            Process32::load(include_bytes!("../../target/windows-api/tls.exe"), 64).unwrap();
        let result = process.run(1000);
        assert_eq!(result.reason, ProcessStop::Exited(42));
        assert_eq!(result.api_calls, 14);
    }
}

fn execute_critical_sections() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&critical_section_executable::pe32(), 32).unwrap();
    let result = process.run(100);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!(result.api_calls, 8);
    assert_eq!(process.cpu.register(Register32::Eax), 1);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    let mut bytes = [1; 24];
    process.memory.read(0x0040_2180, &mut bytes).unwrap();
    assert_eq!(bytes, [0; 24]);
    #[cfg(windows_demo)]
    {
        let mut process = Process32::load(
            include_bytes!("../../target/windows-api/critical-sections.exe"),
            64,
        )
        .unwrap();
        let result = process.run(1000);
        assert_eq!(result.reason, ProcessStop::Exited(42));
        assert_eq!(result.api_calls, 11);
    }
}

fn execute_version() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let bytes = imported_executable::pe32(
        &[0xff, 0x15, 0x60, 0x20, 0x40, 0, 0xcc],
        "kernel32.dll",
        &["GetVersion"],
    );
    let mut process = Process32::load(&bytes, 32).unwrap();
    let result = process.run(10);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!(result.api_calls, 1);
    assert_eq!(process.cpu.register(Register32::Eax), 0x0a28_0105);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    #[cfg(windows_demo)]
    {
        let mut process =
            Process32::load(include_bytes!("../../target/windows-api/version.exe"), 64).unwrap();
        let result = process.run(1000);
        assert_eq!(result.reason, ProcessStop::Exited(42));
        assert_eq!(result.api_calls, 5);
    }
}

fn execute_heap() {
    use ring3_core::execution::{MemoryError, Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&heap_executable::pe32(), 27).unwrap();
    let result = process.run(100);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!(result.api_calls, 2);
    assert_eq!(process.cpu.register(Register32::Eax), 0);
    assert_eq!(process.cpu.register(Register32::Esi), 0x2000_0000);
    assert_eq!(process.memory.mapped_pages(), 25);
    assert_eq!(
        process.memory.read(0x2000_0000, &mut [0]),
        Err(MemoryError::Unmapped {
            address: 0x2000_0000
        })
    );
    #[cfg(windows_demo)]
    {
        let mut process =
            Process32::load(include_bytes!("../../target/windows-api/heap.exe"), 64).unwrap();
        let result = process.run(1000);
        assert_eq!(result.reason, ProcessStop::Exited(42));
        assert_eq!(result.api_calls, 8);
    }
}

fn execute_error_mode() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    for mode in [0, 1, 2, 4, 0x8000, 0x8007] {
        let mut process = Process32::load(&error_mode_executable::pe32(mode), 32).unwrap();
        let result = process.run(100);
        assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
        assert_eq!(result.api_calls, 4);
        assert_eq!(process.cpu.register(Register32::Ebx), 0);
        assert_eq!(process.cpu.register(Register32::Esi), mode & 0x8003);
        assert_eq!(process.cpu.register(Register32::Edi), mode & 0x8003);
        assert_eq!(process.cpu.register(Register32::Eax), 0);
    }
}

fn execute_resident_modules() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    for bytes in [modules_executable::pe32(), modules_executable::absolute()] {
        let mut process = Process32::load(&bytes, 32).unwrap();
        let mut calls = 0;
        for _ in 0..100 {
            let result = process.run(1);
            calls += result.api_calls;
            if result.reason == ProcessStop::Stopped(StopReason::Breakpoint) {
                break;
            }
            assert_eq!(
                result.reason,
                ProcessStop::Stopped(StopReason::InstructionLimit)
            );
        }
        assert_eq!(calls, 4);
        assert_eq!(
            process.cpu.register(Register32::Ebx),
            process.cpu.register(Register32::Esi)
        );
        assert_ne!(process.cpu.register(Register32::Ebx), 0);
        assert_eq!(process.cpu.register(Register32::Edi), 1);
        assert_eq!(process.cpu.register(Register32::Eax), 0);
        assert_eq!(process.last_error().unwrap(), 126);
        assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    }
    #[cfg(windows_demo)]
    {
        let mut process =
            Process32::load(include_bytes!("../../target/windows-api/modules.exe"), 64).unwrap();
        let result = process.run(1000);
        assert_eq!(result.reason, ProcessStop::Exited(42));
        assert_eq!(result.api_calls, 20);
    }
}

fn execute_dword_test() {
    use ring3_core::execution::{Cpu32, Permissions, Register32, StopReason, load_pe32};
    for (value, flags) in [(0_u32, 0x44), (1, 0), (3, 4), (0x8000_0000, 0x84)] {
        let code = [0x85, 0x1d, 0, 0x20, 0x40, 0, 0xcc];
        let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
        image
            .memory
            .write(0x0040_2000, &value.to_le_bytes())
            .unwrap();
        image
            .memory
            .protect(0x0040_2000, 4096, Permissions::READ)
            .unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        cpu.set_register(Register32::Ebx, u32::MAX);
        cpu.eflags = 0x8d7;
        assert_eq!(cpu.run(&mut image.memory, 2).reason, StopReason::Breakpoint);
        assert_eq!(cpu.register(Register32::Ebx), u32::MAX);
        assert_eq!(cpu.eflags & 0x8c5, flags);
        let mut unchanged = [0; 4];
        image.memory.read(0x0040_2000, &mut unchanged).unwrap();
        assert_eq!(unchanged, value.to_le_bytes());
    }
}

fn execute_narrow_operands() {
    use ring3_core::execution::{Cpu32, Register32, StopReason, load_pe32};
    for (input, output, add_flags, test_flags) in [
        (0x7fff_u32, 0x8000, 0x894, 0x80),
        (0xffff, 0, 0x55, 0x44),
        (0x02ff, 0x0300, 0x14, 4),
    ] {
        let code = [
            0x66, 0x05, 1, 0, 0x88, 0x25, 0xff, 0x2f, 0x40, 0, 0x84, 0xe4, 0xcc,
        ];
        let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        cpu.set_register(Register32::Eax, 0xa5a5_0000 | input);
        assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
        assert_eq!(cpu.eflags & 0x8d5, add_flags);
        assert_eq!(cpu.run(&mut image.memory, 3).reason, StopReason::Breakpoint);
        assert_eq!(cpu.register(Register32::Eax), 0xa5a5_0000 | output);
        assert_eq!(cpu.eflags & 0x8d5, test_flags);
        let mut byte = [0];
        image.memory.read(0x0040_2fff, &mut byte).unwrap();
        assert_eq!(u32::from(byte[0]), output >> 8);
    }
}

fn execute_increment() {
    use ring3_core::execution::{Cpu32, Register32, StopReason, load_pe32};
    for carry in [0, 1] {
        let code = [
            0x64, 0xfe, 0x00, 0x66, 0xb9, 3, 0, 0x66, 0x49, 0x75, 0xfc, 0xcc,
        ];
        let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        cpu.set_fs_base(0x0040_2000);
        cpu.eflags = 2 | carry;
        for _ in 0..8 {
            assert_eq!(
                cpu.run(&mut image.memory, 1).reason,
                StopReason::InstructionLimit
            );
        }
        assert_eq!(cpu.run(&mut image.memory, 1).reason, StopReason::Breakpoint);
        assert_eq!(cpu.register(Register32::Ecx), 0);
        assert_eq!(cpu.eflags & 0x8d5, 0x44 | carry);
        let mut value = [0; 4];
        image.memory.read(0x0040_2000, &mut value).unwrap();
        assert_eq!(value, [18, 0, 0, 0]);
    }
}

fn execute_and() {
    use ring3_core::execution::{Cpu32, Register32, StopReason, load_pe32};
    let code = [
        0x20, 0xc4, 0x66, 0x25, 0xf0, 0xff, 0x64, 0x83, 0x25, 0, 0, 0, 0, 0x80, 0xcc,
    ];
    let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
    image
        .memory
        .write(0x0040_2000, &u32::MAX.to_le_bytes())
        .unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_register(Register32::Eax, 0x1234_ff7f);
    cpu.set_fs_base(0x0040_2000);
    cpu.eflags = 0x8d7;
    assert_eq!(
        cpu.run(&mut image.memory, 10).reason,
        StopReason::Breakpoint
    );
    assert_eq!(cpu.register(Register32::Eax), 0x1234_7f70);
    assert_eq!(cpu.eflags & 0x8d5, 0x80);
    let mut bytes = [0; 4];
    image.memory.read(0x0040_2000, &mut bytes).unwrap();
    assert_eq!(bytes, 0xffff_ff80_u32.to_le_bytes());
}

fn execute_imul() {
    use ring3_core::execution::{Cpu32, PAGE_SIZE, Permissions, Register32, StopReason, load_pe32};
    for (left, right, expected, overflow) in [
        (0x8000_0000, u32::MAX, 0x8000_0000, true),
        (u32::MAX, 7, 0xffff_fff9, false),
        (0x7fff_ffff, 2, 0xffff_fffe, true),
    ] {
        let mut image = load_pe32(&executable::pe32(&[0x0f, 0xaf, 0xc3]), 3).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        cpu.set_register(Register32::Eax, left);
        cpu.set_register(Register32::Ebx, right);
        cpu.eflags = 0xced7;
        assert_eq!(
            cpu.run(&mut image.memory, 1).reason,
            StopReason::InstructionLimit
        );
        assert_eq!(cpu.register(Register32::Eax), expected);
        assert_eq!(
            cpu.eflags,
            (0xced7 & !0x801) | if overflow { 0x801 } else { 0 }
        );
    }
    let code = [
        0x69, 0xc9, 0xff, 0xff, 0xff, 0x1f, 0x64, 0x66, 0x6b, 0x05, 0, 0, 0, 0, 0xfd, 0xcc,
    ];
    let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
    image
        .memory
        .write(0x0040_2ffe, &0xfffe_u16.to_le_bytes())
        .unwrap();
    image
        .memory
        .protect(0x0040_2000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_register(Register32::Ecx, 8);
    cpu.set_register(Register32::Eax, 0xabcd_0000);
    cpu.set_fs_base(0x0040_2ffe);
    assert_eq!(
        cpu.run(&mut image.memory, 10).reason,
        StopReason::Breakpoint
    );
    assert_eq!(cpu.register(Register32::Ecx), 0xffff_fff8);
    assert_eq!(cpu.register(Register32::Eax), 0xabcd_0006);
    assert_eq!(cpu.eflags & 0x801, 0);
}

fn execute_shifts() {
    use ring3_core::execution::{Cpu32, Register32, StopReason, load_pe32};
    for (count, result, flags) in [
        (0, 0x8000_0001, 0x8d5),
        (32, 0x8000_0001, 0x8d5),
        (33, 2, 0x801),
        (255, 0x8000_0000, 0x84),
    ] {
        let mut image = load_pe32(&executable::pe32(&[0xd3, 0xe0]), 3).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        cpu.set_register(Register32::Eax, 0x8000_0001);
        cpu.set_register(Register32::Ecx, count);
        cpu.eflags = 0x8d7;
        assert_eq!(
            cpu.run(&mut image.memory, 1).reason,
            StopReason::InstructionLimit
        );
        assert_eq!(cpu.register(Register32::Eax), result);
        assert_eq!(cpu.eflags, flags | 2);
    }
    let code = [
        0xd0, 0xec, 0xd3, 0xe1, 0x64, 0x66, 0xc1, 0x3d, 0, 0, 0, 0, 3, 0xcc,
    ];
    let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
    image
        .memory
        .write(0x0040_2ffe, &0x8001_u16.to_le_bytes())
        .unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_register(Register32::Eax, 0x1234_817f);
    cpu.set_register(Register32::Ecx, 0x1000_0003);
    cpu.set_fs_base(0x0040_2ffe);
    for _ in 0..3 {
        assert_eq!(
            cpu.run(&mut image.memory, 1).reason,
            StopReason::InstructionLimit
        );
    }
    assert_eq!(cpu.run(&mut image.memory, 1).reason, StopReason::Breakpoint);
    assert_eq!(cpu.register(Register32::Eax), 0x1234_407f);
    assert_eq!(cpu.register(Register32::Ecx), 0x8000_0018);
    assert_eq!(cpu.eflags & 0x8d5, 0x84);
    let mut word = [0; 2];
    image.memory.read(0x0040_2ffe, &mut word).unwrap();
    assert_eq!(word, 0xf000_u16.to_le_bytes());
}

fn execute_conditional_branches() {
    use ring3_core::execution::{Cpu32, Register32, StopReason, load_pe32};
    for (left, right, less) in [
        (0x8000_0000_u32, 1_u32, true),
        (0x7fff_ffff, u32::MAX, false),
        (0, 0, false),
    ] {
        let code = [0x39, 0xd8, 0x0f, 0x8d, 5, 0, 0, 0, 0xb9, 42, 0, 0, 0, 0xcc];
        let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        cpu.set_register(Register32::Eax, left);
        cpu.set_register(Register32::Ebx, right);
        assert_eq!(
            cpu.run(&mut image.memory, 10).reason,
            StopReason::Breakpoint
        );
        assert_eq!(cpu.register(Register32::Ecx), if less { 42 } else { 0 });
    }
    let code = [0xb9, 0xfd, 0xff, 0xff, 0xff, 0x41, 0x7c, 0xfd, 0xcc];
    let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    assert_eq!(
        cpu.run(&mut image.memory, 20).reason,
        StopReason::Breakpoint
    );
    assert_eq!(cpu.register(Register32::Ecx), 0);
}

fn execute_delay_thunks() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    for legacy in [false, true] {
        let mut process = Process32::load(&delay_executable::pe32(legacy, false), 32).unwrap();
        let result = process.run(50);
        assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
        assert_eq!(process.cpu.register(Register32::Eax), 42);
        let mut word = [0; 4];
        process.memory.read(0x0040_2190, &mut word).unwrap();
        assert_eq!(u32::from_le_bytes(word), 1);
        process.memory.read(0x0040_2180, &mut word).unwrap();
        assert_eq!(u32::from_le_bytes(word), 0x0040_1080);
    }
}

fn execute_dlls() {
    use ring3_core::execution::{
        GuestModule, Process32, ProcessOptions, ProcessStop, Register32, StopReason,
    };
    let base = 0x5000_0000;
    for success in [true, false] {
        let library = dll_executable::dll(base, &dll_executable::attach(base, 41, success), None);
        let mut process = Process32::load_with_options(
            &dll_executable::exe("demo.dll"),
            32,
            ProcessOptions {
                modules: &[GuestModule {
                    name: "demo.dll",
                    bytes: &library,
                }],
                ..ProcessOptions::default()
            },
        )
        .unwrap();
        let result = process.run(100);
        let reason = if success {
            ProcessStop::Stopped(StopReason::Breakpoint)
        } else {
            ProcessStop::DllInitializationFailed {
                module: "demo.dll".to_owned(),
            }
        };
        assert_eq!(result.reason, reason);
        if success {
            assert_eq!(process.cpu.register(Register32::Eax), 42);
        } else {
            assert_eq!(process.run(100).reason, reason);
            assert_eq!(process.run(100).instructions, 0);
        }
    }
    #[cfg(guest_dll_demo)]
    {
        for library in [
            include_bytes!("../../target/guest-dll/demo.dll").as_slice(),
            include_bytes!("../../target/guest-dll/relocated/demo.dll").as_slice(),
        ] {
            let mut process = Process32::load_with_options(
                include_bytes!("../../target/guest-dll/caller.exe"),
                64,
                ProcessOptions {
                    modules: &[GuestModule {
                        name: "demo.dll",
                        bytes: library,
                    }],
                    ..ProcessOptions::default()
                },
            )
            .unwrap();
            let result = process.run(1000);
            assert_eq!(result.reason, ProcessStop::Exited(42));
            assert_eq!(result.api_calls, 2);
        }
        let mut delayed =
            Process32::load(include_bytes!("../../target/guest-dll/delayed.exe"), 64).unwrap();
        let result = delayed.run(1000);
        assert_eq!(result.reason, ProcessStop::Exited(42));
        assert_eq!(result.api_calls, 1);
    }
    for (library, base) in [
        (relocated_executable::dll(0x1000_0000), 0x3000_0000),
        (export_names_executable::dll(), 0x5000_0000),
    ] {
        let mut process = Process32::load_with_options(
            &dll_executable::exe("demo.dll"),
            128,
            ProcessOptions {
                modules: &[GuestModule {
                    name: "demo.dll",
                    bytes: &library,
                }],
                ..ProcessOptions::default()
            },
        )
        .unwrap();
        let result = process.run(100);
        assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
        assert_eq!(process.cpu.register(Register32::Eax), 42);
        assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
        let mut word = [0; 4];
        process
            .memory
            .read(u64::from(base + 0x2190), &mut word)
            .unwrap();
        assert_eq!(u32::from_le_bytes(word), base);
    }
}

fn execute_thread_notifications() {
    use ring3_core::execution::{
        GuestModule, Process32, ProcessOptions, ProcessStop, Register32, StopReason,
    };
    let library = thread_notifications_executable::dll();
    let mut process = Process32::load_with_options(
        &dll_executable::exe("demo.dll"),
        32,
        ProcessOptions {
            modules: &[GuestModule {
                name: "demo.dll",
                bytes: &library,
            }],
            ..ProcessOptions::default()
        },
    )
    .unwrap();
    let result = process.run(100);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!(result.api_calls, 1);
    assert_eq!(process.cpu.register(Register32::Eax), 1);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
}

fn execute_arguments() {
    use ring3_core::execution::{Process32, ProcessOptions, ProcessStop, Register32, StopReason};

    let mut process = Process32::load_with_options(
        &arguments_executable::pe32(0, 1),
        32,
        ProcessOptions {
            command_line: b"demo one \"two words\"",
            environment: &[b"A=B"],
            ..ProcessOptions::default()
        },
    )
    .unwrap();
    let result = process.run(100);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!(result.api_calls, 1);
    assert_eq!(process.crt_new_mode(), 1);
    assert_eq!(process.cpu.register(Register32::Eax), 0);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    let word = |address: u32| {
        let mut bytes = [0; 4];
        process.memory.read(u64::from(address), &mut bytes).unwrap();
        u32::from_le_bytes(bytes)
    };
    assert_eq!(word(0x0040_2304), 3);
    let argv = word(0x0040_2308);
    assert_eq!(word(argv + 12), 0);
    let mut last_argument = [0; 10];
    process
        .memory
        .read(u64::from(word(argv + 8)), &mut last_argument)
        .unwrap();
    assert_eq!(&last_argument, b"two words\0");
    let env = word(0x0040_230c);
    assert_eq!(word(env + 4), 0);
    let mut environment = [0; 4];
    process
        .memory
        .read(u64::from(word(env)), &mut environment)
        .unwrap();
    assert_eq!(&environment, b"A=B\0");
}

fn execute_initializers() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

    for seed in [0_u32, 13, u32::MAX] {
        let mut process = Process32::load(&initializer_executable::pe32(seed), 32).unwrap();
        let mut api_calls = 0;
        let mut finished = false;
        for _ in 0..500 {
            let step = process.run(1);
            assert_eq!(step.instructions + step.api_calls, 1);
            api_calls += step.api_calls;
            if step.reason == ProcessStop::Stopped(StopReason::Breakpoint) {
                finished = true;
                break;
            }
            assert_eq!(
                step.reason,
                ProcessStop::Stopped(StopReason::InstructionLimit)
            );
        }
        assert!(finished);
        assert_eq!(api_calls, 1);
        assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
        let mut value = [0; 4];
        process.memory.read(0x0040_21c0, &mut value).unwrap();
        assert_eq!(
            u32::from_le_bytes(value),
            seed.wrapping_add(8).wrapping_mul(2)
        );
    }
}

fn execute_fp_control() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

    for (initial, value, mask, word, flags) in [
        (
            0x027f_u16,
            0x0002_0300,
            0x0003_0300,
            0x0c7f_u16,
            0x000a_031f,
        ),
        (0x027f, 0, 0x0008_001f, 0x0242, 0x0009_0000),
        (0x027d, 0x0008_001f, 0x0008_001f, 0x027d, 0x0001_001f),
    ] {
        let bytes = fp_control_executable::pe32(initial, value, mask);
        let mut process = Process32::load(&bytes, 32).unwrap();
        let result = process.run(100);
        assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
        assert_eq!(result.api_calls, 2);
        assert_eq!(process.cpu.x87_control_word(), word);
        assert_eq!(process.cpu.register(Register32::Eax), flags);
        assert_eq!(process.cpu.register(Register32::Ebx), 0x0009_001f);
        assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
        let mut saved = [0; 2];
        process.memory.read(0x0040_2300, &mut saved).unwrap();
        assert_eq!(saved, 0x027f_u16.to_le_bytes());
        process.memory.read(0x0040_2308, &mut saved).unwrap();
        assert_eq!(saved, word.to_le_bytes());
    }
}

fn execute_crt() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

    for application_type in [1_u32, 2, u32::MAX] {
        let mut process = Process32::load(&crt_executable::pe32(application_type), 32).unwrap();
        let mut initial_fmode = [0; 4];
        process
            .memory
            .read(0x7000_2000, &mut initial_fmode)
            .unwrap();
        assert_eq!(u32::from_le_bytes(initial_fmode), 0x4000);
        let result = process.run(100);
        assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
        assert_eq!(result.api_calls, 3);
        assert_eq!(
            process.crt_application_type(),
            application_type.cast_signed()
        );
        assert_eq!(process.cpu.register(Register32::Ecx), application_type);
        assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
        assert_eq!(process.cpu.register(Register32::Edx), 7);
        assert_eq!(process.cpu.register(Register32::Ebx), 0);
        let mut globals = [0; 8];
        process.memory.read(0x7000_2000, &mut globals).unwrap();
        assert_eq!(&globals[..4], &35_u32.to_le_bytes());
        assert_eq!(&globals[4..], &7_u32.to_le_bytes());
    }
}

fn execute_thread() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

    for value in [42_u32, u32::MAX] {
        let mut process = Process32::load(&thread_executable::pe32(value), 32).unwrap();
        let result = process.run(100);
        assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
        assert_eq!(result.api_calls, 2);
        assert_eq!(process.cpu.register(Register32::Eax), value.wrapping_add(7));
        assert_eq!(process.cpu.register(Register32::Ebx), value);
        assert_eq!(process.cpu.register(Register32::Esi), 0x7ffd_e000);
        assert_eq!(process.cpu.register(Register32::Ecx), 0x1001_0000);
        assert_eq!(process.cpu.register(Register32::Edx), 0x1000_0000);
        assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
        assert_eq!(process.last_error().unwrap(), value.wrapping_add(7));
        let mut head = [0; 4];
        process.memory.read(0x7ffd_e000, &mut head).unwrap();
        assert_eq!(u32::from_le_bytes(head), u32::MAX);
    }
}

fn execute_diagnostic() {
    use ring3_core::execution::{LoadError, Process32, ProcessStop};

    for ordinal in [false, true] {
        let mut bytes = imported_executable::pe32(
            &[0xff, 0x15, 0x60, 0x20, 0x40, 0],
            "Missing.dll",
            &["Absent"],
        );
        bytes[0xd0..0xd4].copy_from_slice(&0x2180_u32.to_le_bytes());
        bytes[0x1a8..0x1ac].copy_from_slice(&0x180_u32.to_le_bytes());
        bytes[1028..1032].fill(0xff);
        bytes[1120..1124].fill(0xfe);
        if ordinal {
            bytes[1088..1092].copy_from_slice(&0x8000_0017_u32.to_le_bytes());
        }
        assert!(matches!(
            Process32::load(&bytes, 32),
            Err(LoadError::UnresolvedImport { .. })
        ));
        let mut process = Process32::load_diagnostic(&bytes, 32).unwrap();
        let result = process.run(100);
        assert_eq!(
            result.reason,
            ProcessStop::UnresolvedImport {
                address: 0x7100_0000,
                module: "Missing.dll".into(),
                symbol: if ordinal { "#23" } else { "Absent" }.into(),
            }
        );
        assert_eq!((result.instructions, result.api_calls), (1, 0));
        assert_eq!(process.run(100).instructions, 0);
    }
}

fn execute_graphics() {
    use ring3_core::execution::{Process32, ProcessStop, StopReason};

    for (width, height, color) in [(4, 3, 0xff12_3456_u32), (9, 7, 0xffed_cba9)] {
        let mut process =
            Process32::load(&d3d8_executable::pe32(width, height, color), 32).unwrap();
        let result = process.run(100);
        assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
        assert_eq!(result.api_calls, 12);
        let mut driver = [0; 6];
        process.memory.read(0x0040_2200, &mut driver).unwrap();
        assert_eq!(&driver, b"ring3\0");
        let mut caps = [0; 16];
        process.memory.read(0x0040_2800, &mut caps).unwrap();
        assert_eq!(u32::from_le_bytes(caps[..4].try_into().unwrap()), 1);
        assert_eq!(
            u32::from_le_bytes(caps[12..].try_into().unwrap()),
            0x0008_0000
        );
        let mut count = [0; 4];
        process.memory.read(0x0040_28d4, &mut count).unwrap();
        assert_eq!(u32::from_le_bytes(count), 1);
        let mut mode = [0; 16];
        process.memory.read(0x0040_28e0, &mut mode).unwrap();
        assert_eq!(u32::from_le_bytes(mode[..4].try_into().unwrap()), 640);
        assert_eq!(u32::from_le_bytes(mode[4..8].try_into().unwrap()), 480);
        assert_eq!(u32::from_le_bytes(mode[8..12].try_into().unwrap()), 0);
        assert_eq!(u32::from_le_bytes(mode[12..].try_into().unwrap()), 22);
        let mut status = [0; 4];
        process.memory.read(0x0040_28f0, &mut status).unwrap();
        assert_eq!(u32::from_le_bytes(status), 0);
        process.memory.read(0x0040_28f4, &mut status).unwrap();
        assert_eq!(u32::from_le_bytes(status), 0);
        process.memory.read(0x0040_28f8, &mut status).unwrap();
        assert_eq!(u32::from_le_bytes(status), 0);
        let frame = process.take_frame().unwrap();
        assert_eq!((frame.width, frame.height), (width, height));
        let [_, r, g, b] = color.to_be_bytes();
        assert!(
            frame
                .rgba
                .chunks_exact(4)
                .all(|pixel| pixel == [r, g, b, 255])
        );
    }
    #[cfg(windows_demo)]
    execute_compiled_graphics();
}

#[cfg(windows_demo)]
fn execute_compiled_graphics() {
    use ring3_core::execution::{Process32, ProcessStop};

    let mut process =
        Process32::load(include_bytes!("../../target/d3d8-frame/frame.exe"), 64).unwrap();
    let result = process.run(100_000);
    assert_eq!(result.reason, ProcessStop::Exited(0));
    assert_eq!(result.api_calls, 36);
    let frame = process.take_frame().unwrap();
    assert_eq!((frame.width, frame.height), (320, 200));
    let pixel = |x: usize, y: usize| {
        let offset = (y * frame.width as usize + x) * 4;
        &frame.rgba[offset..offset + 4]
    };
    assert_eq!(pixel(45, 45), &[0, 255, 0, 255]);
    assert_eq!(pixel(100, 50), &[0xe8, 0x6c, 0x42, 255]);
    assert_eq!(pixel(0, 0), &[0x10, 0x20, 0x30, 255]);
}

#[cfg(windows_demo)]
fn execute_compiled_rtti() {
    use ring3_core::execution::{Process32, ProcessStop};

    let mut process =
        Process32::load(include_bytes!("../../target/crt-rtti/casts.exe"), 64).unwrap();
    let result = process.run(1_000);
    assert_eq!(result.reason, ProcessStop::Exited(0));
    assert_eq!(result.api_calls, 4);
}

fn execute_windows_api() {
    use ring3_core::execution::{Process32, ProcessStop};

    for value in [42_u32, 123, u32::MAX] {
        let mut code = vec![0x68];
        code.extend_from_slice(&value.to_le_bytes());
        code.extend_from_slice(&[
            0xff, 0x15, 0x60, 0x20, 0x40, 0, 0xff, 0x15, 0x64, 0x20, 0x40, 0, 0x50, 0xff, 0x15,
            0x68, 0x20, 0x40, 0, 0x0f, 0x0b,
        ]);
        let bytes = imported_executable::pe32(
            &code,
            "KERNEL32.dll",
            &["SetLastError", "GetLastError", "ExitProcess"],
        );
        let mut process = Process32::load(&bytes, 32).unwrap();
        let result = process.run(100);
        assert_eq!(result.reason, ProcessStop::Exited(value));
        assert_eq!(result.api_calls, 3);
        assert_eq!(process.last_error().unwrap(), value);
        assert_eq!(process.run(100).instructions, 0);
    }
}

fn execute_image() {
    use ring3_core::execution::{Cpu32, Register32, StopReason, load_pe32};

    for (left, right) in [(7_u32, 35_u32), (100, 23), (u32::MAX, 1)] {
        let mut code = vec![0xb8];
        code.extend_from_slice(&left.to_le_bytes());
        code.push(0x05);
        code.extend_from_slice(&right.to_le_bytes());
        code.push(0xcc);
        let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        let result = cpu.run(&mut image.memory, 3);
        assert_eq!(result.reason, StopReason::Breakpoint);
        assert_eq!(result.instructions, 3);
        assert_eq!(cpu.register(Register32::Eax), left.wrapping_add(right));
    }
    let mut image = load_pe32(&executable::pe32(&[0xeb, 0xfe]), 3).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    assert_eq!(
        cpu.run(&mut image.memory, 20).reason,
        StopReason::InstructionLimit
    );
    assert_eq!(cpu.eip, image.entry_point);
}

fn execute_function() {
    use ring3_core::execution::{Cpu32, Permissions, Register32, StopReason, load_pe32};

    let code = [
        0x6a, 35, 0x6a, 7, 0xe8, 9, 0, 0, 0, 0x83, 0xc4, 8, 0xa3, 0, 0x20, 0x40, 0, 0xcc, 0x55,
        0x89, 0xe5, 0x8b, 0x45, 8, 0x03, 0x45, 12, 0xc9, 0xc3,
    ];
    let mut image = load_pe32(&executable::pe32(&code), 4).unwrap();
    image
        .memory
        .map_zeroed(0x1000_0000, 4096, Permissions::READ_WRITE)
        .unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_register(Register32::Esp, 0x1000_1000);
    assert_eq!(
        cpu.run(&mut image.memory, 100).reason,
        StopReason::Breakpoint
    );
    assert_eq!(cpu.register(Register32::Eax), 42);
    assert_eq!(cpu.register(Register32::Esp), 0x1000_1000);
    let mut result = [0; 4];
    image.memory.read(0x0040_2000, &mut result).unwrap();
    assert_eq!(u32::from_le_bytes(result), 42);
}

fn fixture() -> [u8; 528] {
    let mut bytes = [0; 528];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    for (offset, value) in [
        (0x84, 0x8664_u16),
        (0x86, 1),
        (0x94, 240),
        (0x96, 0x22),
        (0x98, 0x20b),
        (0xdc, 3),
    ] {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    for (offset, value) in [
        (0x3c, 0x80_u32),
        (0xa8, 0x1000),
        (0xac, 0x1000),
        (0xb8, 0x1000),
        (0xbc, 0x200),
        (0xd0, 0x2000),
        (0xd4, 0x200),
        (0x104, 16),
        (0x190, 16),
        (0x194, 0x1000),
        (0x198, 16),
        (0x19c, 0x200),
        (0x1ac, 0x4000_0040),
    ] {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes[0xb0..0xb8].copy_from_slice(&0x1_4000_0000_u64.to_le_bytes());
    bytes[0x188..0x18d].copy_from_slice(b".test");
    for (slot, value) in bytes[512..].iter_mut().zip(0x10_u8..0x20) {
        *slot = value;
    }
    bytes
}

fn inspect_image() {
    let mut bytes = fixture();
    let original = bytes;
    let headers = parse_pe_headers(&bytes).unwrap();
    assert_eq!(
        headers.prefix,
        PeHeaderPrefix {
            pe_offset: FileOffset::new(0x80),
            machine: 0x8664,
            number_of_sections: 1,
            characteristics: 0x22,
            size_of_optional_header: 240,
            kind: PeKind::Pe32Plus,
        }
    );
    assert_eq!(headers.optional.image_base, 0x1_4000_0000);
    assert_eq!(
        resolve_pe_file_range(&bytes, RelativeVirtualAddress::new(0x1003), 4),
        Ok(PeFileRange {
            file_offset: FileOffset::new(0x203),
            bytes: &[0x13, 0x14, 0x15, 0x16],
            source: PeFileRangeSource::Section(0),
        })
    );
    assert_eq!(
        resolve_pe_file_range(&bytes, RelativeVirtualAddress::new(0x100f), 2),
        Err(PeRvaError::CrossesRegionBoundary {
            start: RelativeVirtualAddress::new(0x100f),
            length: 2,
        })
    );
    assert_eq!(
        fingerprint_pe_declared_evidence(&bytes, 527),
        Err(PeFingerprintError::InputTooLarge {
            length: 528,
            limit: 527,
        })
    );
    let fingerprint = fingerprint_pe_declared_evidence(&bytes, 528).unwrap();
    assert_eq!(
        fingerprint_pe_declared_evidence(&bytes, 528),
        Ok(fingerprint)
    );
    assert_eq!(bytes, original);
    bytes.fill(0);
    assert_eq!(fingerprint.byte_length, 528);
    assert_eq!(
        fingerprint.digest,
        [
            0x56, 0x28, 0x84, 0x9c, 0x7d, 0x69, 0xb0, 0xec, 0xcb, 0x42, 0x67, 0x03, 0xc5, 0x75,
            0x5e, 0xdd, 0xf1, 0x46, 0x97, 0x09, 0x3b, 0x20, 0x91, 0xa4, 0x2c, 0x79, 0x17, 0x07,
            0xc2, 0xe7, 0xd5, 0xd1,
        ]
    );
}

fn inspect_paths() {
    let limits = AsciiSourcePathLimits {
        max_paths: 1,
        max_path_bytes: 12,
        max_total_path_bytes: 12,
        max_depth: 2,
    };
    let paths = {
        let input = String::from(r"Bin\GAME.exe");
        admit_ascii_source_paths(&[&input], limits).unwrap()
    };
    assert_eq!(
        paths,
        AsciiSourcePathBatch {
            total_path_bytes: 12,
            entries: vec![AsciiSourcePathEntry {
                index: 0,
                normalized: "Bin/GAME.exe".into(),
                key: "bin/game.exe".into(),
                depth: 2,
            }],
        }
    );
    assert_eq!(
        admit_ascii_source_paths(&["../GAME.exe"], limits),
        Err(AsciiSourcePathError::Segment {
            index: 0,
            segment: 0,
            reason: AsciiSourcePathSegmentError::Parent,
        })
    );
}

fn inspect_path_collisions() {
    let limits = AsciiSourcePathLimits {
        max_paths: 3,
        max_path_bytes: 12,
        max_total_path_bytes: 36,
        max_depth: 2,
    };
    for (paths, index, prior, kind) in [
        (vec!["Data/A", r"data\a"], 1, 0, Collision::Duplicate),
        (vec!["a", "a/b"], 1, 0, Collision::AncestorFile),
        (vec!["a/b", "a"], 1, 0, Collision::DescendantFile),
        (vec!["a!", "a/b", "a"], 2, 1, Collision::DescendantFile),
        (vec!["a/z", "a/b", "a"], 2, 1, Collision::DescendantFile),
    ] {
        assert_eq!(
            admit_ascii_source_paths(&paths, limits),
            Err(AsciiSourcePathError::Collision { index, prior, kind })
        );
    }
    assert_eq!(
        admit_ascii_source_paths(&["ab/file", "a"], limits),
        Ok(AsciiSourcePathBatch {
            total_path_bytes: 8,
            entries: vec![
                AsciiSourcePathEntry {
                    index: 0,
                    normalized: "ab/file".into(),
                    key: "ab/file".into(),
                    depth: 2,
                },
                AsciiSourcePathEntry {
                    index: 1,
                    normalized: "a".into(),
                    key: "a".into(),
                    depth: 1,
                },
            ],
        })
    );
}

fn import_fixture(name: &[u8; 6]) -> [u8; 528] {
    let mut bytes = fixture();
    for (offset, value) in [(0x110, 0x40_u32), (0x114, 40), (0x4c, 0x70)] {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes[0x70..0x76].copy_from_slice(name);
    bytes
}

fn inspect_dependency_cycle() {
    use ring3_core::{
        AsciiPeDependencyClosure, AsciiPeDependencyClosureError, AsciiPeDependencyClosureLimits,
        AsciiPeModuleDependencyEvidence, AsciiPeModuleDependencyLimits,
        AsciiPeModuleDependencyRequest, AsciiPeSource, AsciiPeSourceModuleEvidenceLimits,
        PeDependencyClosureMode, PeDependencyRequestStep, PeDependencyRequestVisit,
        PeDependencyVisit, PeHeaderBatchLimits, PeImportLookupError, PeModuleDependencyKind,
        PeModuleDependencyViews, PeModuleOutputLimits, PeOwnedImportDescriptor,
        PeStaticImportEvidence, inspect_ascii_pe_source_module_evidence,
        walk_ascii_pe_dependency_closure,
    };
    let paths = ["game/main.exe", "GAME/A.dll", "game/B.dll"];
    let names = ["A.dll", "B.dll", "A.dll"];
    let path_limits = AsciiSourcePathLimits {
        max_paths: 3,
        max_path_bytes: 13,
        max_total_path_bytes: 33,
        max_depth: 2,
    };
    let batch = {
        let mut bytes = [
            import_fixture(b"A.dll\0"),
            import_fixture(b"B.dll\0"),
            import_fixture(b"A.dll\0"),
        ];
        let sources = std::array::from_fn::<_, 3, _>(|i| AsciiPeSource {
            path: paths[i],
            bytes: &bytes[i],
        });
        let family = PeModuleOutputLimits {
            max_rows: 1,
            max_text_bytes: 5,
        };
        let batch = inspect_ascii_pe_source_module_evidence(
            &sources,
            AsciiPeSourceModuleEvidenceLimits {
                paths: path_limits,
                content: PeHeaderBatchLimits {
                    max_files: 3,
                    max_file_bytes: 528,
                    max_total_bytes: 1584,
                },
                static_imports: family,
                delay_imports: family,
                exports: family,
                bound_imports: family,
            },
        )
        .unwrap();
        for input in &mut bytes {
            input.fill(0);
        }
        batch
    };
    let digests = [
        [
            0xfc, 0x6a, 0x18, 0xb5, 0x92, 0xf4, 0x5a, 0xda, 0xe4, 0xfe, 0x55, 0x8e, 0xc9, 0x8c,
            0x8a, 0x0f, 0x90, 0x50, 0x56, 0x43, 0x87, 0xaa, 0xa9, 0xbd, 0xb2, 0xab, 0xfd, 0x5a,
            0x4b, 0x39, 0x4c, 0x5d,
        ],
        [
            0x4b, 0x4c, 0x4e, 0x22, 0x72, 0x49, 0x88, 0x51, 0xad, 0x6e, 0x31, 0xc4, 0xd3, 0x63,
            0xc4, 0x3f, 0x35, 0x5b, 0xd1, 0xf7, 0x75, 0x94, 0x76, 0xd4, 0x20, 0x31, 0x4e, 0xe8,
            0xaf, 0x2e, 0x5c, 0x8f,
        ],
    ];
    assert_eq!(batch.total_path_bytes, 33);
    assert_eq!(batch.total_content_bytes, 1584);
    assert_eq!(batch.entries.len(), 3);
    for (index, entry) in batch.entries.iter().enumerate() {
        let module = entry.module.as_ref().unwrap();
        assert_eq!(module.fingerprinted.byte_length, 528);
        assert_eq!(
            module.fingerprinted.digest,
            digests[usize::from(index == 1)]
        );
        assert_eq!(
            module.static_imports,
            Ok(PeStaticImportEvidence {
                total_rows: 1,
                total_text_bytes: 5,
                descriptors: Ok(vec![PeOwnedImportDescriptor {
                    descriptor_rva: RelativeVirtualAddress::new(64),
                    descriptor_file_offset: FileOffset::new(64),
                    import_lookup_table_rva: RelativeVirtualAddress::new(0),
                    time_date_stamp: 0,
                    forwarder_chain: 0,
                    name_rva: RelativeVirtualAddress::new(112),
                    import_address_table_rva: RelativeVirtualAddress::new(0),
                    dll_name: names[index].into(),
                }]),
                lookups: Err(PeImportLookupError::LookupTableUnavailable {
                    descriptor_index: 0
                }),
            })
        );
    }
    let limits = AsciiPeDependencyClosureLimits {
        observation: AsciiPeModuleDependencyLimits {
            paths: path_limits,
            max_requests: 3,
            max_request_text_bytes: 15,
            max_basename_bytes: 5,
        },
        max_reached_sources: 3,
        max_examined_requests: 3,
    };
    let before = batch.clone();
    let mut retained = Vec::new();
    for mode in [
        PeDependencyClosureMode::StaticOnly,
        PeDependencyClosureMode::StaticAndDelay,
    ] {
        let expected = AsciiPeDependencyClosure {
            observations: AsciiPeModuleDependencyEvidence {
                paths: AsciiSourcePathBatch {
                    total_path_bytes: 33,
                    entries: paths
                        .iter()
                        .enumerate()
                        .map(|(index, path)| AsciiSourcePathEntry {
                            index,
                            normalized: (*path).into(),
                            key: path.to_ascii_lowercase(),
                            depth: 2,
                        })
                        .collect(),
                },
                application_source_index: 0,
                total_requests: 3,
                total_request_text_bytes: 15,
                sources: vec![
                    Ok(PeModuleDependencyViews {
                        static_imports: Ok(1),
                        delay_imports: Ok(None),
                    });
                    3
                ],
                requests: [1, 2, 1]
                    .into_iter()
                    .enumerate()
                    .map(|(source_index, target)| AsciiPeModuleDependencyRequest {
                        source_index,
                        kind: PeModuleDependencyKind::Static,
                        descriptor_index: 0,
                        dll_name: names[source_index].into(),
                        candidate: Ok(Some(target)),
                    })
                    .collect(),
            },
            mode,
            visits: vec![
                PeDependencyVisit {
                    source_index: 0,
                    via_request_index: None,
                },
                PeDependencyVisit {
                    source_index: 1,
                    via_request_index: Some(0),
                },
                PeDependencyVisit {
                    source_index: 2,
                    via_request_index: Some(1),
                },
            ],
            examined_requests: vec![
                PeDependencyRequestVisit {
                    request_index: 0,
                    step: PeDependencyRequestStep::Discovered { visit_index: 1 },
                },
                PeDependencyRequestVisit {
                    request_index: 1,
                    step: PeDependencyRequestStep::Discovered { visit_index: 2 },
                },
                PeDependencyRequestVisit {
                    request_index: 2,
                    step: PeDependencyRequestStep::AlreadyReached { visit_index: 1 },
                },
            ],
        };
        assert_eq!(
            walk_ascii_pe_dependency_closure(
                &batch,
                0,
                mode,
                AsciiPeDependencyClosureLimits {
                    max_examined_requests: 2,
                    ..limits
                }
            ),
            Err(AsciiPeDependencyClosureError::ExaminedRequestsExceeded {
                source_index: 2,
                request_index: 2,
                count: 3,
                limit: 2,
            })
        );
        let actual = walk_ascii_pe_dependency_closure(&batch, 0, mode, limits).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(
            walk_ascii_pe_dependency_closure(&batch, 0, mode, limits),
            Ok(expected.clone())
        );
        retained.push((actual, expected));
    }
    assert_eq!(batch, before);
    drop(batch);
    drop(before);
    for (actual, expected) in retained {
        assert_eq!(actual, expected);
    }
}

#[path = "../../core/tests/support/onexit_executable.rs"]
mod onexit_executable;

fn execute_onexit() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&onexit_executable::pe32(), 25).unwrap();
    let result = process.run(100);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (8, 2));
    assert_eq!(process.cpu.register(Register32::Eax), 0x0040_1090);
    assert_eq!(process.cpu.register(Register32::Ebx), 0x0040_1080);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    let mut trace = [0; 4];
    process.memory.read(0x0040_2180, &mut trace).unwrap();
    assert_eq!(trace, [0; 4]);
}

#[path = "../../core/tests/support/crt_code_page_executable.rs"]
mod crt_code_page_executable;

fn execute_crt_code_page() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&crt_code_page_executable::pe32(), 25).unwrap();
    let result = process.run(100);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (13, 3));
    assert_eq!(process.cpu.register(Register32::Eax), 0x0040_2182);
    assert_eq!(process.cpu.register(Register32::Ebx), 0);
    assert_eq!(process.cpu.register(Register32::Esi), 0x0040_2181);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
}

#[path = "../../core/tests/support/reverse_search_executable.rs"]
mod reverse_search_executable;

fn execute_reverse_search() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&reverse_search_executable::pe32(), 25).unwrap();
    let result = process.run(50);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (10, 2));
    assert_eq!(process.cpu.register(Register32::Eax), 0x0040_218b);
    assert_eq!(process.cpu.register(Register32::Ebx), 0x0040_2187);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    let mut bytes = [0; 12];
    process.memory.read(0x0040_2180, &mut bytes).unwrap();
    assert_eq!(&bytes, b"one.two.ext\0");
}

#[path = "../../core/tests/support/string_traversal_executable.rs"]
mod string_traversal_executable;

fn execute_string_traversal() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    for (bytes, instructions, calls, eax, ebx, expected_output) in [
        (
            string_traversal_executable::increment(),
            8,
            2,
            0x0040_2182,
            0x0040_2181,
            [0; 8],
        ),
        (
            string_traversal_executable::copy(),
            6,
            1,
            0x0040_2190,
            0x0063_6261,
            [b'a', b'b', b'c', 0, 0x55, 0x55, 0x55, 0x55],
        ),
        (
            string_traversal_executable::terminated_copy(),
            5,
            1,
            0x0040_2190,
            0x6463_6261,
            [b'a', b'b', b'c', b'd', b'e', b'f', 0, 0x55],
        ),
        (
            string_traversal_executable::append(),
            5,
            1,
            0x0040_2190,
            0x6463_6261,
            [b'a', b'b', b'c', b'd', 0, 0x55, 0x55, 0x55],
        ),
    ] {
        let mut process = Process32::load(&bytes, 25).unwrap();
        let result = process.run(50);
        assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
        assert_eq!(
            (result.instructions, result.api_calls),
            (instructions, calls)
        );
        assert_eq!(process.cpu.register(Register32::Eax), eax);
        assert_eq!(process.cpu.register(Register32::Ebx), ebx);
        assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
        let mut output = [0; 8];
        process.memory.read(0x0040_2190, &mut output).unwrap();
        assert_eq!(output, expected_output);
    }
    #[cfg(windows_demo)]
    {
        let mut process = Process32::load(
            include_bytes!("../../target/windows-api/string-copy.exe"),
            64,
        )
        .unwrap();
        let result = process.run(1000);
        assert_eq!(result.reason, ProcessStop::Exited(42));
        assert_eq!((result.instructions, result.api_calls), (141, 16));
    }
}

#[path = "../../core/tests/support/strdup_executable.rs"]
mod strdup_executable;

#[path = "../../core/tests/support/buffer_compare_executable.rs"]
mod buffer_compare_executable;

fn execute_buffer_compare() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&buffer_compare_executable::pe32(), 25).unwrap();
    let result = process.run(50);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (12, 2));
    assert_eq!(process.cpu.register(Register32::Eax), u32::MAX);
    assert_eq!(process.cpu.register(Register32::Ebx), 1);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    let mut bytes = [0; 3];
    process.memory.read(0x0040_2180, &mut bytes).unwrap();
    assert_eq!(&bytes, b"a\0\x80");
}

fn execute_strdup() {
    use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};
    let mut process = Process32::load(&strdup_executable::pe32(), 26).unwrap();
    let pages = process.memory.mapped_pages();
    let result = process.run(50);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (11, 2));
    assert_eq!(process.cpu.register(Register32::Eax), 0x2000_0000);
    assert_eq!(process.cpu.register(Register32::Ecx), 0x2000_0000);
    assert_eq!(process.cpu.register(Register32::Ebx), 0x0063_6261);
    assert_eq!(process.cpu.register(Register32::Edx), 0x0063_622a);
    assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    assert_eq!(process.memory.mapped_pages(), pages);
    assert!(process.memory.read(0x2000_0000, &mut [0]).is_err());
    let mut source = [0; 4];
    process.memory.read(0x0040_2180, &mut source).unwrap();
    assert_eq!(&source, b"abc\0");
}

// this isolated test cdylib owns its unique zero-argument export.
#[unsafe(no_mangle)]
pub extern "C" fn run() -> u32 {
    execute_accumulator_sign_extension();
    execute_graphics();
    #[cfg(windows_demo)]
    execute_compiled_rtti();
    execute_thread();
    execute_crt();
    execute_fp_control();
    execute_initializers();
    execute_arguments();
    execute_dlls();
    execute_thread_notifications();
    execute_delay_thunks();
    execute_dword_test();
    execute_narrow_operands();
    execute_increment();
    execute_and();
    execute_conditional_branches();
    execute_shifts();
    execute_imul();
    execute_diagnostic();
    execute_windows_api();
    execute_resident_modules();
    execute_error_mode();
    execute_heap();
    execute_version();
    execute_critical_sections();
    execute_tls();
    execute_thread_state();
    execute_global_memory();
    execute_memset();
    execute_reverse_search();
    execute_crt_code_page();
    execute_onexit();
    execute_string_traversal();
    execute_strdup();
    execute_buffer_compare();
    execute_exception_frame();
    execute_crt_heap();
    execute_dllonexit();
    execute_borrow();
    execute_code_pages();
    execute_multibyte_to_wide();
    execute_com_apartment();
    execute_cpinfo();
    execute_messages();
    execute_clipboard_formats();
    execute_zero_extend();
    execute_condition_bytes();
    execute_string_moves();
    execute_process_version();
    execute_metrics();
    execute_colors();
    execute_gdi();
    execute_brushes();
    execute_cursors();
    execute_cursor_position();
    execute_local_realloc();
    execute_interlocked();
    execute_locale_activity();
    execute_locale_metadata();
    execute_string_length();
    execute_string_append();
    execute_buffer_copy();
    execute_string_copy();
    execute_character_search();
    execute_complement();
    execute_signed_extension();
    execute_string_scan();
    execute_string_comparisons();
    execute_repeated_moves();
    execute_string_stores();
    execute_register_stack();
    execute_flag_stack();
    execute_cpuid();
    execute_thread_identity();
    execute_alternate_teb();
    execute_module_file_name();
    execute_resources();
    execute_accelerators();
    execute_system_directory();
    execute_computer_name();
    execute_mutex_lifecycle();
    execute_event_lifecycle();
    execute_suspended_thread();
    execute_performance_clock();
    execute_current_directory();
    execute_change_directory();
    execute_find_files();
    execute_file_status();
    execute_environment_query();
    execute_startup_information();
    execute_hook_registration();
    execute_procedure_lookup();
    execute_registry_keys();
    execute_registry_values();
    execute_windows_string_length();
    execute_file_attributes();
    execute_short_path();
    execute_registry_defaults();
    execute_x87_register_stores();
    execute_thread_priority();
    execute_windows_formatting();
    execute_buffer_move();
    execute_window_classes();
    execute_window_procedures();
    execute_window_creation();
    execute_window_properties();
    execute_hook_chain();
    execute_icons();
    execute_window_messages();
    #[cfg(windows_demo)]
    execute_get_message_wait();
    #[cfg(windows_demo)]
    execute_translate_message();
    #[cfg(windows_demo)]
    execute_dispatch_message();
    #[cfg(windows_demo)]
    execute_dialog_cbt();
    execute_path_components();
    execute_file_streams();
    execute_desktop_queries();
    execute_x87_data();
    execute_x87_scaling();
    execute_fpu_wait();
    execute_integer_stores();
    execute_millisecond_clock();
    execute_crt_random();
    execute_crt_float_to_integer();
    execute_crt_type_names();
    execute_crt_string_prefix();
    execute_crt_case_comparison();
    execute_crt_formatting();
    execute_crt_scanning();
    execute_crt_lowercase();
    execute_argument_pointers();
    execute_file_removal();
    execute_x87_status();
    execute_x87_division();
    execute_x87_memory_sum();
    execute_x87_register_add();
    execute_x87_divide_pop();
    execute_wide_product();
    execute_unsigned_division();
    execute_command_line();
    execute_image();
    execute_function();
    inspect_image();
    inspect_paths();
    inspect_path_collisions();
    inspect_dependency_cycle();
    0x5233_0001
}
