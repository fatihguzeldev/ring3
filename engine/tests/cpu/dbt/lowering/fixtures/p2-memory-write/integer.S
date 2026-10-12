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

fixture add_reg
    add DWORD PTR [esi], eax
finish add_reg

fixture add_reg_entry
    add DWORD PTR [esi], eax
    jmp .Ladd_reg_next
finish add_reg_entry

.Ladd_reg_next:
fixture add_reg_prefix
    mov edx, 0xffffffff
    add edx, 1
    add DWORD PTR [esi], eax
    mov ecx, 0x99999999
finish add_reg_prefix

fixture add_reg_smc
    add DWORD PTR [esi], eax
    mov ecx, 0xbbbbbbbb
finish add_reg_smc

fixture add_reg_absolute
    add DWORD PTR [0x4000], eax
finish add_reg_absolute

fixture add_reg_no_base
    add DWORD PTR [ecx*4 + 0x4000], eax
finish add_reg_no_base

fixture add_reg_disp8_negative
    add DWORD PTR [ebx - 128], eax
finish add_reg_disp8_negative

fixture add_reg_disp8_positive
    add DWORD PTR [ebx + 127], eax
finish add_reg_disp8_positive

fixture add_reg_disp32
    add DWORD PTR [ebx + 0x12345], eax
finish add_reg_disp32

fixture add_reg_base_wrap
    add DWORD PTR [ebx + 8], eax
finish add_reg_base_wrap

fixture add_reg_index_wrap
    add DWORD PTR [ebx + ecx*8], eax
finish add_reg_index_wrap

fixture add_reg_unaligned
    add DWORD PTR [ebx + ecx*2 - 1], eax
finish add_reg_unaligned

fixture add_imm32
    add DWORD PTR [esi], 0x80000001
finish add_imm32

fixture add_imm32_entry
    add DWORD PTR [esi], 0x80000001
    jmp .Ladd_imm32_next
finish add_imm32_entry

.Ladd_imm32_next:
fixture add_imm32_prefix
    mov edx, 0xffffffff
    add edx, 1
    add DWORD PTR [esi], 0x80000001
    mov ecx, 0x99999999
finish add_imm32_prefix

fixture add_imm32_smc
    add DWORD PTR [esi], 0x80000001
    mov ecx, 0xbbbbbbbb
finish add_imm32_smc

fixture add_imm32_absolute
    add DWORD PTR [0x4000], 0x80000001
finish add_imm32_absolute

fixture add_imm32_no_base
    add DWORD PTR [ecx*4 + 0x4000], 0x80000001
finish add_imm32_no_base

fixture add_imm32_disp8_negative
    add DWORD PTR [ebx - 128], 0x80000001
finish add_imm32_disp8_negative

fixture add_imm32_disp8_positive
    add DWORD PTR [ebx + 127], 0x80000001
finish add_imm32_disp8_positive

fixture add_imm32_disp32
    add DWORD PTR [ebx + 0x12345], 0x80000001
finish add_imm32_disp32

fixture add_imm32_base_wrap
    add DWORD PTR [ebx + 8], 0x80000001
finish add_imm32_base_wrap

fixture add_imm32_index_wrap
    add DWORD PTR [ebx + ecx*8], 0x80000001
finish add_imm32_index_wrap

fixture add_imm32_unaligned
    add DWORD PTR [ebx + ecx*2 - 1], 0x80000001
finish add_imm32_unaligned

fixture add_imm8
    add DWORD PTR [esi], -1
finish add_imm8

fixture add_imm8_entry
    add DWORD PTR [esi], -1
    jmp .Ladd_imm8_next
finish add_imm8_entry

.Ladd_imm8_next:
fixture add_imm8_prefix
    mov edx, 0xffffffff
    add edx, 1
    add DWORD PTR [esi], -1
    mov ecx, 0x99999999
finish add_imm8_prefix

fixture add_imm8_smc
    add DWORD PTR [esi], -1
    mov ecx, 0xbbbbbbbb
finish add_imm8_smc

fixture add_imm8_absolute
    add DWORD PTR [0x4000], -1
finish add_imm8_absolute

fixture add_imm8_no_base
    add DWORD PTR [ecx*4 + 0x4000], -1
finish add_imm8_no_base

fixture add_imm8_disp8_negative
    add DWORD PTR [ebx - 128], -1
finish add_imm8_disp8_negative

fixture add_imm8_disp8_positive
    add DWORD PTR [ebx + 127], -1
finish add_imm8_disp8_positive

fixture add_imm8_disp32
    add DWORD PTR [ebx + 0x12345], -1
finish add_imm8_disp32

fixture add_imm8_base_wrap
    add DWORD PTR [ebx + 8], -1
finish add_imm8_base_wrap

fixture add_imm8_index_wrap
    add DWORD PTR [ebx + ecx*8], -1
finish add_imm8_index_wrap

fixture add_imm8_unaligned
    add DWORD PTR [ebx + ecx*2 - 1], -1
finish add_imm8_unaligned

fixture add_base_eax
    add DWORD PTR [eax], eax
finish add_base_eax

fixture add_sib_eax
    add DWORD PTR [eax + eax*2 - 4], eax
finish add_sib_eax

fixture add_index_eax
    add DWORD PTR [ebx + eax*4 - 4], eax
