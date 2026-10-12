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

fixture not
    not DWORD PTR [esi]
finish not

fixture not_entry
    not DWORD PTR [esi]
    jmp .Lnext
finish not_entry

.Lnext:
fixture not_prefix
    mov edx, 0xffffffff
    add edx, 1
    not DWORD PTR [esi]
    mov ecx, 0x99999999
finish not_prefix

fixture not_base_eax
    not DWORD PTR [eax]
finish not_base_eax

fixture not_sib_eax
    not DWORD PTR [eax + eax*2 - 4]
finish not_sib_eax

fixture not_index_eax
    not DWORD PTR [ebx + eax*4 - 4]
finish not_index_eax

fixture not_base_ecx
    not DWORD PTR [ecx]
finish not_base_ecx

fixture not_sib_ecx
    not DWORD PTR [ecx + ecx*2 - 4]
finish not_sib_ecx

fixture not_index_ecx
    not DWORD PTR [ebx + ecx*4 - 4]
finish not_index_ecx

fixture not_base_edx
    not DWORD PTR [edx]
finish not_base_edx

fixture not_sib_edx
    not DWORD PTR [edx + edx*2 - 4]
finish not_sib_edx

fixture not_index_edx
    not DWORD PTR [ebx + edx*4 - 4]
finish not_index_edx

fixture not_base_ebx
    not DWORD PTR [ebx]
finish not_base_ebx

fixture not_sib_ebx
    not DWORD PTR [ebx + ebx*2 - 4]
finish not_sib_ebx

fixture not_index_ebx
    not DWORD PTR [ebx + ebx*4 - 4]
finish not_index_ebx

fixture not_base_esp
    not DWORD PTR [esp]
finish not_base_esp

fixture not_sib_esp
    not DWORD PTR [esp + ecx*4 - 4]
finish not_sib_esp

fixture not_base_ebp
    not DWORD PTR [ebp]
finish not_base_ebp

fixture not_sib_ebp
    not DWORD PTR [ebp + ebp*2 - 4]
finish not_sib_ebp

fixture not_index_ebp
    not DWORD PTR [ebx + ebp*4 - 4]
finish not_index_ebp

fixture not_base_esi
    not DWORD PTR [esi]
finish not_base_esi

fixture not_sib_esi
    not DWORD PTR [esi + esi*2 - 4]
finish not_sib_esi

fixture not_index_esi
    not DWORD PTR [ebx + esi*4 - 4]
finish not_index_esi

fixture not_base_edi
    not DWORD PTR [edi]
finish not_base_edi

fixture not_sib_edi
    not DWORD PTR [edi + edi*2 - 4]
finish not_sib_edi

fixture not_index_edi
    not DWORD PTR [ebx + edi*4 - 4]
finish not_index_edi

fixture not_absolute
    not DWORD PTR [0x4000]
finish not_absolute

fixture not_no_base
    not DWORD PTR [ecx*4 + 0x4000]
finish not_no_base

fixture not_disp8_negative
    not DWORD PTR [ebx - 128]
finish not_disp8_negative

fixture not_disp8_positive
    not DWORD PTR [ebx + 127]
finish not_disp8_positive

fixture not_disp32
    not DWORD PTR [ebx + 0x12345]
finish not_disp32

fixture not_base_wrap
    not DWORD PTR [ebx + 8]
finish not_base_wrap

fixture not_index_wrap
    not DWORD PTR [ebx + ecx*8]
finish not_index_wrap

fixture not_unaligned
    not DWORD PTR [ebx + ecx*2 - 1]
finish not_unaligned

fixture not_smc
    not DWORD PTR [esi]
    mov ecx, 0xbbbbbbbb
finish not_smc

.org 0x600, 0x90
.Linvert:
fixture invert_loop
    not DWORD PTR [esi]
    lea esi, [esi + 4]
    sub ecx, 1
    jnz .Linvert
finish invert_loop

fixture invert_finish
    jmp .Lreset
finish invert_finish

.org 0x680, 0x90
.Lreset:
fixture reset_loop
    mov esi, edi
    mov ecx, ebp
    jmp .Lrestore
finish reset_loop

.org 0x6c0, 0x90
.Lrestore:
fixture restore_loop
    not DWORD PTR [esi]
    lea esi, [esi + 4]
    sub ecx, 1
    jnz .Lrestore
finish restore_loop

fixture restore_finish
    jmp .Ldone
finish restore_finish

.org 0x700, 0x90
.Ldone:
fixture restore_done
    ud2
finish restore_done
