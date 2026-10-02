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

.macro register_forms number, register
fixture push_\number
push \register
finish push_\number
fixture push_modrm_\number
.byte 0xff, 0xf0 + \number
finish push_modrm_\number
fixture pop_\number
pop \register
finish pop_\number
fixture pop_modrm_\number
.byte 0x8f, 0xc0 + \number
finish pop_modrm_\number
.endm

register_forms 0, eax
register_forms 1, ecx
register_forms 2, edx
register_forms 3, ebx
register_forms 4, esp
register_forms 5, ebp
register_forms 6, esi
register_forms 7, edi

fixture imm8_negative128
push -128
finish imm8_negative128
fixture imm8_negative1
push -1
finish imm8_negative1
fixture imm8_zero
push 0
finish imm8_zero
fixture imm8_positive127
push 127
finish imm8_positive127
fixture imm32_negative129
push -129
finish imm32_negative129
fixture imm32_high
push 0x80000000
finish imm32_high
fixture imm32_literal
push 0x12345678
finish imm32_literal

fixture cleanup_0
ret 0
finish cleanup_0
fixture cleanup_1
ret 1
finish cleanup_1
fixture cleanup_4
ret 4
finish cleanup_4
fixture cleanup_65535
ret 65535
finish cleanup_65535

fixture arguments
push ecx
push edx
call callee
nop
callee:
push ebx
mov ebx, dword ptr [esp + 8]
mov eax, dword ptr [esp + 12]
add eax, ebx
pop ebx
ret 8
finish arguments

fixture arithmetic_push
mov eax, 0xffffffff
add eax, 1
push eax
finish arithmetic_push
fixture arithmetic_pop_esp
mov eax, 0xffffffff
add eax, 1
pop esp
finish arithmetic_pop_esp
fixture arithmetic_pop_edi
mov eax, 0xffffffff
add eax, 1
pop edi
finish arithmetic_pop_edi
fixture arithmetic_cleanup
mov eax, 0xffffffff
add eax, 1
ret 65535
finish arithmetic_cleanup

fixture smc_push
push eax
mov ecx, 0xaaaaaaaa
finish smc_push