finish add_index_eax

fixture add_base_ecx
    add DWORD PTR [ecx], ecx
finish add_base_ecx

fixture add_sib_ecx
    add DWORD PTR [ecx + ecx*2 - 4], ecx
finish add_sib_ecx

fixture add_index_ecx
    add DWORD PTR [ebx + ecx*4 - 4], ecx
finish add_index_ecx

fixture add_base_edx
    add DWORD PTR [edx], edx
finish add_base_edx

fixture add_sib_edx
    add DWORD PTR [edx + edx*2 - 4], edx
finish add_sib_edx

fixture add_index_edx
    add DWORD PTR [ebx + edx*4 - 4], edx
finish add_index_edx

fixture add_base_ebx
    add DWORD PTR [ebx], ebx
finish add_base_ebx

fixture add_sib_ebx
    add DWORD PTR [ebx + ebx*2 - 4], ebx
finish add_sib_ebx

fixture add_index_ebx
    add DWORD PTR [ebx + ebx*4 - 4], ebx
finish add_index_ebx

fixture add_base_esp
    add DWORD PTR [esp], esp
finish add_base_esp

fixture add_sib_esp
    add DWORD PTR [esp + ecx*4 - 4], esp
finish add_sib_esp

fixture add_base_ebp
    add DWORD PTR [ebp], ebp
finish add_base_ebp

fixture add_sib_ebp
    add DWORD PTR [ebp + ebp*2 - 4], ebp
finish add_sib_ebp

fixture add_index_ebp
    add DWORD PTR [ebx + ebp*4 - 4], ebp
finish add_index_ebp

fixture add_base_esi
    add DWORD PTR [esi], esi
finish add_base_esi

fixture add_sib_esi
    add DWORD PTR [esi + esi*2 - 4], esi
finish add_sib_esi

fixture add_index_esi
    add DWORD PTR [ebx + esi*4 - 4], esi
finish add_index_esi

fixture add_base_edi
    add DWORD PTR [edi], edi
finish add_base_edi

fixture add_sib_edi
    add DWORD PTR [edi + edi*2 - 4], edi
finish add_sib_edi

fixture add_index_edi
    add DWORD PTR [ebx + edi*4 - 4], edi
finish add_index_edi

fixture add_imm8_min
    add DWORD PTR [esi], -128
finish add_imm8_min

fixture add_imm8_minus1
    add DWORD PTR [esi], -1
finish add_imm8_minus1

fixture add_imm8_zero
    add DWORD PTR [esi], 0
finish add_imm8_zero

fixture add_imm8_max
    add DWORD PTR [esi], 127
finish add_imm8_max

fixture add_imm32_max
    add DWORD PTR [esi], 0x7fffffff
finish add_imm32_max

fixture add_imm32_literal
    add DWORD PTR [esi], 0x12345678
finish add_imm32_literal

fixture add_imm32_minus1
    .byte 0x81, 6
    .long 0xffffffff
finish add_imm32_minus1

fixture sub_reg
    sub DWORD PTR [esi], eax
finish sub_reg

fixture sub_reg_entry
    sub DWORD PTR [esi], eax
    jmp .Lsub_reg_next
finish sub_reg_entry

.Lsub_reg_next:
fixture sub_reg_prefix
    mov edx, 0xffffffff
    add edx, 1
    sub DWORD PTR [esi], eax
    mov ecx, 0x99999999
finish sub_reg_prefix

fixture sub_reg_smc
    sub DWORD PTR [esi], eax
    mov ecx, 0xbbbbbbbb
finish sub_reg_smc

fixture sub_reg_absolute
    sub DWORD PTR [0x4000], eax
finish sub_reg_absolute

fixture sub_reg_no_base
    sub DWORD PTR [ecx*4 + 0x4000], eax
finish sub_reg_no_base

fixture sub_reg_disp8_negative
    sub DWORD PTR [ebx - 128], eax
finish sub_reg_disp8_negative

fixture sub_reg_disp8_positive
    sub DWORD PTR [ebx + 127], eax
finish sub_reg_disp8_positive

fixture sub_reg_disp32
    sub DWORD PTR [ebx + 0x12345], eax
finish sub_reg_disp32

fixture sub_reg_base_wrap
    sub DWORD PTR [ebx + 8], eax
finish sub_reg_base_wrap

fixture sub_reg_index_wrap
    sub DWORD PTR [ebx + ecx*8], eax
finish sub_reg_index_wrap

fixture sub_reg_unaligned
    sub DWORD PTR [ebx + ecx*2 - 1], eax
finish sub_reg_unaligned

fixture sub_imm32
    sub DWORD PTR [esi], 0x80000001
finish sub_imm32

fixture sub_imm32_entry
    sub DWORD PTR [esi], 0x80000001
    jmp .Lsub_imm32_next
finish sub_imm32_entry

.Lsub_imm32_next:
fixture sub_imm32_prefix
    mov edx, 0xffffffff
    add edx, 1
    sub DWORD PTR [esi], 0x80000001
    mov ecx, 0x99999999
finish sub_imm32_prefix

fixture sub_imm32_smc
    sub DWORD PTR [esi], 0x80000001
    mov ecx, 0xbbbbbbbb
finish sub_imm32_smc

fixture sub_imm32_absolute
    sub DWORD PTR [0x4000], 0x80000001
