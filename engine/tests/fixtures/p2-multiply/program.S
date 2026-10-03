.intel_syntax noprefix
.text

.global overflow_branch_chain
.type overflow_branch_chain,@function
overflow_branch_chain:
  mov eax, 0x80000000
  mov ecx, 0xffffffff
  imul eax, ecx
overflow_after_first:
  jo overflow_first_taken
overflow_first_fallthrough:
  mov edx, 0xdeadbeef
overflow_first_taken:
  imul ebx, eax, -1
overflow_after_second:
  jc overflow_second_taken
overflow_second_fallthrough:
  mov esi, 0xdeadbeef
overflow_second_taken:
  imul ebp, ebx, 1
overflow_after_third:
  jno overflow_third_taken
overflow_third_fallthrough:
  mov edi, 0xdeadbeef
overflow_third_taken:
  jmp overflow_done
overflow_done:
  ud2
.size overflow_branch_chain, .-overflow_branch_chain

.org 0x100
.global source_alias_chain
.type source_alias_chain,@function
source_alias_chain:
  mov eax, 0xfffffff9
  mov edx, 6
  imul eax, edx
source_after_two:
  imul ecx, eax, 0x7f
source_after_7f:
  imul ebx, ecx, -128
source_after_80:
  imul esi, ecx, 0x80000000
source_after_min:
  .byte 0x69,0xfe,0xff,0xff,0xff,0xff
source_after_full_ff:
  mov ebp, 0x10000
  imul ebp, ebp
source_after_square:
  mov esp, 0xfffffff7
  imul esp, esp, -1
source_after_esp:
  jmp source_done
source_done:
  ud2
.size source_alias_chain, .-source_alias_chain

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

row imul_eax_edx, 0x200, 0x0f,0xaf,0xc2
row imul_ecx_ebx, 0x210, 0x0f,0xaf,0xcb
row imul_edx_ecx, 0x220, 0x0f,0xaf,0xd1
row imul_ebx_eax, 0x230, 0x0f,0xaf,0xd8
row imul_esp_self, 0x240, 0x0f,0xaf,0xe4
row imul_ebp_self, 0x250, 0x0f,0xaf,0xed
row imul_esi_edi, 0x260, 0x0f,0xaf,0xf7
row imul_edi_esi, 0x270, 0x0f,0xaf,0xfe
row imul_7f_eax_ecx, 0x280, 0x6b,0xc1,0x7f
row imul_80_ecx_self, 0x290, 0x6b,0xc9,0x80
row imul_ff_esp_self, 0x2a0, 0x6b,0xe4,0xff
row imul_zero_ebx_edx, 0x2b0, 0x6b,0xda,0x00
row imul_min_esi_ecx, 0x2c0, 0x69,0xf1,0x00,0x00,0x00,0x80
row imul_max_edi_esi, 0x2d0, 0x69,0xfe,0xff,0xff,0xff,0x7f
row imul_full_ff_ebp_ebx, 0x2e0, 0x69,0xeb,0xff,0xff,0xff,0xff
