#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_open")]
pub extern "C" fn open(pages: u32, low: u32, high: u32) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::open(pages, low, high)
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_close")]
pub extern "C" fn close() -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::close()
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_arena_ptr")]
pub extern "C" fn arena_ptr() -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::arena_ptr()
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_map")]
pub extern "C" fn map(address: u32, pages: u32, permissions: u32) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::map(address, pages, permissions)
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_protect")]
pub extern "C" fn protect(address: u32, pages: u32, permissions: u32) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::protect(address, pages, permissions)
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_unmap")]
pub extern "C" fn unmap(address: u32, pages: u32) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::unmap(address, pages)
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_upload")]
pub extern "C" fn upload(address: u32, length: u32) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::upload(address, length)
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_compile")]
pub extern "C" fn compile(count: u32) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::compile(count)
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_generation")]
pub extern "C" fn generation() -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::generation()
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_compile_with_gates")]
pub extern "C" fn compile_with_gates(count: u32, gate_count: u32) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::compile_with_gates(count, gate_count)
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_module_ptr")]
pub extern "C" fn module_ptr() -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::module_ptr()
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_module_len")]
pub extern "C" fn module_len() -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::module_len()
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_guard")]
pub extern "C" fn guard(
    low: u32,
    high: u32,
    generation: u32,
    state: u32,
    exit: u32,
    cancel: u32,
) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::guard(low, high, generation, state, exit, cancel)
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_read32")]
pub extern "C" fn read32(address: u32) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::read32(address)
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_write32")]
pub extern "C" fn write32(address: u32, value: u32) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::write32(address, value)
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_write_words32")]
pub extern "C" fn write_words32(count: u32) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::write_words32(count)
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_store32")]
pub extern "C" fn store32(address: u32, value: u32) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::store32(address, value)
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_capture_call")]
pub extern "C" fn capture_call(
    low: u32,
    high: u32,
    generation: u32,
    convention: u32,
    words: u32,
) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::capture_call(low, high, generation, convention, words)
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_complete_call")]
pub extern "C" fn complete_call(
    low: u32,
    high: u32,
    generation: u32,
    token: u32,
    result: u32,
) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::complete_call(low, high, generation, token, result)
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_abandon_call")]
pub extern "C" fn abandon_call(low: u32, high: u32, token: u32) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::abandon_call(low, high, token)
    }
}

#[allow(unsafe_code, clippy::too_many_arguments)]
#[unsafe(export_name = "ring3_abi_v1_begin_callback")]
pub extern "C" fn begin_callback(
    low: u32,
    high: u32,
    generation: u32,
    outer_token: u32,
    entry_pc: u32,
    return_pc: u32,
    return_id: u32,
    count: u32,
) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::begin_callback(
            low,
            high,
            generation,
            outer_token,
            entry_pc,
            return_pc,
            return_id,
            count,
        )
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_finish_callback")]
pub extern "C" fn finish_callback(low: u32, high: u32, generation: u32, token: u32) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::finish_callback(low, high, generation, token)
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_abort_callback")]
pub extern "C" fn abort_callback(low: u32, high: u32, token: u32) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::abort_callback(low, high, token)
    }
}