finish sub_imm32_absolute

fixture sub_imm32_no_base
    sub DWORD PTR [ecx*4 + 0x4000], 0x80000001
finish sub_imm32_no_base

fixture sub_imm32_disp8_negative
    sub DWORD PTR [ebx - 128], 0x80000001
finish sub_imm32_disp8_negative

fixture sub_imm32_disp8_positive
    sub DWORD PTR [ebx + 127], 0x80000001
finish sub_imm32_disp8_positive

fixture sub_imm32_disp32
    sub DWORD PTR [ebx + 0x12345], 0x80000001
finish sub_imm32_disp32

fixture sub_imm32_base_wrap
    sub DWORD PTR [ebx + 8], 0x80000001
finish sub_imm32_base_wrap

fixture sub_imm32_index_wrap
    sub DWORD PTR [ebx + ecx*8], 0x80000001
finish sub_imm32_index_wrap

fixture sub_imm32_unaligned
    sub DWORD PTR [ebx + ecx*2 - 1], 0x80000001
finish sub_imm32_unaligned

fixture sub_imm8
    sub DWORD PTR [esi], -1
finish sub_imm8

fixture sub_imm8_entry
    sub DWORD PTR [esi], -1
    jmp .Lsub_imm8_next
finish sub_imm8_entry

.Lsub_imm8_next:
fixture sub_imm8_prefix
    mov edx, 0xffffffff
    add edx, 1
    sub DWORD PTR [esi], -1
    mov ecx, 0x99999999
finish sub_imm8_prefix

fixture sub_imm8_smc
    sub DWORD PTR [esi], -1
    mov ecx, 0xbbbbbbbb
finish sub_imm8_smc

fixture sub_imm8_absolute
    sub DWORD PTR [0x4000], -1
finish sub_imm8_absolute

fixture sub_imm8_no_base
    sub DWORD PTR [ecx*4 + 0x4000], -1
finish sub_imm8_no_base

fixture sub_imm8_disp8_negative
    sub DWORD PTR [ebx - 128], -1
finish sub_imm8_disp8_negative

fixture sub_imm8_disp8_positive
    sub DWORD PTR [ebx + 127], -1
finish sub_imm8_disp8_positive

fixture sub_imm8_disp32
    sub DWORD PTR [ebx + 0x12345], -1
finish sub_imm8_disp32

fixture sub_imm8_base_wrap
    sub DWORD PTR [ebx + 8], -1
finish sub_imm8_base_wrap

fixture sub_imm8_index_wrap
    sub DWORD PTR [ebx + ecx*8], -1
finish sub_imm8_index_wrap

fixture sub_imm8_unaligned
    sub DWORD PTR [ebx + ecx*2 - 1], -1
finish sub_imm8_unaligned

fixture sub_base_eax
    sub DWORD PTR [eax], eax
finish sub_base_eax

fixture sub_sib_eax
    sub DWORD PTR [eax + eax*2 - 4], eax
finish sub_sib_eax

fixture sub_index_eax
    sub DWORD PTR [ebx + eax*4 - 4], eax
finish sub_index_eax

fixture sub_base_ecx
    sub DWORD PTR [ecx], ecx
finish sub_base_ecx

fixture sub_sib_ecx
    sub DWORD PTR [ecx + ecx*2 - 4], ecx
finish sub_sib_ecx

fixture sub_index_ecx
    sub DWORD PTR [ebx + ecx*4 - 4], ecx
finish sub_index_ecx

fixture sub_base_edx
    sub DWORD PTR [edx], edx
finish sub_base_edx

fixture sub_sib_edx
    sub DWORD PTR [edx + edx*2 - 4], edx
finish sub_sib_edx

fixture sub_index_edx
    sub DWORD PTR [ebx + edx*4 - 4], edx
finish sub_index_edx

fixture sub_base_ebx
    sub DWORD PTR [ebx], ebx
finish sub_base_ebx

fixture sub_sib_ebx
    sub DWORD PTR [ebx + ebx*2 - 4], ebx
finish sub_sib_ebx

fixture sub_index_ebx
    sub DWORD PTR [ebx + ebx*4 - 4], ebx
finish sub_index_ebx

fixture sub_base_esp
    sub DWORD PTR [esp], esp
finish sub_base_esp

fixture sub_sib_esp
    sub DWORD PTR [esp + ecx*4 - 4], esp
finish sub_sib_esp

fixture sub_base_ebp
    sub DWORD PTR [ebp], ebp
finish sub_base_ebp

fixture sub_sib_ebp
    sub DWORD PTR [ebp + ebp*2 - 4], ebp
finish sub_sib_ebp

fixture sub_index_ebp
    sub DWORD PTR [ebx + ebp*4 - 4], ebp
finish sub_index_ebp

fixture sub_base_esi
    sub DWORD PTR [esi], esi
finish sub_base_esi

fixture sub_sib_esi
    sub DWORD PTR [esi + esi*2 - 4], esi
finish sub_sib_esi

fixture sub_index_esi
    sub DWORD PTR [ebx + esi*4 - 4], esi
finish sub_index_esi

fixture sub_base_edi
    sub DWORD PTR [edi], edi
finish sub_base_edi

fixture sub_sib_edi
    sub DWORD PTR [edi + edi*2 - 4], edi
finish sub_sib_edi

