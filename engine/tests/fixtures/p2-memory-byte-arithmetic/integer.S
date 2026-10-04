.intel_syntax noprefix
.code32
.text
.globl add_memory_reg
.type add_memory_reg, @function
add_memory_reg:
  add byte ptr [esi], ah
.size add_memory_reg, .-add_memory_reg
.globl add_memory_imm
.type add_memory_imm, @function
add_memory_imm:
  add byte ptr [esi], 0x80
.size add_memory_imm, .-add_memory_imm
.globl sub_memory_reg
.type sub_memory_reg, @function
sub_memory_reg:
  sub byte ptr [esi], ah
.size sub_memory_reg, .-sub_memory_reg
.globl sub_memory_imm
.type sub_memory_imm, @function
sub_memory_imm:
  sub byte ptr [esi], 0x80
.size sub_memory_imm, .-sub_memory_imm
