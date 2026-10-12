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

fixture inc
    inc DWORD PTR [esi]
finish inc

fixture inc_entry
    inc DWORD PTR [esi]
    jmp .Linc_next
finish inc_entry

.Linc_next:
fixture inc_prefix
    mov edx, 0xffffffff
    add edx, 1
    inc DWORD PTR [esi]
    mov ecx, 0x99999999
finish inc_prefix

fixture inc_base_eax
    inc DWORD PTR [eax]
finish inc_base_eax

fixture inc_sib_eax
    inc DWORD PTR [eax + eax*2 - 4]
finish inc_sib_eax

fixture inc_index_eax
    inc DWORD PTR [ebx + eax*4 - 4]
finish inc_index_eax

fixture inc_base_ecx
    inc DWORD PTR [ecx]
finish inc_base_ecx

fixture inc_sib_ecx
    inc DWORD PTR [ecx + ecx*2 - 4]
finish inc_sib_ecx

fixture inc_index_ecx
    inc DWORD PTR [ebx + ecx*4 - 4]
finish inc_index_ecx

fixture inc_base_edx
    inc DWORD PTR [edx]
finish inc_base_edx

fixture inc_sib_edx
    inc DWORD PTR [edx + edx*2 - 4]
finish inc_sib_edx

fixture inc_index_edx
    inc DWORD PTR [ebx + edx*4 - 4]
finish inc_index_edx

fixture inc_base_ebx
    inc DWORD PTR [ebx]
finish inc_base_ebx

fixture inc_sib_ebx
    inc DWORD PTR [ebx + ebx*2 - 4]
finish inc_sib_ebx

fixture inc_index_ebx
    inc DWORD PTR [ebx + ebx*4 - 4]
finish inc_index_ebx

fixture inc_base_esp
    inc DWORD PTR [esp]
finish inc_base_esp

fixture inc_sib_esp
    inc DWORD PTR [esp + ecx*4 - 4]
finish inc_sib_esp

fixture inc_base_ebp
    inc DWORD PTR [ebp]
finish inc_base_ebp

fixture inc_sib_ebp
    inc DWORD PTR [ebp + ebp*2 - 4]
finish inc_sib_ebp

fixture inc_index_ebp
    inc DWORD PTR [ebx + ebp*4 - 4]
finish inc_index_ebp

fixture inc_base_esi
    inc DWORD PTR [esi]
finish inc_base_esi

fixture inc_sib_esi
    inc DWORD PTR [esi + esi*2 - 4]
finish inc_sib_esi

fixture inc_index_esi
    inc DWORD PTR [ebx + esi*4 - 4]
finish inc_index_esi

fixture inc_base_edi
    inc DWORD PTR [edi]
finish inc_base_edi

fixture inc_sib_edi
    inc DWORD PTR [edi + edi*2 - 4]
finish inc_sib_edi

fixture inc_index_edi
    inc DWORD PTR [ebx + edi*4 - 4]
finish inc_index_edi

fixture inc_absolute
    inc DWORD PTR [0x4000]
finish inc_absolute

fixture inc_no_base
    inc DWORD PTR [ecx*4 + 0x4000]
finish inc_no_base

fixture inc_disp8_negative
    inc DWORD PTR [ebx - 128]
finish inc_disp8_negative

fixture inc_disp8_positive
    inc DWORD PTR [ebx + 127]
finish inc_disp8_positive

fixture inc_disp32
    inc DWORD PTR [ebx + 0x12345]
finish inc_disp32

fixture inc_base_wrap
    inc DWORD PTR [ebx + 8]
finish inc_base_wrap

fixture inc_index_wrap
    inc DWORD PTR [ebx + ecx*8]
finish inc_index_wrap

fixture inc_unaligned
    inc DWORD PTR [ebx + ecx*2 - 1]
finish inc_unaligned

fixture inc_smc
    inc DWORD PTR [esi]
    mov ecx, 0xbbbbbbbb
finish inc_smc

fixture dec
    dec DWORD PTR [esi]
finish dec

