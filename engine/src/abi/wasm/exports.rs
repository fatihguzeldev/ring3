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
#[unsafe(export_name = "ring3_abi_v1_load_pe32")]
pub extern "C" fn load_pe32(length: u32) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::load_pe32(length)
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_load_pe32_at")]
pub extern "C" fn load_pe32_at(length: u32, actual_base: u32) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::load_pe32_at(length, actual_base)
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_load_pe32_linked_at")]
pub extern "C" fn load_pe32_linked_at(length: u32, actual_base: u32, gate_base: u32) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::load_pe32_linked_at(length, actual_base, gate_base)
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_load_pe32_linked_v2_at")]
pub extern "C" fn load_pe32_linked_v2_at(length: u32, actual_base: u32, gate_base: u32) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::load_pe32_linked_v2_at(length, actual_base, gate_base)
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_begin_image_input")]
pub extern "C" fn begin_image_input(total: u32) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::begin_image_input(total)
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_append_image_input")]
pub extern "C" fn append_image_input(offset: u32, length: u32) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::append_image_input(offset, length)
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_abort_image_input")]
pub extern "C" fn abort_image_input() -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::abort_image_input()
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_load_pe32_linked_v2_input_at")]
pub extern "C" fn load_pe32_linked_v2_input_at(actual_base: u32, gate_base: u32) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::load_pe32_linked_v2_input_at(actual_base, gate_base)
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_start_loaded_image")]
pub extern "C" fn start_loaded_image(stack_base: u32, pages: u32) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::start_loaded_image(stack_base, pages)
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
#[unsafe(export_name = "ring3_abi_v1_compile_entries")]
pub extern "C" fn compile_entries(count: u32, gate_count: u32) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::compile_entries(count, gate_count)
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_compile_resident")]
pub extern "C" fn compile_resident(count: u32) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::compile_resident(count)
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_compile_resident_with_gates")]
pub extern "C" fn compile_resident_with_gates(count: u32, gate_count: u32) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::compile_resident_with_gates(count, gate_count)
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_compile_resident_entries")]
pub extern "C" fn compile_resident_entries(count: u32, gate_count: u32) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::compile_resident_entries(count, gate_count)
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_find_resident")]
pub extern "C" fn find_resident(pc: u32) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::find_resident(pc)
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_compile_resident_callback_unit")]
pub extern "C" fn compile_resident_callback_unit(
    key_low: u32,
    key_high: u32,
    home_low: u32,
    home_high: u32,
    callback_token: u32,
    count: u32,
    gate_count: u32,
) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::compile_resident_callback_unit(
            key_low,
            key_high,
            home_low,
            home_high,
            callback_token,
            count,
            gate_count,
        )
    }
}

