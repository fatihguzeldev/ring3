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

fixture call_eax
call eax
finish call_eax
fixture call_esp
call esp
finish call_esp
fixture call_esi
call esi
finish call_esi
fixture call_ebx
call dword ptr [ebx]
finish call_ebx
fixture call_esp_memory
call dword ptr [esp]
finish call_esp_memory
fixture call_esp_minus4
call dword ptr [esp - 4]
finish call_esp_minus4
fixture call_sib
call dword ptr [ebx + ecx * 4 - 4]
finish call_sib
fixture jmp_eax
jmp eax
finish jmp_eax
fixture jmp_esp
jmp esp
finish jmp_esp
fixture jmp_ebx
jmp dword ptr [ebx]
finish jmp_ebx
fixture jmp_sib
jmp dword ptr [ebx + ecx * 4 - 4]
finish jmp_sib
fixture jmp_wrap
jmp dword ptr [ebx + esi * 4]
finish jmp_wrap

fixture function_pointer
call dword ptr [ebx]
nop
add eax, ecx
jmp edx
sub eax, ecx
jmp edx
ret
finish function_pointer
