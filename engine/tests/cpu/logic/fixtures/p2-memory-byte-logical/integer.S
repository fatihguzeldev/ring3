.intel_syntax noprefix
.code32
.text
.globl and_memory_reg
.type and_memory_reg, @function
and_memory_reg:
  and byte ptr [esi], ah
.size and_memory_reg, .-and_memory_reg
.globl and_memory_imm
.type and_memory_imm, @function
and_memory_imm:
  and byte ptr [esi], 0x80
.size and_memory_imm, .-and_memory_imm
.globl or_memory_reg
.type or_memory_reg, @function
or_memory_reg:
  or byte ptr [esi], ah
.size or_memory_reg, .-or_memory_reg
.globl or_memory_imm
.type or_memory_imm, @function
or_memory_imm:
  or byte ptr [esi], 0x80
.size or_memory_imm, .-or_memory_imm
.globl xor_memory_reg
.type xor_memory_reg, @function
xor_memory_reg:
  xor byte ptr [esi], ah
.size xor_memory_reg, .-xor_memory_reg
.globl xor_memory_imm
.type xor_memory_imm, @function
xor_memory_imm:
  xor byte ptr [esi], 0x80
.size xor_memory_imm, .-xor_memory_imm
