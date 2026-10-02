.intel_syntax noprefix
.code32
.text

.globl outer_caller
.type outer_caller, @function
outer_caller:
    push -1
    push 0
    call .Louter_gate
    nop
    jmp .Lunknown
.size outer_caller, .-outer_caller

.org 0x100, 0x90
.globl outer_gate
.type outer_gate, @function
outer_gate:
.Louter_gate:
    ud2
.size outer_gate, .-outer_gate

.org 0x200, 0x90
.globl inner_gate
.type inner_gate, @function
inner_gate:
.Linner_gate:
    ud2
.size inner_gate, .-inner_gate

.org 0x300, 0x90
.globl callback_return_gate
.type callback_return_gate, @function
callback_return_gate:
    ud2
.size callback_return_gate, .-callback_return_gate

.org 0x400, 0x90
.globl callback_entry
.type callback_entry, @function
callback_entry:
    mov eax, DWORD PTR [esp + 4]
    mov edx, DWORD PTR [esp + 8]
    add eax, edx
    call .Lcallee
.size callback_entry, .-callback_entry

.globl callback_after_callee
.type callback_after_callee, @function
callback_after_callee:
    push eax
    call .Linner_gate
.size callback_after_callee, .-callback_after_callee

.globl callback_after_inner
.type callback_after_inner, @function
callback_after_inner:
    add eax, 1
    ret 8
.size callback_after_inner, .-callback_after_inner

.org 0x600, 0x90
.globl unsupported_integer
.type unsupported_integer, @function
unsupported_integer:
    inc eax
.size unsupported_integer, .-unsupported_integer

.org 0x3000, 0x90
.globl absent_callee
.type absent_callee, @function
absent_callee:
.Lcallee:
    add eax, 3
    ret
.size absent_callee, .-absent_callee

.org 0x3100, 0x90
.Lunknown:
    ud2
