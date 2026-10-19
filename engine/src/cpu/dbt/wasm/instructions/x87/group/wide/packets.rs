use super::{GroupLocals, PacketContract, X87WideGroup32, descriptor, validate_packet};
use crate::cpu::dbt::{X87_WIDE_GROUP_MAX_STEPS, wasm::imports::Imports};
use wasm_encoder::{BlockType, InstructionSink};

pub(super) fn validate(
    code: &mut InstructionSink<'_>,
    group: X87WideGroup32<'_>,
    locals: GroupLocals,
    imports: Imports,
) {
    let first = PacketContract::from(descriptor(group, 0));
    let mut contracts = [first; X87_WIDE_GROUP_MAX_STEPS];
    let mut count = 0;
    for index in 0..group.len() {
        let contract = PacketContract::from(descriptor(group, index));
        if !contracts[..count].contains(&contract) {
            contracts[count] = contract;
            count += 1;
        }
    }
    // catch failure before retiring the completed prefix, exactly as max8 does.
    code.block(BlockType::Empty);
    for (index, contract) in contracts[..count].iter().enumerate() {
        let last = index + 1 == count;
        if !last {
            let mut first = true;
            for step in 0..group.len() {
                if PacketContract::from(descriptor(group, step)) == *contract {
                    code.local_get(locals.reported_step)
                        .i32_const(step as i32)
                        .i32_eq();
                    if !first {
                        code.i32_or();
                    }
                    first = false;
                }
            }
            code.if_(BlockType::Empty);
        }
        validate_packet(code, *contract, imports, u32::from(!last));
        if !last {
            code.br(1).end();
        }
    }
    code.end();
}