fixture sub_index_edi
    sub DWORD PTR [ebx + edi*4 - 4], edi
finish sub_index_edi

fixture sub_imm8_min
    sub DWORD PTR [esi], -128
finish sub_imm8_min

fixture sub_imm8_minus1
    sub DWORD PTR [esi], -1
finish sub_imm8_minus1

fixture sub_imm8_zero
    sub DWORD PTR [esi], 0
finish sub_imm8_zero

fixture sub_imm8_max
    sub DWORD PTR [esi], 127
finish sub_imm8_max

fixture sub_imm32_max
    sub DWORD PTR [esi], 0x7fffffff
finish sub_imm32_max

fixture sub_imm32_literal
    sub DWORD PTR [esi], 0x12345678
finish sub_imm32_literal

fixture sub_imm32_minus1
    .byte 0x81, 46
    .long 0xffffffff
finish sub_imm32_minus1

fixture and_reg
    and DWORD PTR [esi], eax
finish and_reg

fixture and_reg_entry
    and DWORD PTR [esi], eax
    jmp .Land_reg_next
finish and_reg_entry

.Land_reg_next:
fixture and_reg_prefix
    mov edx, 0xffffffff
    add edx, 1
    and DWORD PTR [esi], eax
    mov ecx, 0x99999999
finish and_reg_prefix

fixture and_reg_smc
    and DWORD PTR [esi], eax
    mov ecx, 0xbbbbbbbb
finish and_reg_smc

fixture and_reg_absolute
    and DWORD PTR [0x4000], eax
finish and_reg_absolute

fixture and_reg_no_base
    and DWORD PTR [ecx*4 + 0x4000], eax
finish and_reg_no_base

fixture and_reg_disp8_negative
    and DWORD PTR [ebx - 128], eax
finish and_reg_disp8_negative

fixture and_reg_disp8_positive
    and DWORD PTR [ebx + 127], eax
finish and_reg_disp8_positive

fixture and_reg_disp32
    and DWORD PTR [ebx + 0x12345], eax
finish and_reg_disp32

fixture and_reg_base_wrap
    and DWORD PTR [ebx + 8], eax
finish and_reg_base_wrap

fixture and_reg_index_wrap
    and DWORD PTR [ebx + ecx*8], eax
finish and_reg_index_wrap

fixture and_reg_unaligned
    and DWORD PTR [ebx + ecx*2 - 1], eax
finish and_reg_unaligned

fixture and_imm32
    and DWORD PTR [esi], 0x80000001
finish and_imm32

fixture and_imm32_entry
    and DWORD PTR [esi], 0x80000001
    jmp .Land_imm32_next
finish and_imm32_entry

.Land_imm32_next:
fixture and_imm32_prefix
    mov edx, 0xffffffff
    add edx, 1
    and DWORD PTR [esi], 0x80000001
    mov ecx, 0x99999999
finish and_imm32_prefix

fixture and_imm32_smc
    and DWORD PTR [esi], 0x80000001
    mov ecx, 0xbbbbbbbb
finish and_imm32_smc

fixture and_imm32_absolute
    and DWORD PTR [0x4000], 0x80000001
finish and_imm32_absolute

fixture and_imm32_no_base
    and DWORD PTR [ecx*4 + 0x4000], 0x80000001
finish and_imm32_no_base

fixture and_imm32_disp8_negative
    and DWORD PTR [ebx - 128], 0x80000001
finish and_imm32_disp8_negative

fixture and_imm32_disp8_positive
    and DWORD PTR [ebx + 127], 0x80000001
finish and_imm32_disp8_positive

fixture and_imm32_disp32
    and DWORD PTR [ebx + 0x12345], 0x80000001
finish and_imm32_disp32

fixture and_imm32_base_wrap
    and DWORD PTR [ebx + 8], 0x80000001
finish and_imm32_base_wrap

fixture and_imm32_index_wrap
    and DWORD PTR [ebx + ecx*8], 0x80000001
finish and_imm32_index_wrap

fixture and_imm32_unaligned
    and DWORD PTR [ebx + ecx*2 - 1], 0x80000001
finish and_imm32_unaligned

fixture and_imm8
    and DWORD PTR [esi], -1
finish and_imm8

fixture and_imm8_entry
    and DWORD PTR [esi], -1
    jmp .Land_imm8_next
finish and_imm8_entry

.Land_imm8_next:
fixture and_imm8_prefix
    mov edx, 0xffffffff
    add edx, 1
    and DWORD PTR [esi], -1
    mov ecx, 0x99999999
finish and_imm8_prefix

fixture and_imm8_smc
    and DWORD PTR [esi], -1
    mov ecx, 0xbbbbbbbb
finish and_imm8_smc

fixture and_imm8_absolute
    and DWORD PTR [0x4000], -1
finish and_imm8_absolute

fixture and_imm8_no_base
    and DWORD PTR [ecx*4 + 0x4000], -1
finish and_imm8_no_base

fixture and_imm8_disp8_negative
    and DWORD PTR [ebx - 128], -1
finish and_imm8_disp8_negative

fixture and_imm8_disp8_positive
    and DWORD PTR [ebx + 127], -1
finish and_imm8_disp8_positive

fixture and_imm8_disp32
    and DWORD PTR [ebx + 0x12345], -1
finish and_imm8_disp32