fixture dec_entry
    dec DWORD PTR [esi]
    jmp .Ldec_next
finish dec_entry

.Ldec_next:
fixture dec_prefix
    mov edx, 0xffffffff
    add edx, 1
    dec DWORD PTR [esi]
    mov ecx, 0x99999999
finish dec_prefix

fixture dec_base_eax
    dec DWORD PTR [eax]
finish dec_base_eax

fixture dec_sib_eax
    dec DWORD PTR [eax + eax*2 - 4]
finish dec_sib_eax

fixture dec_index_eax
    dec DWORD PTR [ebx + eax*4 - 4]
finish dec_index_eax

fixture dec_base_ecx
    dec DWORD PTR [ecx]
finish dec_base_ecx

fixture dec_sib_ecx
    dec DWORD PTR [ecx + ecx*2 - 4]
finish dec_sib_ecx

fixture dec_index_ecx
    dec DWORD PTR [ebx + ecx*4 - 4]
finish dec_index_ecx

fixture dec_base_edx
    dec DWORD PTR [edx]
finish dec_base_edx

fixture dec_sib_edx
    dec DWORD PTR [edx + edx*2 - 4]
finish dec_sib_edx

fixture dec_index_edx
    dec DWORD PTR [ebx + edx*4 - 4]
finish dec_index_edx

fixture dec_base_ebx
    dec DWORD PTR [ebx]
finish dec_base_ebx

fixture dec_sib_ebx
    dec DWORD PTR [ebx + ebx*2 - 4]
finish dec_sib_ebx

fixture dec_index_ebx
    dec DWORD PTR [ebx + ebx*4 - 4]
finish dec_index_ebx

fixture dec_base_esp
    dec DWORD PTR [esp]
finish dec_base_esp

fixture dec_sib_esp
    dec DWORD PTR [esp + ecx*4 - 4]
finish dec_sib_esp

fixture dec_base_ebp
    dec DWORD PTR [ebp]
finish dec_base_ebp

fixture dec_sib_ebp
    dec DWORD PTR [ebp + ebp*2 - 4]
finish dec_sib_ebp

fixture dec_index_ebp
    dec DWORD PTR [ebx + ebp*4 - 4]
finish dec_index_ebp

fixture dec_base_esi
    dec DWORD PTR [esi]
finish dec_base_esi

fixture dec_sib_esi
    dec DWORD PTR [esi + esi*2 - 4]
finish dec_sib_esi

fixture dec_index_esi
    dec DWORD PTR [ebx + esi*4 - 4]
finish dec_index_esi

fixture dec_base_edi
    dec DWORD PTR [edi]
finish dec_base_edi

fixture dec_sib_edi
    dec DWORD PTR [edi + edi*2 - 4]
finish dec_sib_edi

fixture dec_index_edi
    dec DWORD PTR [ebx + edi*4 - 4]
finish dec_index_edi

fixture dec_absolute
    dec DWORD PTR [0x4000]
finish dec_absolute

fixture dec_no_base
    dec DWORD PTR [ecx*4 + 0x4000]
finish dec_no_base

fixture dec_disp8_negative
    dec DWORD PTR [ebx - 128]
finish dec_disp8_negative

fixture dec_disp8_positive
    dec DWORD PTR [ebx + 127]
finish dec_disp8_positive

fixture dec_disp32
    dec DWORD PTR [ebx + 0x12345]
finish dec_disp32

fixture dec_base_wrap
    dec DWORD PTR [ebx + 8]
finish dec_base_wrap

fixture dec_index_wrap
    dec DWORD PTR [ebx + ecx*8]
finish dec_index_wrap

fixture dec_unaligned
    dec DWORD PTR [ebx + ecx*2 - 1]
finish dec_unaligned

fixture dec_smc
    dec DWORD PTR [esi]
    mov ecx, 0xbbbbbbbb
finish dec_smc

fixture neg
    neg DWORD PTR [esi]
finish neg

fixture neg_entry
    neg DWORD PTR [esi]
    jmp .Lneg_next
finish neg_entry

