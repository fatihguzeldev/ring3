export_process_calls! {
    core_prepare(pc: u32, instructions: u32, blocks: u32);
    core_code_ptr();
    core_code_len();
    core_context_ptr();
    core_begin();
    core_end();
    core_write(context: u32, address: u32, width: u32, value: u32);
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_core_read")]
pub extern "C" fn core_read(context: u32, address: u32, width: u32) -> i64 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::core_read(context, address, width)
    }
}