#[allow(unsafe_code, clippy::too_many_arguments)]
#[unsafe(export_name = "ring3_abi_v1_acknowledge_resident_callback_installation")]
pub extern "C" fn acknowledge_resident_callback_installation(
    key_low: u32,
    key_high: u32,
    home_low: u32,
    home_high: u32,
    callback_token: u32,
    unit_low: u32,
    unit_high: u32,
    slot: u32,
) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::acknowledge_resident_callback_installation(
            key_low,
            key_high,
            home_low,
            home_high,
            callback_token,
            unit_low,
            unit_high,
            slot,
        )
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_dispatcher_module")]
pub extern "C" fn dispatcher_module(low: u32, high: u32) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::dispatcher_module(low, high)
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_resident_module")]
pub extern "C" fn resident_module(low: u32, high: u32) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::resident_module(low, high)
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_retire_stale_resident")]
pub extern "C" fn retire_stale_resident(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::retire_stale_resident(key_low, key_high, id_low, id_high)
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_acknowledge_resident_installation")]
pub extern "C" fn acknowledge_resident_installation(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    slot: u32,
) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::acknowledge_resident_installation(
            key_low, key_high, id_low, id_high, slot,
        )
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_find_installed_resident")]
pub extern "C" fn find_installed_resident(key_low: u32, key_high: u32, pc: u32) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::find_installed_resident(key_low, key_high, pc)
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_guard_dispatch_entry")]
pub extern "C" fn guard_dispatch_entry(
    key_low: u32,
    key_high: u32,
    state: u32,
    exit: u32,
    cancel: u32,
) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::guard_dispatch_entry(key_low, key_high, state, exit, cancel)
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_guard_resident")]
pub extern "C" fn guard_resident(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    state: u32,
    exit: u32,
    cancel: u32,
) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::guard_resident(
            key_low, key_high, id_low, id_high, state, exit, cancel,
        )
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
#[unsafe(export_name = "ring3_abi_v1_write8")]
pub extern "C" fn write8(address: u32, value: u32) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::write8(address, value)
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_store8")]
pub extern "C" fn store8(address: u32, value: u32) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::store8(address, value)
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_store_resident8")]
pub extern "C" fn store_resident8(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    address: u32,
    value: u32,
) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::store_resident8(key_low, key_high, id_low, id_high, address, value)
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_read8")]
pub extern "C" fn read8(address: u32) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::read8(address)
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_read16")]
pub extern "C" fn read16(address: u32) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::read16(address)
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
#[unsafe(export_name = "ring3_abi_v1_store_resident32")]
pub extern "C" fn store_resident32(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    address: u32,
    value: u32,
) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::store_resident32(key_low, key_high, id_low, id_high, address, value)
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
#[unsafe(export_name = "ring3_abi_v1_capture_resident_call")]
pub extern "C" fn capture_resident_call(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    convention: u32,
    words: u32,
) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::capture_resident_call(
            key_low, key_high, id_low, id_high, convention, words,
        )
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_complete_resident_call")]
pub extern "C" fn complete_resident_call(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    token: u32,
    result: u32,
) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::complete_resident_call(
            key_low, key_high, id_low, id_high, token, result,
        )
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

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_complete_windows_call")]
pub extern "C" fn complete_windows_call(low: u32, high: u32, generation: u32, token: u32) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::complete_windows_call(low, high, generation, token)
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_complete_resident_windows_call")]
pub extern "C" fn complete_resident_windows_call(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    token: u32,
) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::complete_resident_windows_call(
            key_low, key_high, id_low, id_high, token,
        )
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_select_resident_callback_unit")]
pub extern "C" fn select_resident_callback_unit(
    key_low: u32,
    key_high: u32,
    home_low: u32,
    home_high: u32,
    callback_token: u32,
    target_low: u32,
    target_high: u32,
) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::select_resident_callback_unit(
            key_low,
            key_high,
            home_low,
            home_high,
            callback_token,
            target_low,
            target_high,
        )
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_capture_active_resident_callback_call")]
pub extern "C" fn capture_active_resident_callback_call(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    callback_token: u32,
    convention: u32,
    words: u32,
) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::capture_active_resident_callback_call(
            key_low,
            key_high,
            id_low,
            id_high,
            callback_token,
            convention,
            words,
        )
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_complete_active_resident_callback_call")]
pub extern "C" fn complete_active_resident_callback_call(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    callback_token: u32,
    inner_token: u32,
    result: u32,
) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::complete_active_resident_callback_call(
            key_low,
            key_high,
            id_low,
            id_high,
            callback_token,
            inner_token,
            result,
        )
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_complete_active_resident_callback_windows_call")]
pub extern "C" fn complete_active_resident_callback_windows_call(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    callback_token: u32,
    inner_token: u32,
) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::complete_active_resident_callback_windows_call(
            key_low,
            key_high,
            id_low,
            id_high,
            callback_token,
            inner_token,
        )
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_capture_resident_callback_call")]
pub extern "C" fn capture_resident_callback_call(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    callback_token: u32,
    convention: u32,
    words: u32,
) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::capture_resident_callback_call(
            key_low,
            key_high,
            id_low,
            id_high,
            callback_token,
            convention,
            words,
        )
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_complete_resident_callback_call")]
pub extern "C" fn complete_resident_callback_call(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    callback_token: u32,
    inner_token: u32,
    result: u32,
) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::complete_resident_callback_call(
            key_low,
            key_high,
            id_low,
            id_high,
            callback_token,
            inner_token,
            result,
        )
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

#[allow(unsafe_code, clippy::too_many_arguments)]
#[unsafe(export_name = "ring3_abi_v1_begin_resident_callback")]
pub extern "C" fn begin_resident_callback(
    key_low: u32,
    key_high: u32,
    outer_low: u32,
    outer_high: u32,
    callback_low: u32,
    callback_high: u32,
    outer_token: u32,
    entry_pc: u32,
    return_pc: u32,
    return_id: u32,
    count: u32,
) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::begin_resident_callback(
            key_low,
            key_high,
            outer_low,
            outer_high,
            callback_low,
            callback_high,
            outer_token,
            entry_pc,
            return_pc,
            return_id,
            count,
        )
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_finish_resident_callback")]
pub extern "C" fn finish_resident_callback(
    key_low: u32,
    key_high: u32,
    callback_low: u32,
    callback_high: u32,
    token: u32,
) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::finish_resident_callback(
            key_low,
            key_high,
            callback_low,
            callback_high,
            token,
        )
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_authorize_resident_callback")]
pub extern "C" fn authorize_resident_callback(
    key_low: u32,
    key_high: u32,
    callback_low: u32,
    callback_high: u32,
    token: u32,
) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::authorize_resident_callback(
            key_low,
            key_high,
            callback_low,
            callback_high,
            token,
        )
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

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_resume_callback_code")]
pub extern "C" fn resume_callback_code(
    low: u32,
    high: u32,
    generation: u32,
    callback_token: u32,
    count: u32,
    gate_count: u32,
) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::resume_callback_code(
            low,
            high,
            generation,
            callback_token,
            count,
            gate_count,
        )
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_resume_callback_entries")]
pub extern "C" fn resume_callback_entries(
    low: u32,
    high: u32,
    generation: u32,
    callback_token: u32,
    count: u32,
    gate_count: u32,
) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::resume_callback_entries(
            low,
            high,
            generation,
            callback_token,
            count,
            gate_count,
        )
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_load_pe32_linked_v3_input_at")]
pub extern "C" fn load_pe32_linked_v3_input_at(actual_base: u32, gate_base: u32) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::load_pe32_linked_v3_input_at(actual_base, gate_base)
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_load_pe32_linked_v4_input_at")]
pub extern "C" fn load_pe32_linked_v4_input_at(actual_base: u32, gate_base: u32) -> u32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::load_pe32_linked_v4_input_at(actual_base, gate_base)
    }
}
