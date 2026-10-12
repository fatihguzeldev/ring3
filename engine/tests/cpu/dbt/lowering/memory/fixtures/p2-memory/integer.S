.intel_syntax noprefix
.code32
.text

.macro fixture name
.globl \name
.type \name, @function
\name:
.endm

.macro finish name
.size \name, .-\name
.endm

fixture load_ind
mov eax, dword ptr [ebx]
finish load_ind
fixture store_ind
mov dword ptr [ebx], eax
finish store_ind
fixture load_alias
mov eax, dword ptr [eax]
finish load_alias
fixture store_alias
mov dword ptr [ebx], ebx
finish store_alias
fixture load_sib
mov eax, dword ptr [ebx + esi * 4 - 4]
finish load_sib
fixture store_sib
mov dword ptr [ebx + ecx * 4 + 16], edx
finish store_sib
fixture load_no_base
mov eax, dword ptr [esi * 4 + 0x4010]
finish load_no_base
fixture load_esp
mov eax, dword ptr [esp]
finish load_esp
fixture load_ebp
mov eax, dword ptr [ebp]
finish load_ebp
fixture load_disp32
mov eax, dword ptr [ebx + 0x1000]
finish load_disp32
fixture load_absolute
mov eax, dword ptr [0x4010]
finish load_absolute
fixture store_absolute
mov dword ptr [0x4014], eax
finish store_absolute
fixture store_imm
mov dword ptr [ebx + 4], 0xdeadbeef
finish store_imm
fixture store_imm_sib
mov dword ptr [ebx + ecx * 4 + 16], 0x12345678
finish store_imm_sib
fixture load_wrap
mov eax, dword ptr [ebx + esi * 4]
finish load_wrap

fixture ram_sum
mov eax, 0
sum_guard:
cmp ecx, 0
je sum_finish
mov edx, dword ptr [esi]
add eax, edx
add esi, 4
sub ecx, 1
jmp sum_guard
sum_finish:
mov dword ptr [edi], eax
finish ram_sum
nop

fixture smc
mov dword ptr [0x100b], 0x44332211
mov ecx, 0xaaaaaaaa
finish smc
nop
