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

fixture push_ebx
push dword ptr [ebx]
finish push_ebx
fixture push_esp
push dword ptr [esp]
finish push_esp
fixture push_esp_minus4
push dword ptr [esp - 4]
finish push_esp_minus4
fixture push_sib
push dword ptr [ebx + ecx * 4 - 8]
finish push_sib
fixture pop_ebx
pop dword ptr [ebx]
finish pop_ebx
fixture pop_esp
pop dword ptr [esp]
finish pop_esp
fixture pop_esp_minus4
pop dword ptr [esp - 4]
finish pop_esp_minus4
fixture pop_sib
pop dword ptr [ebx + ecx * 4 + 16]
finish pop_sib
fixture pop_esp_sib
pop dword ptr [esp + ecx * 4 + 8]
finish pop_esp_sib
fixture pop_esp_minus8
pop dword ptr [esp - 8]
finish pop_esp_minus8

fixture transfer
push dword ptr [esi]
push dword ptr [ebx]
push dword ptr [ebx + 4]
pop dword ptr [edi]
pop dword ptr [edi + 4]
pop dword ptr [esi]
mov eax, dword ptr [edi]
add eax, ebp
finish transfer

fixture smc_push
push dword ptr [ebx]
mov ecx, 0xbbbbbbbb
finish smc_push
fixture smc_pop
pop dword ptr [ebx]
mov ecx, 0xbbbbbbbb
finish smc_pop
