.intel_syntax noprefix
.code32
.text

.globl direct_caller
.type direct_caller, @function
direct_caller:
    call .Ldirect_gate
    add eax, 7
    nop
.size direct_caller, .-direct_caller

.org 0x100, 0x90
.globl gate_marker
.type gate_marker, @function
gate_marker:
.Ldirect_gate:
    ud2
.size gate_marker, .-gate_marker

.globl indirect_caller
.type indirect_caller, @function
indirect_caller:
    call DWORD PTR [ebx]
    add eax, 7
    nop
.size indirect_caller, .-indirect_caller

.globl scripted_return
.type scripted_return, @function
scripted_return:
    ret
.size scripted_return, .-scripted_return
