.intel_syntax noprefix
.code32
.text
.globl baseline_entry
.type baseline_entry, @function
baseline_entry:
    mov eax, 0
baseline_guard:
    cmp ecx, 0
    je baseline_done
baseline_body:
    add eax, ecx
    sub ecx, 1
    jmp baseline_guard
baseline_done:
    nop
.size baseline_entry, .-baseline_entry
