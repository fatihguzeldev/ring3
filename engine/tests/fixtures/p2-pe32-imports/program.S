.intel_syntax noprefix
.code32
.text

.globl imported_windows_sequence
.type imported_windows_sequence, @function
imported_windows_sequence:
    call DWORD PTR [0x00404050]
    mov ebx, eax
    push 0xf1234567
    call DWORD PTR [0x00404054]
    mov esi, eax
    call DWORD PTR [0x00404050]
    mov edi, eax
    mov DWORD PTR [0x00403000], eax
    push 0
    call DWORD PTR [0x00404054]
    mov ebp, eax
    call DWORD PTR [0x00404050]
    lea eax, [eax + 7]
    jmp sequence_done
sequence_done:
    ud2
.size imported_windows_sequence, .-imported_windows_sequence
