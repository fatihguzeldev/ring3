.intel_syntax noprefix
.code32
.text

.globl relocated_data
.type relocated_data, @function
relocated_data:
    mov edx, dword ptr [0x00403000]
    mov eax, dword ptr [edx]
    mov ecx, dword ptr [0x00403200]
    lea eax, [eax + 5]
    mov dword ptr [0x00403200], eax
    jmp relocated_done
relocated_done:
    ud2
.size relocated_data, .-relocated_data
