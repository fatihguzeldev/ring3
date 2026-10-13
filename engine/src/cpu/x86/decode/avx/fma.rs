use super::*;

pub(super) fn lower(
    instruction: &Instruction,
    encoding: Encoding,
) -> Option<Result<AvxOperation, DecodeError>> {
    let (kind, double, length) = match instruction.code() {
        Code::VEX_Vfmaddsub132ps_xmm_xmm_xmmm128 => (73, false, 0),
        Code::VEX_Vfmaddsub132ps_ymm_ymm_ymmm256 => (74, false, 1),
        Code::VEX_Vfmaddsub132pd_xmm_xmm_xmmm128 => (75, true, 0),
        Code::VEX_Vfmaddsub132pd_ymm_ymm_ymmm256 => (76, true, 1),
        Code::VEX_Vfmsubadd132ps_xmm_xmm_xmmm128 => (85, false, 0),
        Code::VEX_Vfmsubadd132ps_ymm_ymm_ymmm256 => (86, false, 1),
        Code::VEX_Vfmsubadd132pd_xmm_xmm_xmmm128 => (87, true, 0),
        Code::VEX_Vfmsubadd132pd_ymm_ymm_ymmm256 => (88, true, 1),
        Code::VEX_Vfmadd132ps_xmm_xmm_xmmm128 => (1, false, 0),
        Code::VEX_Vfmadd132ps_ymm_ymm_ymmm256 => (2, false, 1),
        Code::VEX_Vfmadd132pd_xmm_xmm_xmmm128 => (3, true, 0),
        Code::VEX_Vfmadd132pd_ymm_ymm_ymmm256 => (4, true, 1),
        Code::VEX_Vfmadd132ss_xmm_xmm_xmmm32 => (5, false, 2),
        Code::VEX_Vfmadd132sd_xmm_xmm_xmmm64 => (6, true, 2),
        Code::VEX_Vfmsub132ps_xmm_xmm_xmmm128 => (19, false, 0),
        Code::VEX_Vfmsub132ps_ymm_ymm_ymmm256 => (20, false, 1),
        Code::VEX_Vfmsub132pd_xmm_xmm_xmmm128 => (21, true, 0),
        Code::VEX_Vfmsub132pd_ymm_ymm_ymmm256 => (22, true, 1),
        Code::VEX_Vfmsub132ss_xmm_xmm_xmmm32 => (23, false, 2),
        Code::VEX_Vfmsub132sd_xmm_xmm_xmmm64 => (24, true, 2),
        Code::VEX_Vfnmadd132ps_xmm_xmm_xmmm128 => (37, false, 0),
        Code::VEX_Vfnmadd132ps_ymm_ymm_ymmm256 => (38, false, 1),
        Code::VEX_Vfnmadd132pd_xmm_xmm_xmmm128 => (39, true, 0),
        Code::VEX_Vfnmadd132pd_ymm_ymm_ymmm256 => (40, true, 1),
        Code::VEX_Vfnmadd132ss_xmm_xmm_xmmm32 => (41, false, 2),
        Code::VEX_Vfnmadd132sd_xmm_xmm_xmmm64 => (42, true, 2),
        Code::VEX_Vfnmsub132ps_xmm_xmm_xmmm128 => (55, false, 0),
        Code::VEX_Vfnmsub132ps_ymm_ymm_ymmm256 => (56, false, 1),
        Code::VEX_Vfnmsub132pd_xmm_xmm_xmmm128 => (57, true, 0),
        Code::VEX_Vfnmsub132pd_ymm_ymm_ymmm256 => (58, true, 1),
        Code::VEX_Vfnmsub132ss_xmm_xmm_xmmm32 => (59, false, 2),
        Code::VEX_Vfnmsub132sd_xmm_xmm_xmmm64 => (60, true, 2),
        Code::VEX_Vfmaddsub213ps_xmm_xmm_xmmm128 => (77, false, 0),
        Code::VEX_Vfmaddsub213ps_ymm_ymm_ymmm256 => (78, false, 1),
        Code::VEX_Vfmaddsub213pd_xmm_xmm_xmmm128 => (79, true, 0),
        Code::VEX_Vfmaddsub213pd_ymm_ymm_ymmm256 => (80, true, 1),
        Code::VEX_Vfmsubadd213ps_xmm_xmm_xmmm128 => (89, false, 0),
        Code::VEX_Vfmsubadd213ps_ymm_ymm_ymmm256 => (90, false, 1),
        Code::VEX_Vfmsubadd213pd_xmm_xmm_xmmm128 => (91, true, 0),
        Code::VEX_Vfmsubadd213pd_ymm_ymm_ymmm256 => (92, true, 1),
        Code::VEX_Vfmadd213ps_xmm_xmm_xmmm128 => (7, false, 0),
        Code::VEX_Vfmadd213ps_ymm_ymm_ymmm256 => (8, false, 1),
        Code::VEX_Vfmadd213pd_xmm_xmm_xmmm128 => (9, true, 0),
        Code::VEX_Vfmadd213pd_ymm_ymm_ymmm256 => (10, true, 1),
        Code::VEX_Vfmadd213ss_xmm_xmm_xmmm32 => (11, false, 2),
        Code::VEX_Vfmadd213sd_xmm_xmm_xmmm64 => (12, true, 2),
        Code::VEX_Vfmsub213ps_xmm_xmm_xmmm128 => (25, false, 0),
        Code::VEX_Vfmsub213ps_ymm_ymm_ymmm256 => (26, false, 1),
        Code::VEX_Vfmsub213pd_xmm_xmm_xmmm128 => (27, true, 0),
        Code::VEX_Vfmsub213pd_ymm_ymm_ymmm256 => (28, true, 1),
        Code::VEX_Vfmsub213ss_xmm_xmm_xmmm32 => (29, false, 2),
        Code::VEX_Vfmsub213sd_xmm_xmm_xmmm64 => (30, true, 2),
        Code::VEX_Vfnmadd213ps_xmm_xmm_xmmm128 => (43, false, 0),
        Code::VEX_Vfnmadd213ps_ymm_ymm_ymmm256 => (44, false, 1),
        Code::VEX_Vfnmadd213pd_xmm_xmm_xmmm128 => (45, true, 0),
        Code::VEX_Vfnmadd213pd_ymm_ymm_ymmm256 => (46, true, 1),
        Code::VEX_Vfnmadd213ss_xmm_xmm_xmmm32 => (47, false, 2),
        Code::VEX_Vfnmadd213sd_xmm_xmm_xmmm64 => (48, true, 2),
        Code::VEX_Vfnmsub213ps_xmm_xmm_xmmm128 => (61, false, 0),
        Code::VEX_Vfnmsub213ps_ymm_ymm_ymmm256 => (62, false, 1),
        Code::VEX_Vfnmsub213pd_xmm_xmm_xmmm128 => (63, true, 0),
        Code::VEX_Vfnmsub213pd_ymm_ymm_ymmm256 => (64, true, 1),
        Code::VEX_Vfnmsub213ss_xmm_xmm_xmmm32 => (65, false, 2),
        Code::VEX_Vfnmsub213sd_xmm_xmm_xmmm64 => (66, true, 2),
        Code::VEX_Vfmaddsub231ps_xmm_xmm_xmmm128 => (81, false, 0),
        Code::VEX_Vfmaddsub231ps_ymm_ymm_ymmm256 => (82, false, 1),
        Code::VEX_Vfmaddsub231pd_xmm_xmm_xmmm128 => (83, true, 0),
        Code::VEX_Vfmaddsub231pd_ymm_ymm_ymmm256 => (84, true, 1),
        Code::VEX_Vfmsubadd231ps_xmm_xmm_xmmm128 => (93, false, 0),
        Code::VEX_Vfmsubadd231ps_ymm_ymm_ymmm256 => (94, false, 1),
        Code::VEX_Vfmsubadd231pd_xmm_xmm_xmmm128 => (95, true, 0),
        Code::VEX_Vfmsubadd231pd_ymm_ymm_ymmm256 => (96, true, 1),
        Code::VEX_Vfmadd231ps_xmm_xmm_xmmm128 => (13, false, 0),
        Code::VEX_Vfmadd231ps_ymm_ymm_ymmm256 => (14, false, 1),
        Code::VEX_Vfmadd231pd_xmm_xmm_xmmm128 => (15, true, 0),
        Code::VEX_Vfmadd231pd_ymm_ymm_ymmm256 => (16, true, 1),
        Code::VEX_Vfmadd231ss_xmm_xmm_xmmm32 => (17, false, 2),
        Code::VEX_Vfmadd231sd_xmm_xmm_xmmm64 => (18, true, 2),
        Code::VEX_Vfmsub231ps_xmm_xmm_xmmm128 => (31, false, 0),
        Code::VEX_Vfmsub231ps_ymm_ymm_ymmm256 => (32, false, 1),
        Code::VEX_Vfmsub231pd_xmm_xmm_xmmm128 => (33, true, 0),
        Code::VEX_Vfmsub231pd_ymm_ymm_ymmm256 => (34, true, 1),
        Code::VEX_Vfmsub231ss_xmm_xmm_xmmm32 => (35, false, 2),
        Code::VEX_Vfmsub231sd_xmm_xmm_xmmm64 => (36, true, 2),
        Code::VEX_Vfnmadd231ps_xmm_xmm_xmmm128 => (49, false, 0),
        Code::VEX_Vfnmadd231ps_ymm_ymm_ymmm256 => (50, false, 1),
        Code::VEX_Vfnmadd231pd_xmm_xmm_xmmm128 => (51, true, 0),
        Code::VEX_Vfnmadd231pd_ymm_ymm_ymmm256 => (52, true, 1),
        Code::VEX_Vfnmadd231ss_xmm_xmm_xmmm32 => (53, false, 2),
        Code::VEX_Vfnmadd231sd_xmm_xmm_xmmm64 => (54, true, 2),
        Code::VEX_Vfnmsub231ps_xmm_xmm_xmmm128 => (67, false, 0),
        Code::VEX_Vfnmsub231ps_ymm_ymm_ymmm256 => (68, false, 1),
        Code::VEX_Vfnmsub231pd_xmm_xmm_xmmm128 => (69, true, 0),
        Code::VEX_Vfnmsub231pd_ymm_ymm_ymmm256 => (70, true, 1),
        Code::VEX_Vfnmsub231ss_xmm_xmm_xmmm32 => (71, false, 2),
        Code::VEX_Vfnmsub231sd_xmm_xmm_xmmm64 => (72, true, 2),
        _ => return None,
    };
    Some((|| {
        encoding.validate_w(double)?;
        // scalar FMA is LIG; packed forms have an exact vector width.
        encoding.validate(false, length, false)?;
        let mut operands = index(instruction, 0, false, false)?;
        operands |= index(instruction, 1, false, false)? << 3;
        operands |= index(instruction, 2, false, true)? << 6;
        if instruction.op_kind(2) == OpKind::Memory {
            operands |= 1 << 12;
        }
        let value = AvxInstruction::decode_descriptor(0x01001300 | kind, operands, 0)
            .map_err(|_| invalid())?;
        bind_address(instruction, value)
    })())
}

#[cfg(test)]
mod native;
