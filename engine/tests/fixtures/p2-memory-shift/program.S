.intel_syntax noprefix
.text
.code32
.global memory_terminal_sequence, memory_after_jc, memory_after_jo, memory_call, memory_exit_canary, memory_canary_done
.type memory_terminal_sequence,@function
memory_terminal_sequence:
    mov ebx, 0x00403000
    mov ecx, 0xabcd0020
    mov eax, 0xffffffff
    add eax, 1
    shl dword ptr [ebx], cl
    jc .Lafter_jc
    mov edx, 0xdeadbeef
.type memory_after_jc,@function
memory_after_jc:
.Lafter_jc:
    shr dword ptr [ebx + 4], 1
    jo .Lafter_jo
    mov esi, 0xbaadf00d
.size memory_after_jc,memory_after_jo-memory_after_jc
.type memory_after_jo,@function
memory_after_jo:
.Lafter_jo:
    sar dword ptr [ebx + 8], 2
    mov eax, dword ptr [ebx]
    xor eax, dword ptr [ebx + 4]
    xor eax, dword ptr [ebx + 8]
    mov dword ptr [ebx + 12], eax
    push eax
memory_call:
    call dword ptr [0x00404050]
.size memory_after_jo,memory_exit_canary-memory_after_jo
.type memory_exit_canary,@function
memory_exit_canary:
    mov dword ptr [ebx + 20], 0xdeadbeef
    jmp .Lcanary_done
memory_canary_done:
.Lcanary_done:
    ud2
.size memory_exit_canary,memory_canary_done-memory_exit_canary
.size memory_terminal_sequence,.-memory_terminal_sequence

.macro finish name
    jmp .L\name\()_done
    .global \name\()_done
\name\()_done:
.L\name\()_done:
    ud2
    .size \name,.-\name
.endm

.org 0x80, 0xcc
.global finite_f1
.type finite_f1,@function
finite_f1:
    shl dword ptr [ebx], 1
finish finite_f1
.org 0x90, 0xcc
.global finite_f2
.type finite_f2,@function
finite_f2:
    shr dword ptr [ebx + 4], 1
finish finite_f2
.org 0xa0, 0xcc
.global finite_f3
.type finite_f3,@function
finite_f3:
    sar dword ptr [esp + 0x24], 1
finish finite_f3
.org 0xb0, 0xcc
.global finite_f4
.type finite_f4,@function
finite_f4:
    shl dword ptr [0x2004], 31
finish finite_f4
.org 0xc0, 0xcc
.global finite_f5
.type finite_f5,@function
finite_f5:
    shr dword ptr [ebp - 4], 33
finish finite_f5
.org 0xd0, 0xcc
.global finite_f6
.type finite_f6,@function
finite_f6:
    sar dword ptr [esi + edi * 2 + 8], 255
finish finite_f6
.org 0xe0, 0xcc
.global finite_f7
.type finite_f7,@function
finite_f7:
    shl dword ptr [ebx + ecx * 2 + 0x6410b94d], cl
finish finite_f7
.org 0xf0, 0xcc
.global finite_f8
.type finite_f8,@function
finite_f8:
    shr dword ptr [ecx + 0x54332002], cl
finish finite_f8
.org 0x100, 0xcc
.global finite_f9
.type finite_f9,@function
finite_f9:
    sar dword ptr [eax + ecx + 0x4310ecc0], cl
finish finite_f9

.macro prefix name, at
    .org \at, 0xcc
    .global \name, \name\()_shift
    .type \name,@function
\name:
    lea edx, [edx + 1]
\name\()_shift:
.endm

prefix micro_m1, 0x120
    shl dword ptr [ebx + ecx * 2 - 2], cl
finish micro_m1
prefix micro_m2, 0x140
    sar dword ptr [esp + 8], 2
finish micro_m2
prefix micro_m3, 0x160
    shr dword ptr [0x4000], 0
finish micro_m3
prefix micro_m4, 0x180
    shl dword ptr [edi], 1
finish micro_m4

prefix smc_one, 0x1a0
    .byte 0xc1, 0x25
    .long 0x8f00
    .byte 1
.global smc_one_canary
smc_one_canary:
    mov edi, 0xdeadbeef
finish smc_one
prefix smc_zero, 0x1c0
    shl dword ptr [0x8f00], 32
.global smc_zero_canary
smc_zero_canary:
    mov edi, 0xdeadbeef
finish smc_zero
