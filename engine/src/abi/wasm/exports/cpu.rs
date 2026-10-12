export_process_calls! {
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
    segment_load(target: u32, selector: u32);
    segment_load_resident(key_low: u32, key_high: u32, id_low: u32, id_high: u32, target: u32, selector: u32);
    segment_load_far(target: u32, address: u32, word: u32);
    segment_load_far_resident(key_low: u32, key_high: u32, id_low: u32, id_high: u32, target: u32, address: u32, word: u32);
    set_segment_base(segment: u32, base: u32);
    configure_segment_descriptor(selector: u32, base: u32, dpl: u32, present: u32, kind: u32);
    simd_execute_resident(key_low: u32, key_high: u32, id_low: u32, id_high: u32, descriptor: u32, operands: u32, immediate: u32, source_gpr: u32, destination_gpr: u32, flags: u32);
    cpu_integer(descriptor: u32, address: u32);
    cpu_integer_resident(key_low: u32, key_high: u32, id_low: u32, id_high: u32, descriptor: u32, address: u32);
    stack_execute(control: u32);
    stack_execute_resident(key_low: u32, key_high: u32, id_low: u32, id_high: u32, control: u32);
}
