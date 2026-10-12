.intel_syntax noprefix
.code32
.text

.globl exit_sequence
.type exit_sequence, @function
exit_sequence:
    mov eax, 0x13579bdf
    push 0xf1234567
    call .Lexit_gate

.globl exit_canary
.type exit_canary, @function
exit_canary:
    mov eax, 0xdeadbeef
    mov DWORD PTR [0x00403000], eax
    jmp .Ldone
.Ldone:
    ud2
.size exit_canary, .-exit_canary
.size exit_sequence, .-exit_sequence

.org 0x100
.globl exit_gate
.type exit_gate, @function
exit_gate:
.Lexit_gate:
    ud2
.size exit_gate, .-exit_gate
