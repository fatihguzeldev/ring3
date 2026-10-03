.intel_syntax noprefix
.code32
.text

.globl windows_sequence
.type windows_sequence, @function
windows_sequence:
    call .Lget_gate
    mov ebx, eax
    push 0xf1234567
    call .Lset_gate
    mov esi, eax
    call .Lget_gate
    mov edi, eax
    push 0
    call .Lset_gate
    mov ebp, eax
    call .Lget_gate
    lea eax, [eax + 7]
    jmp sequence_done
sequence_done:
    ud2
.size windows_sequence, .-windows_sequence

.org 0x1000
.globl get_gate
.type get_gate, @function
get_gate:
.Lget_gate:
    ud2
.size get_gate, .-get_gate

.org 0x1100
.globl set_gate
.type set_gate, @function
set_gate:
.Lset_gate:
    ud2
.size set_gate, .-set_gate