fixture and_imm8_base_wrap
    and DWORD PTR [ebx + 8], -1
finish and_imm8_base_wrap

fixture and_imm8_index_wrap
    and DWORD PTR [ebx + ecx*8], -1
finish and_imm8_index_wrap

fixture and_imm8_unaligned
    and DWORD PTR [ebx + ecx*2 - 1], -1
finish and_imm8_unaligned

fixture and_base_eax
    and DWORD PTR [eax], eax
finish and_base_eax

fixture and_sib_eax
    and DWORD PTR [eax + eax*2 - 4], eax
finish and_sib_eax

fixture and_index_eax
    and DWORD PTR [ebx + eax*4 - 4], eax
finish and_index_eax

fixture and_base_ecx
    and DWORD PTR [ecx], ecx
finish and_base_ecx

fixture and_sib_ecx
    and DWORD PTR [ecx + ecx*2 - 4], ecx
finish and_sib_ecx

fixture and_index_ecx
    and DWORD PTR [ebx + ecx*4 - 4], ecx
finish and_index_ecx

fixture and_base_edx
    and DWORD PTR [edx], edx
finish and_base_edx

fixture and_sib_edx
    and DWORD PTR [edx + edx*2 - 4], edx
finish and_sib_edx

fixture and_index_edx
    and DWORD PTR [ebx + edx*4 - 4], edx
finish and_index_edx

fixture and_base_ebx
    and DWORD PTR [ebx], ebx
finish and_base_ebx

fixture and_sib_ebx
    and DWORD PTR [ebx + ebx*2 - 4], ebx
finish and_sib_ebx

fixture and_index_ebx
    and DWORD PTR [ebx + ebx*4 - 4], ebx
finish and_index_ebx

fixture and_base_esp
    and DWORD PTR [esp], esp
finish and_base_esp

fixture and_sib_esp
    and DWORD PTR [esp + ecx*4 - 4], esp
finish and_sib_esp

fixture and_base_ebp
    and DWORD PTR [ebp], ebp
finish and_base_ebp

fixture and_sib_ebp
    and DWORD PTR [ebp + ebp*2 - 4], ebp
finish and_sib_ebp

fixture and_index_ebp
    and DWORD PTR [ebx + ebp*4 - 4], ebp
finish and_index_ebp

fixture and_base_esi
    and DWORD PTR [esi], esi
finish and_base_esi

fixture and_sib_esi
    and DWORD PTR [esi + esi*2 - 4], esi
finish and_sib_esi

fixture and_index_esi
    and DWORD PTR [ebx + esi*4 - 4], esi
finish and_index_esi

fixture and_base_edi
    and DWORD PTR [edi], edi
finish and_base_edi

fixture and_sib_edi
    and DWORD PTR [edi + edi*2 - 4], edi
finish and_sib_edi

fixture and_index_edi
    and DWORD PTR [ebx + edi*4 - 4], edi
finish and_index_edi

fixture and_imm8_min
    and DWORD PTR [esi], -128
finish and_imm8_min

fixture and_imm8_minus1
    and DWORD PTR [esi], -1
finish and_imm8_minus1

fixture and_imm8_zero
    and DWORD PTR [esi], 0
finish and_imm8_zero

fixture and_imm8_max
    and DWORD PTR [esi], 127
finish and_imm8_max

fixture and_imm32_max
    and DWORD PTR [esi], 0x7fffffff
finish and_imm32_max

fixture and_imm32_literal
    and DWORD PTR [esi], 0x12345678
finish and_imm32_literal

fixture and_imm32_minus1
    .byte 0x81, 38
    .long 0xffffffff
finish and_imm32_minus1

fixture or_reg
    or DWORD PTR [esi], eax
finish or_reg

fixture or_reg_entry
    or DWORD PTR [esi], eax
    jmp .Lor_reg_next
finish or_reg_entry

.Lor_reg_next:
fixture or_reg_prefix
    mov edx, 0xffffffff
    add edx, 1
    or DWORD PTR [esi], eax
    mov ecx, 0x99999999
finish or_reg_prefix

fixture or_reg_smc
    or DWORD PTR [esi], eax
    mov ecx, 0xbbbbbbbb
finish or_reg_smc

fixture or_reg_absolute
    or DWORD PTR [0x4000], eax
finish or_reg_absolute

fixture or_reg_no_base
    or DWORD PTR [ecx*4 + 0x4000], eax
finish or_reg_no_base

fixture or_reg_disp8_negative
    or DWORD PTR [ebx - 128], eax
finish or_reg_disp8_negative

fixture or_reg_disp8_positive
    or DWORD PTR [ebx + 127], eax
finish or_reg_disp8_positive

fixture or_reg_disp32
    or DWORD PTR [ebx + 0x12345], eax
finish or_reg_disp32

fixture or_reg_base_wrap
    or DWORD PTR [ebx + 8], eax
finish or_reg_base_wrap

fixture or_reg_index_wrap
    or DWORD PTR [ebx + ecx*8], eax
finish or_reg_index_wrap

fixture or_reg_unaligned
    or DWORD PTR [ebx + ecx*2 - 1], eax
finish or_reg_unaligned

fixture or_imm32
    or DWORD PTR [esi], 0x80000001
finish or_imm32

