.intel_syntax noprefix
.code32
.text

.macro fixture name
.globl \name
.type \name, @function
\name:
.endm
.macro finish name
.size \name, .-\name
.endm

fixture cmp_reg
    cmp DWORD PTR [esi], eax
finish cmp_reg

fixture cmp_reg_entry
    cmp DWORD PTR [esi], eax
    jmp .Lcmp_reg_next
finish cmp_reg_entry
.Lcmp_reg_next:

fixture cmp_reg_prefix
    mov edx, 0xffffffff
    add edx, 1
    cmp DWORD PTR [esi], eax
    mov ecx, 0x99999999
finish cmp_reg_prefix

fixture cmp_imm32
    cmp DWORD PTR [esi], 0x80000001
finish cmp_imm32

fixture cmp_imm32_entry
    cmp DWORD PTR [esi], 0x80000001
    jmp .Lcmp_imm32_next
finish cmp_imm32_entry
.Lcmp_imm32_next:

fixture cmp_imm32_prefix
    mov edx, 0xffffffff
    add edx, 1
    cmp DWORD PTR [esi], 0x80000001
    mov ecx, 0x99999999
finish cmp_imm32_prefix

fixture cmp_imm8
    cmp DWORD PTR [esi], -1
finish cmp_imm8

fixture cmp_imm8_entry
    cmp DWORD PTR [esi], -1
    jmp .Lcmp_imm8_next
finish cmp_imm8_entry
.Lcmp_imm8_next:

fixture cmp_imm8_prefix
    mov edx, 0xffffffff
    add edx, 1
    cmp DWORD PTR [esi], -1
    mov ecx, 0x99999999
finish cmp_imm8_prefix

fixture test_reg
    test DWORD PTR [esi], eax
finish test_reg

fixture test_reg_entry
    test DWORD PTR [esi], eax
    jmp .Ltest_reg_next
finish test_reg_entry
.Ltest_reg_next:

fixture test_reg_prefix
    mov edx, 0xffffffff
    add edx, 1
    test DWORD PTR [esi], eax
    mov ecx, 0x99999999
finish test_reg_prefix

fixture test_imm32
    test DWORD PTR [esi], 0x80000001
finish test_imm32

fixture test_imm32_entry
    test DWORD PTR [esi], 0x80000001
    jmp .Ltest_imm32_next
finish test_imm32_entry
.Ltest_imm32_next:

fixture test_imm32_prefix
    mov edx, 0xffffffff
    add edx, 1
    test DWORD PTR [esi], 0x80000001
    mov ecx, 0x99999999
finish test_imm32_prefix

fixture cmp_base_eax
    cmp DWORD PTR [eax], eax
finish cmp_base_eax

fixture cmp_sib_eax
    cmp DWORD PTR [eax + eax*2 - 4], eax
finish cmp_sib_eax

fixture cmp_base_ecx
    cmp DWORD PTR [ecx], ecx
finish cmp_base_ecx

fixture cmp_sib_ecx
    cmp DWORD PTR [ecx + ecx*2 - 4], ecx
finish cmp_sib_ecx

fixture cmp_base_edx
    cmp DWORD PTR [edx], edx
finish cmp_base_edx

fixture cmp_sib_edx
    cmp DWORD PTR [edx + edx*2 - 4], edx
finish cmp_sib_edx

fixture cmp_base_ebx
    cmp DWORD PTR [ebx], ebx
finish cmp_base_ebx

fixture cmp_sib_ebx
    cmp DWORD PTR [ebx + ebx*2 - 4], ebx
finish cmp_sib_ebx

fixture cmp_base_esp
    cmp DWORD PTR [esp], esp
finish cmp_base_esp

fixture cmp_sib_esp
    cmp DWORD PTR [esp + ecx*4 - 4], esp
finish cmp_sib_esp

fixture cmp_base_ebp
    cmp DWORD PTR [ebp], ebp
finish cmp_base_ebp

fixture cmp_sib_ebp
    cmp DWORD PTR [ebp + ebp*2 - 4], ebp
finish cmp_sib_ebp

fixture cmp_base_esi
    cmp DWORD PTR [esi], esi
finish cmp_base_esi

fixture cmp_sib_esi
    cmp DWORD PTR [esi + esi*2 - 4], esi
finish cmp_sib_esi

fixture cmp_base_edi
    cmp DWORD PTR [edi], edi
finish cmp_base_edi

fixture cmp_sib_edi
    cmp DWORD PTR [edi + edi*2 - 4], edi
finish cmp_sib_edi

fixture test_base_eax
    test DWORD PTR [eax], eax
finish test_base_eax

fixture test_sib_eax
    test DWORD PTR [eax + eax*2 - 4], eax
