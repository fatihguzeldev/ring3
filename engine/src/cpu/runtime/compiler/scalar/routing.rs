use super::*;

const RETURN_PC: u32 = 77;
const RETURN_ROUTE: u32 = 78;

fn landing(entries: &[EntryPoint], pc: u32) -> i32 {
    entries
        .binary_search_by_key(&pc, |entry| entry.pc)
        .map_or(-1, |index| {
            let entry = &entries[index];
            ((entry.block << 6) | entry.instruction) as i32
        })
}

pub(super) fn next(
    out: &mut Output,
    blocks: &[Block],
    entries: &[EntryPoint],
    index: usize,
) -> bool {
    let block = &blocks[index];
    let Some(instruction) = block.instructions.last() else {
        out.c().i32_const(-1).local_set(A);
        return false;
    };
    let fallthrough = instruction.next_pc().0;
    match *instruction.operation() {
        Operation::Call { target } => {
            // a prediction never replaces the architectural stack read on return.
            out.c()
                .i32_const(fallthrough as i32)
                .local_set(RETURN_PC)
                .i32_const(landing(entries, fallthrough).wrapping_add(1))
                .local_set(RETURN_ROUTE);
            match target {
                BranchTarget::Direct(target) => {
                    out.c().i32_const(landing(entries, target.0)).local_set(A);
                }
                _ => {
                    out.c().i32_const(-2).local_set(A);
                }
            }
            return !matches!(target, BranchTarget::Direct(_));
        }
        Operation::Jump {
            target: BranchTarget::Direct(target),
        } => {
            out.c().i32_const(landing(entries, target.0)).local_set(A);
        }
        Operation::ConditionalJump { target, .. } | Operation::CountBranch { target, .. } => {
            out.c()
                .i32_const(landing(entries, target.0))
                .i32_const(landing(entries, fallthrough))
                .local_get(PC)
                .i32_const(target.0 as i32)
                .i32_eq()
                .select()
                .local_set(A);
        }
        Operation::Return { .. } => {
            out.c()
                .local_get(RETURN_ROUTE)
                .i32_eqz()
                .i32_eqz()
                .local_get(PC)
                .local_get(RETURN_PC)
                .i32_eq()
                .i32_and()
                .if_(BlockType::Result(ValType::I32))
                .local_get(RETURN_ROUTE)
                .i32_const(1)
                .i32_sub()
                .else_();
            out.c().i32_const(-2).end().local_set(A);
            return true;
        }
        Operation::Jump { .. } => {
            out.c().i32_const(-2).local_set(A);
            return true;
        }
        _ => {
            out.c()
                .i32_const(landing(entries, fallthrough))
                .local_set(A);
        }
    }
    false
}
