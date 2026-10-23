use super::{CTX, REMAIN, StopReason, link};
use wasm_encoder::{BlockType, Function, InstructionSink, ValType};
use wasmparser::{BinaryReader, FunctionBody, Operator};

const REASON: u32 = 108;
pub(super) const PRECISE: i32 = -1;

// Only the two region bodies use this pass. Helpers retain their own return contracts.
// Original branch labels stay relative to the same enclosing controls; a return now
// branches through all of those controls to one publication point outside the wrapper.
pub(super) fn finish(
    function: Function,
    plan: &crate::cpu::core::region::values::RegionPlan,
    numeric: bool,
    precise: bool,
) -> Vec<u8> {
    let original = function.into_raw_body();
    let body = FunctionBody::new(BinaryReader::new(&original, 0));
    let mut reader = body
        .get_operators_reader()
        .expect("generated region body has valid local declarations");
    let mut result = Vec::with_capacity(original.len());
    result.extend_from_slice(&original[..reader.original_position() as usize]);
    InstructionSink::new(&mut result).block(BlockType::Empty);
    let mut depth = 0u32;
    let mut guard = false;
    let mut complete = false;
    let mut previous_constant = None;
    while !reader.eof() {
        let start = reader.original_position() as usize;
        let operator = reader.read().expect("generated region operator is valid");
        let end = reader.original_position() as usize;
        match operator {
            Operator::Return if !guard => {
                // No state has been loaded yet. Publishing here would overwrite the
                // caller's state with default locals on an invalid context or lease.
                assert_eq!(depth, 1, "the lease guard is the first region return");
                assert_eq!(previous_constant, Some(StopReason::Infrastructure as i32));
                guard = true;
                result.extend_from_slice(&original[start..end]);
            }
            Operator::Return => {
                InstructionSink::new(&mut result)
                    .local_set(REASON)
                    .br(depth);
            }
            Operator::Br { relative_depth } | Operator::BrIf { relative_depth } => {
                assert!(
                    relative_depth < depth,
                    "region branches target explicit controls"
                );
                result.extend_from_slice(&original[start..end]);
            }
            Operator::BrTable { ref targets } => {
                assert!(targets.default() < depth);
                for target in targets.targets() {
                    assert!(
                        target.expect("generated branch table target is valid") < depth,
                        "region branch tables target explicit controls"
                    );
                }
                result.extend_from_slice(&original[start..end]);
            }
            Operator::Block { .. } | Operator::Loop { .. } | Operator::If { .. } => {
                depth += 1;
                result.extend_from_slice(&original[start..end]);
            }
            Operator::End => {
                if depth == 0 {
                    assert!(reader.eof(), "the function end is the final operator");
                    complete = true;
                } else {
                    depth -= 1;
                }
                // The original function end now closes our surrounding empty block.
                result.extend_from_slice(&original[start..end]);
            }
            _ => result.extend_from_slice(&original[start..end]),
        }
        previous_constant = match operator {
            Operator::I32Const { value } => Some(value),
            _ => None,
        };
    }
    assert!(
        guard && complete,
        "the complete generated region body was rewritten"
    );
    let shift = u32::from(numeric);
    let mut code = InstructionSink::new(&mut result);
    super::numeric::emit_normalizer(plan, &mut code, numeric);
    code.local_get(CTX);
    for (local, _) in link::boundary_fields(numeric) {
        code.local_get(local);
    }
    code.call(5 + shift);
    if precise {
        code.local_get(REASON);
    } else {
        code.local_get(REASON)
            .i32_const(PRECISE)
            .i32_eq()
            .if_(BlockType::Result(ValType::I32))
            .local_get(CTX)
            .local_get(REMAIN)
            .call(6 + shift)
            .else_()
            .local_get(REASON)
            .end();
    }
    code.return_().end();
    result
}