finish test_sib_eax

fixture test_base_ecx
    test DWORD PTR [ecx], ecx
finish test_base_ecx

fixture test_sib_ecx
    test DWORD PTR [ecx + ecx*2 - 4], ecx
finish test_sib_ecx

fixture test_base_edx
    test DWORD PTR [edx], edx
finish test_base_edx

fixture test_sib_edx
    test DWORD PTR [edx + edx*2 - 4], edx
finish test_sib_edx

fixture test_base_ebx
    test DWORD PTR [ebx], ebx
finish test_base_ebx

fixture test_sib_ebx
    test DWORD PTR [ebx + ebx*2 - 4], ebx
finish test_sib_ebx

fixture test_base_esp
    test DWORD PTR [esp], esp
finish test_base_esp

fixture test_sib_esp
    test DWORD PTR [esp + ecx*4 - 4], esp
finish test_sib_esp

fixture test_base_ebp
    test DWORD PTR [ebp], ebp
finish test_base_ebp

fixture test_sib_ebp
    test DWORD PTR [ebp + ebp*2 - 4], ebp
finish test_sib_ebp

fixture test_base_esi
    test DWORD PTR [esi], esi
finish test_base_esi

fixture test_sib_esi
    test DWORD PTR [esi + esi*2 - 4], esi
finish test_sib_esi

fixture test_base_edi
    test DWORD PTR [edi], edi
finish test_base_edi

fixture test_sib_edi
    test DWORD PTR [edi + edi*2 - 4], edi
finish test_sib_edi

fixture cmp_reg_absolute
    cmp DWORD PTR [0x4000], eax
finish cmp_reg_absolute

fixture cmp_reg_no_base
    cmp DWORD PTR [ecx*4 + 0x4000], eax
finish cmp_reg_no_base

fixture cmp_reg_disp8_negative
    cmp DWORD PTR [ebx - 128], eax
finish cmp_reg_disp8_negative

fixture cmp_reg_disp8_positive
    cmp DWORD PTR [ebx + 127], eax
finish cmp_reg_disp8_positive

fixture cmp_reg_disp32
    cmp DWORD PTR [ebx + 0x12345], eax
finish cmp_reg_disp32

fixture cmp_reg_base_wrap
    cmp DWORD PTR [ebx + 8], eax
finish cmp_reg_base_wrap

fixture cmp_reg_index_wrap
    cmp DWORD PTR [ebx + ecx*8], eax
finish cmp_reg_index_wrap

fixture cmp_reg_unaligned
    cmp DWORD PTR [ebx + ecx*2 - 1], eax
finish cmp_reg_unaligned

fixture cmp_imm32_absolute
    cmp DWORD PTR [0x4000], 0x80000001
finish cmp_imm32_absolute

fixture cmp_imm32_no_base
    cmp DWORD PTR [ecx*4 + 0x4000], 0x80000001
finish cmp_imm32_no_base

fixture cmp_imm32_disp8_negative
    cmp DWORD PTR [ebx - 128], 0x80000001
finish cmp_imm32_disp8_negative

fixture cmp_imm32_disp8_positive
    cmp DWORD PTR [ebx + 127], 0x80000001
finish cmp_imm32_disp8_positive

fixture cmp_imm32_disp32
    cmp DWORD PTR [ebx + 0x12345], 0x80000001
finish cmp_imm32_disp32

fixture cmp_imm32_base_wrap
    cmp DWORD PTR [ebx + 8], 0x80000001
finish cmp_imm32_base_wrap

fixture cmp_imm32_index_wrap
    cmp DWORD PTR [ebx + ecx*8], 0x80000001
finish cmp_imm32_index_wrap

fixture cmp_imm32_unaligned
    cmp DWORD PTR [ebx + ecx*2 - 1], 0x80000001
finish cmp_imm32_unaligned

fixture cmp_imm8_absolute
    cmp DWORD PTR [0x4000], -1
finish cmp_imm8_absolute

fixture cmp_imm8_no_base
    cmp DWORD PTR [ecx*4 + 0x4000], -1
finish cmp_imm8_no_base

fixture cmp_imm8_disp8_negative
    cmp DWORD PTR [ebx - 128], -1
finish cmp_imm8_disp8_negative

fixture cmp_imm8_disp8_positive
    cmp DWORD PTR [ebx + 127], -1
finish cmp_imm8_disp8_positive

fixture cmp_imm8_disp32
    cmp DWORD PTR [ebx + 0x12345], -1
finish cmp_imm8_disp32

fixture cmp_imm8_base_wrap
    cmp DWORD PTR [ebx + 8], -1
finish cmp_imm8_base_wrap

