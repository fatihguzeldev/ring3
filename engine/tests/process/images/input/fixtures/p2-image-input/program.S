.intel_syntax noprefix
.code32
.text

.globl linked_terminal_sequence
.type linked_terminal_sequence, @function
linked_terminal_sequence:
    call DWORD PTR [0x00404050]
    mov ebx, eax
    push 0x89abcdef
    call DWORD PTR [0x00404054]
    call DWORD PTR [0x00404050]
    mov esi, eax
    push 0xf1234567
    call DWORD PTR [0x00404058]

.globl exit_canary
.type exit_canary, @function
exit_canary:
    mov eax, 0xdeadbeef
    mov DWORD PTR [0x00403000], eax
    jmp .Ldone
.Ldone:
    ud2
.size exit_canary, .-exit_canary
.size linked_terminal_sequence, .-linked_terminal_sequence
