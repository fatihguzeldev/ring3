.intel_syntax noprefix
.code32
.text

.macro push_args count
.if \count == 16
    push 0xcafebabe
    push 0x12345678
    push 0xfedcba98
    push 0xabcdef01
    push 0x80
    push -128
    push 7
    push 42
    push 0xdeadbeef
    push 0x55aa55aa
    push 0x11223344
    push 0x01020304
    push 0x7fffffff
    push 0x80000000
.endif
.if \count >= 2
    push -1
    push 0
.endif
.endm

.macro caller name, convention, count
.balign 256, 0x90
.globl \name
.type \name, @function
\name:
.if \convention == 3
    mov ecx, 0xf1234567
.endif
    push_args \count
.if \convention == 1
    call .Lgate_\name
.elseif \convention == 2
    call DWORD PTR [ebx]
.else
    call esi
.endif
.if \convention == 1 && \count > 0
    add esp, 4 * \count
.endif
    nop
.size \name, .-\name
.space 256 - (. - \name), 0x90
.globl \name\()_gate
.type \name\()_gate, @function
\name\()_gate:
.Lgate_\name:
    ud2
.size \name\()_gate, .-\name\()_gate
.endm

caller cdecl_0, 1, 0
caller cdecl_2, 1, 2
caller cdecl_16, 1, 16
caller stdcall_0, 2, 0
caller stdcall_2, 2, 2
caller stdcall_16, 2, 16
caller thiscall_0, 3, 0
caller thiscall_2, 3, 2
caller thiscall_16, 3, 16