fixture or_imm32_entry
    or DWORD PTR [esi], 0x80000001
    jmp .Lor_imm32_next
finish or_imm32_entry

.Lor_imm32_next:
fixture or_imm32_prefix
    mov edx, 0xffffffff
    add edx, 1
    or DWORD PTR [esi], 0x80000001
    mov ecx, 0x99999999
finish or_imm32_prefix

fixture or_imm32_smc
    or DWORD PTR [esi], 0x80000001
    mov ecx, 0xbbbbbbbb
finish or_imm32_smc

fixture or_imm32_absolute
    or DWORD PTR [0x4000], 0x80000001
finish or_imm32_absolute

fixture or_imm32_no_base
    or DWORD PTR [ecx*4 + 0x4000], 0x80000001
finish or_imm32_no_base

fixture or_imm32_disp8_negative
    or DWORD PTR [ebx - 128], 0x80000001
finish or_imm32_disp8_negative

fixture or_imm32_disp8_positive
    or DWORD PTR [ebx + 127], 0x80000001
finish or_imm32_disp8_positive

fixture or_imm32_disp32
    or DWORD PTR [ebx + 0x12345], 0x80000001
finish or_imm32_disp32

fixture or_imm32_base_wrap
    or DWORD PTR [ebx + 8], 0x80000001
finish or_imm32_base_wrap

fixture or_imm32_index_wrap
    or DWORD PTR [ebx + ecx*8], 0x80000001
finish or_imm32_index_wrap

fixture or_imm32_unaligned
    or DWORD PTR [ebx + ecx*2 - 1], 0x80000001
finish or_imm32_unaligned

fixture or_imm8
    or DWORD PTR [esi], -1
finish or_imm8

fixture or_imm8_entry
    or DWORD PTR [esi], -1
    jmp .Lor_imm8_next
finish or_imm8_entry

.Lor_imm8_next:
fixture or_imm8_prefix
    mov edx, 0xffffffff
    add edx, 1
    or DWORD PTR [esi], -1
    mov ecx, 0x99999999
finish or_imm8_prefix

fixture or_imm8_smc
    or DWORD PTR [esi], -1
    mov ecx, 0xbbbbbbbb
finish or_imm8_smc

fixture or_imm8_absolute
    or DWORD PTR [0x4000], -1
finish or_imm8_absolute

fixture or_imm8_no_base
    or DWORD PTR [ecx*4 + 0x4000], -1
finish or_imm8_no_base

fixture or_imm8_disp8_negative
    or DWORD PTR [ebx - 128], -1
finish or_imm8_disp8_negative

fixture or_imm8_disp8_positive
    or DWORD PTR [ebx + 127], -1
finish or_imm8_disp8_positive

fixture or_imm8_disp32
    or DWORD PTR [ebx + 0x12345], -1
finish or_imm8_disp32

fixture or_imm8_base_wrap
    or DWORD PTR [ebx + 8], -1
finish or_imm8_base_wrap

fixture or_imm8_index_wrap
    or DWORD PTR [ebx + ecx*8], -1
finish or_imm8_index_wrap

fixture or_imm8_unaligned
    or DWORD PTR [ebx + ecx*2 - 1], -1
finish or_imm8_unaligned

fixture or_base_eax
    or DWORD PTR [eax], eax
finish or_base_eax

fixture or_sib_eax
    or DWORD PTR [eax + eax*2 - 4], eax
finish or_sib_eax

fixture or_index_eax
    or DWORD PTR [ebx + eax*4 - 4], eax
finish or_index_eax

fixture or_base_ecx
    or DWORD PTR [ecx], ecx
finish or_base_ecx

fixture or_sib_ecx
    or DWORD PTR [ecx + ecx*2 - 4], ecx
finish or_sib_ecx

fixture or_index_ecx
    or DWORD PTR [ebx + ecx*4 - 4], ecx
finish or_index_ecx

fixture or_base_edx
    or DWORD PTR [edx], edx
finish or_base_edx

fixture or_sib_edx
    or DWORD PTR [edx + edx*2 - 4], edx
finish or_sib_edx

fixture or_index_edx
    or DWORD PTR [ebx + edx*4 - 4], edx
finish or_index_edx

fixture or_base_ebx
    or DWORD PTR [ebx], ebx
finish or_base_ebx

fixture or_sib_ebx
    or DWORD PTR [ebx + ebx*2 - 4], ebx
finish or_sib_ebx

fixture or_index_ebx
    or DWORD PTR [ebx + ebx*4 - 4], ebx
finish or_index_ebx

fixture or_base_esp
    or DWORD PTR [esp], esp
finish or_base_esp

fixture or_sib_esp
    or DWORD PTR [esp + ecx*4 - 4], esp
finish or_sib_esp

fixture or_base_ebp
    or DWORD PTR [ebp], ebp
finish or_base_ebp

fixture or_sib_ebp
    or DWORD PTR [ebp + ebp*2 - 4], ebp
finish or_sib_ebp

fixture or_index_ebp
    or DWORD PTR [ebx + ebp*4 - 4], ebp
finish or_index_ebp

fixture or_base_esi
    or DWORD PTR [esi], esi
finish or_base_esi

fixture or_sib_esi
    or DWORD PTR [esi + esi*2 - 4], esi
finish or_sib_esi

fixture or_index_esi
    or DWORD PTR [ebx + esi*4 - 4], esi
