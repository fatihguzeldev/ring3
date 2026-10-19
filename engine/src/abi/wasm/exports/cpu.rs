export_process_calls! {
    configure_system_register_bank();
    cpu_read8(address: u32);
    cpu_read16(address: u32);
    cpu_read32(address: u32);
    cpu_read(address: u32, length: u32, alignment: u32);
    cpu_write(address: u32, length: u32, alignment: u32);
    cpu_write_resident(key_low: u32, key_high: u32, id_low: u32, id_high: u32, address: u32, length: u32, alignment: u32);
    cpu_fxsave(address: u32);
    cpu_fxsave_resident(key_low: u32, key_high: u32, id_low: u32, id_high: u32, address: u32);
    cpu_mask_write(address: u32, length: u32, mask: u32);
    cpu_mask_write_resident(key_low: u32, key_high: u32, id_low: u32, id_high: u32, address: u32, length: u32, mask: u32);
    x87_op(operation: u32, address: u32, flags: u32, pc: u32, opcode: u32, waiting: u32);
    x87_resident_op(key_low: u32, key_high: u32, id_low: u32, id_high: u32, operation: u32, address: u32, flags: u32, pc: u32, opcode: u32, waiting: u32);
    simd_execute(descriptor: u32, operands: u32, immediate: u32, source_gpr: u32, destination_gpr: u32, flags: u32);
    far_call(offset: u32, selector: u32, word: u32, return_eip: u32, esp: u32);
    far_call_resident(key_low: u32, key_high: u32, id_low: u32, id_high: u32, offset: u32, selector: u32, word: u32, return_eip: u32, esp: u32);
    far_return(offset: u32, selector: u32, word: u32);
    far_return_resident(key_low: u32, key_high: u32, id_low: u32, id_high: u32, offset: u32, selector: u32, word: u32);
    far_jump(offset: u32, selector: u32, word: u32);
    far_jump_resident(key_low: u32, key_high: u32, id_low: u32, id_high: u32, offset: u32, selector: u32, word: u32);
    segment_load(target: u32, selector: u32);
    segment_load_resident(key_low: u32, key_high: u32, id_low: u32, id_high: u32, target: u32, selector: u32);
    segment_load_far(target: u32, address: u32, word: u32);
    segment_load_far_resident(key_low: u32, key_high: u32, id_low: u32, id_high: u32, target: u32, address: u32, word: u32);
    set_segment_base(segment: u32, base: u32);
    configure_segment_descriptor(selector: u32, base: u32, dpl: u32, present: u32, kind: u32);
    configure_segment_descriptor_raw(selector: u32, low: u32, high: u32);
    simd_execute_resident(key_low: u32, key_high: u32, id_low: u32, id_high: u32, descriptor: u32, operands: u32, immediate: u32, source_gpr: u32, destination_gpr: u32, flags: u32);
    cpu_integer(descriptor: u32, address: u32);
    cpu_integer_resident(key_low: u32, key_high: u32, id_low: u32, id_high: u32, descriptor: u32, address: u32);
    stack_execute(control: u32);
    stack_execute_resident(key_low: u32, key_high: u32, id_low: u32, id_high: u32, control: u32);
}

#[allow(unsafe_code, clippy::too_many_arguments)]
#[unsafe(export_name = "ring3_abi_v1_x87_resident_pair")]
pub extern "C" fn x87_resident_pair(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    first_pc: u32,
    address0: u32,
    address1: u32,
    flags: u32,
    max_steps: u32,
) -> i64 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::x87_resident_pair(
            key_low, key_high, id_low, id_high, first_pc, address0, address1, flags, max_steps,
        )
    }
}
