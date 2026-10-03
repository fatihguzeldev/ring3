.intel_syntax noprefix
.text

.global register_chain
.type register_chain,@function
register_chain:
  mov eax, 0xffffffff
  mov edx, 0x7fffffff
  add eax, 1
register_after_add:
  adc edx, 0
register_after_adc:
  mov ecx, 0
  sub ecx, 1
register_after_sub:
  sbb edx, 0
  jmp register_done
register_done:
  ud2
.size register_chain, .-register_chain

.org 0x100
.global memory_chain
.type memory_chain,@function
memory_chain:
  mov eax, 0x5000
  mov ecx, 4
  mov ebx, 0x7fffffff
  mov edx, 0xffffffff
  add edx, 1
memory_after_add:
  adc DWORD PTR [eax+ecx*4], -1
memory_after_adc_store:
  adc ebx, DWORD PTR [eax+ecx*4]
memory_after_adc_load:
  sub edx, 1
memory_after_sub:
  sbb DWORD PTR [eax+ecx*4], -1
memory_after_sbb_store:
  sbb ebx, DWORD PTR [eax+ecx*4]
  jmp memory_done
memory_done:
  ud2
.size memory_chain, .-memory_chain

.macro row name, at, bytes:vararg
  .org \at
  .global \name
  .type \name,@function
\name:
  .byte \bytes
  jmp \name\()_done
\name\()_done:
  ud2
  .size \name, .-\name
.endm

# literal encodings keep all five forms per kind distinct.
row adc_rm32_r32, 0x200, 0x11,0xd0
row adc_r32_rm32, 0x210, 0x13,0xc2
row adc_eax_imm32, 0x220, 0x15,0xff,0xff,0xff,0xff
row adc_rm32_imm32, 0x230, 0x81,0xd0,0xff,0xff,0xff,0xff
row adc_rm32_imm8, 0x240, 0x83,0xd0,0xff
row sbb_rm32_r32, 0x250, 0x19,0xd0
row sbb_r32_rm32, 0x260, 0x1b,0xc2
row sbb_eax_imm32, 0x270, 0x1d,0xff,0xff,0xff,0xff
row sbb_rm32_imm32, 0x280, 0x81,0xd8,0xff,0xff,0xff,0xff
row sbb_rm32_imm8, 0x290, 0x83,0xd8,0xff

row adc_self, 0x300, 0x11,0xc0
row sbb_self, 0x310, 0x19,0xc0
row adc_esp_edx, 0x320, 0x11,0xd4
row sbb_esp_edx, 0x330, 0x19,0xd4
row adc_destination_base, 0x340, 0x13,0x00
row sbb_destination_base, 0x350, 0x1b,0x00
row adc_source_base, 0x360, 0x11,0x00
row sbb_source_base, 0x370, 0x19,0x00
row adc_destination_index, 0x380, 0x13,0x0c,0x88
row sbb_destination_index, 0x390, 0x1b,0x0c,0x88
row adc_esp_memory, 0x3a0, 0x13,0x14,0x24
row sbb_esp_memory, 0x3b0, 0x1b,0x14,0x24
row adc_no_base, 0x3c0, 0x13,0x14,0x8d,0x00,0x50,0x00,0x00
row sbb_no_base, 0x3d0, 0x1b,0x14,0x8d,0x00,0x50,0x00,0x00
row adc_ebp_memory, 0x3e0, 0x83,0x55,0x00,0xff
row sbb_ebp_memory, 0x3f0, 0x83,0x5d,0x00,0xff

.macro fault_read name, at, opcode
  .org \at
  .global \name
  .type \name,@function
\name:
  add ebx, 1
\name\()_fault:
  .byte \opcode,0x10
  jmp \name\()_done
\name\()_done:
  ud2
  .size \name, .-\name
.endm
fault_read adc_read_fault, 0x400, 0x13
fault_read sbb_read_fault, 0x420, 0x1b

.macro fault_store name, at, modrm
  .org \at
  .global \name
  .type \name,@function
\name:
  add ebx, 1
\name\()_fault:
  .byte 0x83,\modrm,1
  jmp \name\()_done
\name\()_done:
  ud2
  .size \name, .-\name
.endm
fault_store adc_store_fault, 0x440, 0x10
fault_store sbb_store_fault, 0x460, 0x18

.macro smc name, at, modrm, immediate
  .org \at
  .global \name
  .type \name,@function
\name:
  add ebx, 1
  .byte 0x83,\modrm,\immediate
\name\()_successor:
  mov ecx, 0x2468ace0
  jmp \name\()_done
\name\()_done:
  ud2
  .size \name, .-\name
.endm
smc adc_smc_same_byte, 0x500, 0x10, 0xff
smc sbb_smc_changed_byte, 0x520, 0x18, 0

.org 0x600
.global carry_branch
.type carry_branch,@function
carry_branch:
  mov eax, 0xffffffff
  add eax, 1
  adc eax, -1
  jc carry_taken
carry_fallthrough:
  mov edx, 0xdeadbeef
carry_taken:
  mov ecx, 0x2468ace0
  jmp carry_done
carry_done:
  ud2
.size carry_branch, .-carry_branch

.org 0x620
.global overflow_branch
.type overflow_branch,@function
overflow_branch:
  mov eax, 0x80000000
  mov edx, 0
  sub edx, 1
  sbb eax, 0
  jo overflow_taken
overflow_fallthrough:
  mov ebx, 0xdeadbeef
overflow_taken:
  mov ecx, 0x2468ace0
  jmp overflow_done
overflow_done:
  ud2
.size overflow_branch, .-overflow_branch