finish or_index_esi

fixture or_base_edi
    or DWORD PTR [edi], edi
finish or_base_edi

fixture or_sib_edi
    or DWORD PTR [edi + edi*2 - 4], edi
finish or_sib_edi

fixture or_index_edi
    or DWORD PTR [ebx + edi*4 - 4], edi
finish or_index_edi

fixture or_imm8_min
    or DWORD PTR [esi], -128
finish or_imm8_min

fixture or_imm8_minus1
    or DWORD PTR [esi], -1
finish or_imm8_minus1

fixture or_imm8_zero
    or DWORD PTR [esi], 0
finish or_imm8_zero

fixture or_imm8_max
    or DWORD PTR [esi], 127
finish or_imm8_max

fixture or_imm32_max
    or DWORD PTR [esi], 0x7fffffff
finish or_imm32_max

fixture or_imm32_literal
    or DWORD PTR [esi], 0x12345678
finish or_imm32_literal

fixture or_imm32_minus1
    .byte 0x81, 14
    .long 0xffffffff
finish or_imm32_minus1

fixture xor_reg
    xor DWORD PTR [esi], eax
finish xor_reg

fixture xor_reg_entry
    xor DWORD PTR [esi], eax
    jmp .Lxor_reg_next
finish xor_reg_entry

.Lxor_reg_next:
fixture xor_reg_prefix
    mov edx, 0xffffffff
    add edx, 1
    xor DWORD PTR [esi], eax
    mov ecx, 0x99999999
finish xor_reg_prefix

fixture xor_reg_smc
    xor DWORD PTR [esi], eax
    mov ecx, 0xbbbbbbbb
finish xor_reg_smc

fixture xor_reg_absolute
    xor DWORD PTR [0x4000], eax
finish xor_reg_absolute

fixture xor_reg_no_base
    xor DWORD PTR [ecx*4 + 0x4000], eax
finish xor_reg_no_base

fixture xor_reg_disp8_negative
    xor DWORD PTR [ebx - 128], eax
finish xor_reg_disp8_negative

fixture xor_reg_disp8_positive
    xor DWORD PTR [ebx + 127], eax
finish xor_reg_disp8_positive

fixture xor_reg_disp32
    xor DWORD PTR [ebx + 0x12345], eax
finish xor_reg_disp32

fixture xor_reg_base_wrap
    xor DWORD PTR [ebx + 8], eax
finish xor_reg_base_wrap

fixture xor_reg_index_wrap
    xor DWORD PTR [ebx + ecx*8], eax
finish xor_reg_index_wrap

fixture xor_reg_unaligned
    xor DWORD PTR [ebx + ecx*2 - 1], eax
finish xor_reg_unaligned

fixture xor_imm32
    xor DWORD PTR [esi], 0x80000001
finish xor_imm32

fixture xor_imm32_entry
    xor DWORD PTR [esi], 0x80000001
    jmp .Lxor_imm32_next
finish xor_imm32_entry

.Lxor_imm32_next:
fixture xor_imm32_prefix
    mov edx, 0xffffffff
    add edx, 1
    xor DWORD PTR [esi], 0x80000001
    mov ecx, 0x99999999
finish xor_imm32_prefix

fixture xor_imm32_smc
    xor DWORD PTR [esi], 0x80000001
    mov ecx, 0xbbbbbbbb
finish xor_imm32_smc

fixture xor_imm32_absolute
    xor DWORD PTR [0x4000], 0x80000001
finish xor_imm32_absolute

fixture xor_imm32_no_base
    xor DWORD PTR [ecx*4 + 0x4000], 0x80000001
finish xor_imm32_no_base

fixture xor_imm32_disp8_negative
    xor DWORD PTR [ebx - 128], 0x80000001
finish xor_imm32_disp8_negative

fixture xor_imm32_disp8_positive
    xor DWORD PTR [ebx + 127], 0x80000001
finish xor_imm32_disp8_positive

fixture xor_imm32_disp32
    xor DWORD PTR [ebx + 0x12345], 0x80000001
finish xor_imm32_disp32

fixture xor_imm32_base_wrap
    xor DWORD PTR [ebx + 8], 0x80000001
finish xor_imm32_base_wrap

fixture xor_imm32_index_wrap
    xor DWORD PTR [ebx + ecx*8], 0x80000001
finish xor_imm32_index_wrap

fixture xor_imm32_unaligned
    xor DWORD PTR [ebx + ecx*2 - 1], 0x80000001
finish xor_imm32_unaligned

fixture xor_imm8
    xor DWORD PTR [esi], -1
finish xor_imm8

fixture xor_imm8_entry
    xor DWORD PTR [esi], -1
    jmp .Lxor_imm8_next
finish xor_imm8_entry

.Lxor_imm8_next:
fixture xor_imm8_prefix
    mov edx, 0xffffffff
    add edx, 1
    xor DWORD PTR [esi], -1
    mov ecx, 0x99999999
finish xor_imm8_prefix

fixture xor_imm8_smc
    xor DWORD PTR [esi], -1
    mov ecx, 0xbbbbbbbb
finish xor_imm8_smc

fixture xor_imm8_absolute
    xor DWORD PTR [0x4000], -1
finish xor_imm8_absolute

