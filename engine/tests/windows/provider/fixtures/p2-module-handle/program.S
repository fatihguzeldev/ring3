.intel_syntax noprefix
.code32
.text
.global module_handle_sequence, module_after_set, module_after_handle, module_after_get, module_exit_canary, module_done
.type module_handle_sequence,@function
module_handle_sequence:
    lea esi, [0x00403000]
    mov ecx, dword ptr [esi + 16]
    push ecx
    call dword ptr [0x00404054]
module_after_set:
    push 0
    call dword ptr [0x0040405c]
module_after_handle:
    mov dword ptr [esi], eax
    mov ebx, eax
    movzx edx, word ptr [eax]
    mov dword ptr [esi + 4], edx
    call dword ptr [0x00404050]
module_after_get:
    mov dword ptr [esi + 8], eax
    xor eax, ebx
    xor eax, edx
    mov dword ptr [esi + 12], eax
    push eax
    call dword ptr [0x00404058]
.type module_exit_canary,@function
module_exit_canary:
    mov dword ptr [esi + 20], 0xa5a5a5a5
    jmp .Lmodule_done
module_done:
.Lmodule_done:
    ud2
.size module_exit_canary,module_done-module_exit_canary
.size module_handle_sequence,module_exit_canary-module_handle_sequence