fixture cmp_imm8_index_wrap
    cmp DWORD PTR [ebx + ecx*8], -1
finish cmp_imm8_index_wrap

fixture cmp_imm8_unaligned
    cmp DWORD PTR [ebx + ecx*2 - 1], -1
finish cmp_imm8_unaligned

fixture test_reg_absolute
    test DWORD PTR [0x4000], eax
finish test_reg_absolute

fixture test_reg_no_base
    test DWORD PTR [ecx*4 + 0x4000], eax
finish test_reg_no_base

fixture test_reg_disp8_negative
    test DWORD PTR [ebx - 128], eax
finish test_reg_disp8_negative

fixture test_reg_disp8_positive
    test DWORD PTR [ebx + 127], eax
finish test_reg_disp8_positive

fixture test_reg_disp32
    test DWORD PTR [ebx + 0x12345], eax
finish test_reg_disp32

fixture test_reg_base_wrap
    test DWORD PTR [ebx + 8], eax
finish test_reg_base_wrap

fixture test_reg_index_wrap
    test DWORD PTR [ebx + ecx*8], eax
finish test_reg_index_wrap

fixture test_reg_unaligned
    test DWORD PTR [ebx + ecx*2 - 1], eax
finish test_reg_unaligned

fixture test_imm32_absolute
    test DWORD PTR [0x4000], 0x80000001
finish test_imm32_absolute

fixture test_imm32_no_base
    test DWORD PTR [ecx*4 + 0x4000], 0x80000001
finish test_imm32_no_base

fixture test_imm32_disp8_negative
    test DWORD PTR [ebx - 128], 0x80000001
finish test_imm32_disp8_negative

fixture test_imm32_disp8_positive
    test DWORD PTR [ebx + 127], 0x80000001
finish test_imm32_disp8_positive

fixture test_imm32_disp32
    test DWORD PTR [ebx + 0x12345], 0x80000001
finish test_imm32_disp32

fixture test_imm32_base_wrap
    test DWORD PTR [ebx + 8], 0x80000001
finish test_imm32_base_wrap

fixture test_imm32_index_wrap
    test DWORD PTR [ebx + ecx*8], 0x80000001
finish test_imm32_index_wrap

fixture test_imm32_unaligned
    test DWORD PTR [ebx + ecx*2 - 1], 0x80000001
finish test_imm32_unaligned

fixture cmp_imm8_min
    cmp DWORD PTR [esi], -128
finish cmp_imm8_min

fixture cmp_imm8_minus1
    cmp DWORD PTR [esi], -1
finish cmp_imm8_minus1

fixture cmp_imm8_zero
    cmp DWORD PTR [esi], 0
finish cmp_imm8_zero

fixture cmp_imm8_max
    cmp DWORD PTR [esi], 127
finish cmp_imm8_max

fixture cmp_imm32_high
    cmp DWORD PTR [esi], 0x80000000
finish cmp_imm32_high

fixture cmp_imm32_max
    cmp DWORD PTR [esi], 0x7fffffff
finish cmp_imm32_max

fixture cmp_imm32_literal
    cmp DWORD PTR [esi], 0x12345678
finish cmp_imm32_literal

fixture cmp_imm32_minus1
    .byte 0x81, 0x3e
    .long 0xffffffff
finish cmp_imm32_minus1

fixture test_imm32_zero
    test DWORD PTR [esi], 0
finish test_imm32_zero

fixture test_imm32_minus1
    test DWORD PTR [esi], 0xffffffff
finish test_imm32_minus1

fixture test_imm32_high
    test DWORD PTR [esi], 0x80000000
finish test_imm32_high

fixture test_imm32_max
    test DWORD PTR [esi], 0x7fffffff
finish test_imm32_max

fixture test_imm32_literal
    test DWORD PTR [esi], 0x12345678
finish test_imm32_literal

.org 0x600, 0x90
.Lcompare:
fixture scan_compare
    cmp DWORD PTR [esi], eax
    jb .Lskip
finish scan_compare

fixture scan_test
    test DWORD PTR [esi + 4], ebx
    jz .Lskip
finish scan_test

fixture scan_accept
    add edx, 1
    jmp .Ljoined
finish scan_accept

.org 0x680, 0x90
.Lskip:
fixture scan_skip
    jmp .Ljoined
finish scan_skip

.org 0x6c0, 0x90
.Ljoined:
fixture scan_joined
    lea esi, [esi + 8]
    sub ecx, 1
    jnz .Lcompare
finish scan_joined

fixture scan_finish
    jmp .Ldone
finish scan_finish

.org 0x700, 0x90
.Ldone:
fixture scan_done
    ud2
finish scan_done