fixture xor_imm8_no_base
    xor DWORD PTR [ecx*4 + 0x4000], -1
finish xor_imm8_no_base

fixture xor_imm8_disp8_negative
    xor DWORD PTR [ebx - 128], -1
finish xor_imm8_disp8_negative

fixture xor_imm8_disp8_positive
    xor DWORD PTR [ebx + 127], -1
finish xor_imm8_disp8_positive

fixture xor_imm8_disp32
    xor DWORD PTR [ebx + 0x12345], -1
finish xor_imm8_disp32

fixture xor_imm8_base_wrap
    xor DWORD PTR [ebx + 8], -1
finish xor_imm8_base_wrap

fixture xor_imm8_index_wrap
    xor DWORD PTR [ebx + ecx*8], -1
finish xor_imm8_index_wrap

fixture xor_imm8_unaligned
    xor DWORD PTR [ebx + ecx*2 - 1], -1
finish xor_imm8_unaligned

fixture xor_base_eax
    xor DWORD PTR [eax], eax
finish xor_base_eax

fixture xor_sib_eax
    xor DWORD PTR [eax + eax*2 - 4], eax
finish xor_sib_eax

fixture xor_index_eax
    xor DWORD PTR [ebx + eax*4 - 4], eax
finish xor_index_eax

fixture xor_base_ecx
    xor DWORD PTR [ecx], ecx
finish xor_base_ecx

fixture xor_sib_ecx
    xor DWORD PTR [ecx + ecx*2 - 4], ecx
finish xor_sib_ecx

fixture xor_index_ecx
    xor DWORD PTR [ebx + ecx*4 - 4], ecx
finish xor_index_ecx

fixture xor_base_edx
    xor DWORD PTR [edx], edx
finish xor_base_edx

fixture xor_sib_edx
    xor DWORD PTR [edx + edx*2 - 4], edx
finish xor_sib_edx

fixture xor_index_edx
    xor DWORD PTR [ebx + edx*4 - 4], edx
finish xor_index_edx

fixture xor_base_ebx
    xor DWORD PTR [ebx], ebx
finish xor_base_ebx

fixture xor_sib_ebx
    xor DWORD PTR [ebx + ebx*2 - 4], ebx
finish xor_sib_ebx

fixture xor_index_ebx
    xor DWORD PTR [ebx + ebx*4 - 4], ebx
finish xor_index_ebx

fixture xor_base_esp
    xor DWORD PTR [esp], esp
finish xor_base_esp

fixture xor_sib_esp
    xor DWORD PTR [esp + ecx*4 - 4], esp
finish xor_sib_esp

fixture xor_base_ebp
    xor DWORD PTR [ebp], ebp
finish xor_base_ebp

fixture xor_sib_ebp
    xor DWORD PTR [ebp + ebp*2 - 4], ebp
finish xor_sib_ebp

fixture xor_index_ebp
    xor DWORD PTR [ebx + ebp*4 - 4], ebp
finish xor_index_ebp

fixture xor_base_esi
    xor DWORD PTR [esi], esi
finish xor_base_esi

fixture xor_sib_esi
    xor DWORD PTR [esi + esi*2 - 4], esi
finish xor_sib_esi

fixture xor_index_esi
    xor DWORD PTR [ebx + esi*4 - 4], esi
finish xor_index_esi

fixture xor_base_edi
    xor DWORD PTR [edi], edi
finish xor_base_edi

fixture xor_sib_edi
    xor DWORD PTR [edi + edi*2 - 4], edi
finish xor_sib_edi

fixture xor_index_edi
    xor DWORD PTR [ebx + edi*4 - 4], edi
finish xor_index_edi

fixture xor_imm8_min
    xor DWORD PTR [esi], -128
finish xor_imm8_min

fixture xor_imm8_minus1
    xor DWORD PTR [esi], -1
finish xor_imm8_minus1

fixture xor_imm8_zero
    xor DWORD PTR [esi], 0
finish xor_imm8_zero

fixture xor_imm8_max
    xor DWORD PTR [esi], 127
finish xor_imm8_max

fixture xor_imm32_max
    xor DWORD PTR [esi], 0x7fffffff
finish xor_imm32_max

fixture xor_imm32_literal
    xor DWORD PTR [esi], 0x12345678
finish xor_imm32_literal

fixture xor_imm32_minus1
    .byte 0x81, 54
    .long 0xffffffff
finish xor_imm32_minus1

.org 0x1000, 0x90
.Larray:
fixture array_loop
    add DWORD PTR [esi], eax
    sub DWORD PTR [esi], ebx
    and DWORD PTR [esi], edx
    or DWORD PTR [esi], ebp
    xor DWORD PTR [esi], edi
    jz .Lzero
finish array_loop

fixture array_nonzero
    mov DWORD PTR [esi + 4], 2
    jmp .Ljoined
finish array_nonzero

.org 0x1080, 0x90
.Lzero:
fixture array_zero
    mov DWORD PTR [esi + 4], 1
    jmp .Ljoined
finish array_zero

.org 0x10c0, 0x90
.Ljoined:
fixture array_joined
    lea esi, [esi + 8]
    sub ecx, 1
    jnz .Larray
finish array_joined

fixture array_finish
    jmp .Ldone
finish array_finish

.org 0x1100, 0x90
.Ldone:
fixture array_done
    ud2
finish array_done
