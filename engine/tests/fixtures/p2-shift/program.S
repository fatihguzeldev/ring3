.intel_syntax noprefix
.text

.global masked_zero_chain
.type masked_zero_chain,@function
masked_zero_chain:
  mov eax, 0xffffffff
  add eax, 1
masked_zero_after_add1:
  mov ecx, 0xabcd0020
  shl eax, cl
masked_zero_after_zero1:
  jc masked_zero_cf_taken
masked_zero_cf_fallthrough:
  mov edx, 0xdeadbeef
masked_zero_cf_taken:
  mov ebx, 0x7fffffff
  add ebx, 1
masked_zero_after_add2:
  shr ebx, 0x20
masked_zero_after_zero2:
  jo masked_zero_of_taken
masked_zero_of_fallthrough:
  mov esi, 0xdeadbeef
masked_zero_of_taken:
  sar ebx, 0
masked_zero_after_zero3:
  jmp masked_zero_done
masked_zero_done:
  ud2
.size masked_zero_chain, .-masked_zero_chain

.org 0x100
.global count_alias_chain
.type count_alias_chain,@function
count_alias_chain:
  mov eax, 0x80000001
  shl eax, 1
count_alias_after_shl1:
  mov edx, 0x80000001
  shr edx, 1
count_alias_after_shr1:
  mov ebx, 0x80000001
  sar ebx, 1
count_alias_after_sar1:
  mov ecx, 0x80000021
  sar ecx, cl
count_alias_after_self:
  mov esi, 0xfffffff7
  sar esi, 2
count_alias_after_sar2:
  mov edi, 1
  shl edi, 0x1f
count_alias_after_shl31:
  jmp count_alias_done
count_alias_done:
  ud2
.size count_alias_chain, .-count_alias_chain

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

row shl_one_eax, 0x200, 0xd1,0xe0
row shr_one_edx, 0x210, 0xd1,0xea
row sar_one_ebx, 0x220, 0xd1,0xfb
row shl_imm31_esi, 0x230, 0xc1,0xe6,0x1f
row shr_imm33_edi, 0x240, 0xc1,0xef,0x21
row sar_imm255_ebp, 0x250, 0xc1,0xfd,0xff
row shl_cl_self, 0x260, 0xd3,0xe1
row shr_cl_esp, 0x270, 0xd3,0xec
row sar_cl_eax, 0x280, 0xd3,0xf8