.Lneg_next:
fixture neg_prefix
    mov edx, 0xffffffff
    add edx, 1
    neg DWORD PTR [esi]
    mov ecx, 0x99999999
finish neg_prefix

fixture neg_base_eax
    neg DWORD PTR [eax]
finish neg_base_eax

fixture neg_sib_eax
    neg DWORD PTR [eax + eax*2 - 4]
finish neg_sib_eax

fixture neg_index_eax
    neg DWORD PTR [ebx + eax*4 - 4]
finish neg_index_eax

fixture neg_base_ecx
    neg DWORD PTR [ecx]
finish neg_base_ecx

fixture neg_sib_ecx
    neg DWORD PTR [ecx + ecx*2 - 4]
finish neg_sib_ecx

fixture neg_index_ecx
    neg DWORD PTR [ebx + ecx*4 - 4]
finish neg_index_ecx

fixture neg_base_edx
    neg DWORD PTR [edx]
finish neg_base_edx

fixture neg_sib_edx
    neg DWORD PTR [edx + edx*2 - 4]
finish neg_sib_edx

fixture neg_index_edx
    neg DWORD PTR [ebx + edx*4 - 4]
finish neg_index_edx

fixture neg_base_ebx
    neg DWORD PTR [ebx]
finish neg_base_ebx

fixture neg_sib_ebx
    neg DWORD PTR [ebx + ebx*2 - 4]
finish neg_sib_ebx

fixture neg_index_ebx
    neg DWORD PTR [ebx + ebx*4 - 4]
finish neg_index_ebx

fixture neg_base_esp
    neg DWORD PTR [esp]
finish neg_base_esp

fixture neg_sib_esp
    neg DWORD PTR [esp + ecx*4 - 4]
finish neg_sib_esp

fixture neg_base_ebp
    neg DWORD PTR [ebp]
finish neg_base_ebp

fixture neg_sib_ebp
    neg DWORD PTR [ebp + ebp*2 - 4]
finish neg_sib_ebp

fixture neg_index_ebp
    neg DWORD PTR [ebx + ebp*4 - 4]
finish neg_index_ebp

fixture neg_base_esi
    neg DWORD PTR [esi]
finish neg_base_esi

fixture neg_sib_esi
    neg DWORD PTR [esi + esi*2 - 4]
finish neg_sib_esi

fixture neg_index_esi
    neg DWORD PTR [ebx + esi*4 - 4]
finish neg_index_esi

fixture neg_base_edi
    neg DWORD PTR [edi]
finish neg_base_edi

fixture neg_sib_edi
    neg DWORD PTR [edi + edi*2 - 4]
finish neg_sib_edi

fixture neg_index_edi
    neg DWORD PTR [ebx + edi*4 - 4]
finish neg_index_edi

fixture neg_absolute
    neg DWORD PTR [0x4000]
finish neg_absolute

fixture neg_no_base
    neg DWORD PTR [ecx*4 + 0x4000]
finish neg_no_base

fixture neg_disp8_negative
    neg DWORD PTR [ebx - 128]
finish neg_disp8_negative

fixture neg_disp8_positive
    neg DWORD PTR [ebx + 127]
finish neg_disp8_positive

fixture neg_disp32
    neg DWORD PTR [ebx + 0x12345]
finish neg_disp32

fixture neg_base_wrap
    neg DWORD PTR [ebx + 8]
finish neg_base_wrap

fixture neg_index_wrap
    neg DWORD PTR [ebx + ecx*8]
finish neg_index_wrap

fixture neg_unaligned
    neg DWORD PTR [ebx + ecx*2 - 1]
finish neg_unaligned

fixture neg_smc
    neg DWORD PTR [esi]
    mov ecx, 0xbbbbbbbb
finish neg_smc

.org 0x600, 0x90
.Lcounter:
fixture counter_loop
    dec DWORD PTR [esi]
    jnz .Lcounter
finish counter_loop

fixture counter_finish
    jmp .Ldone
finish counter_finish

.org 0x700, 0x90
.Ldone:
fixture counter_done
    ud2
finish counter_done
