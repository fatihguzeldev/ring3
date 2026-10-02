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

fixture add
    add eax, DWORD PTR [esi]
finish add

fixture add_prefix
    mov edx, 0xffffffff
    add edx, 1
    add eax, DWORD PTR [esi]
    mov ecx, 0x99999999
finish add_prefix

fixture add_base_eax
    add eax, DWORD PTR [eax]
finish add_base_eax

fixture add_sib_eax
    add eax, DWORD PTR [eax + eax*2 - 4]
finish add_sib_eax

fixture add_base_ecx
    add ecx, DWORD PTR [ecx]
finish add_base_ecx

fixture add_sib_ecx
    add ecx, DWORD PTR [ecx + ecx*2 - 4]
finish add_sib_ecx

fixture add_base_edx
    add edx, DWORD PTR [edx]
finish add_base_edx

fixture add_sib_edx
    add edx, DWORD PTR [edx + edx*2 - 4]
finish add_sib_edx

fixture add_base_ebx
    add ebx, DWORD PTR [ebx]
finish add_base_ebx

fixture add_sib_ebx
    add ebx, DWORD PTR [ebx + ebx*2 - 4]
finish add_sib_ebx

fixture add_base_esp
    add esp, DWORD PTR [esp]
finish add_base_esp

fixture add_sib_esp
    add esp, DWORD PTR [esp + ecx*4 - 4]
finish add_sib_esp

fixture add_base_ebp
    add ebp, DWORD PTR [ebp]
finish add_base_ebp

fixture add_sib_ebp
    add ebp, DWORD PTR [ebp + ebp*2 - 4]
finish add_sib_ebp

fixture add_base_esi
    add esi, DWORD PTR [esi]
finish add_base_esi

fixture add_sib_esi
    add esi, DWORD PTR [esi + esi*2 - 4]
finish add_sib_esi

fixture add_base_edi
    add edi, DWORD PTR [edi]
finish add_base_edi

fixture add_sib_edi
    add edi, DWORD PTR [edi + edi*2 - 4]
finish add_sib_edi

fixture add_absolute
    add eax, DWORD PTR [0x4000]
finish add_absolute

fixture add_no_base
    add eax, DWORD PTR [ecx*4 + 0x4000]
finish add_no_base

fixture add_disp8_negative
    add eax, DWORD PTR [ebx - 128]
finish add_disp8_negative

fixture add_disp8_positive
    add eax, DWORD PTR [ebx + 127]
finish add_disp8_positive

fixture add_disp32
    add eax, DWORD PTR [ebx + 0x12345]
finish add_disp32

fixture add_base_wrap
    add eax, DWORD PTR [ebx + 8]
finish add_base_wrap

fixture add_index_wrap
    add eax, DWORD PTR [ebx + ecx*8]
finish add_index_wrap

fixture add_unaligned
    add eax, DWORD PTR [ebx + ecx*2 - 1]
finish add_unaligned

fixture sub
    sub eax, DWORD PTR [esi]
finish sub

fixture sub_prefix
    mov edx, 0xffffffff
    add edx, 1
    sub eax, DWORD PTR [esi]
    mov ecx, 0x99999999
finish sub_prefix

fixture sub_base_eax
    sub eax, DWORD PTR [eax]
finish sub_base_eax

fixture sub_sib_eax
    sub eax, DWORD PTR [eax + eax*2 - 4]
finish sub_sib_eax

fixture sub_base_ecx
    sub ecx, DWORD PTR [ecx]
finish sub_base_ecx

fixture sub_sib_ecx
    sub ecx, DWORD PTR [ecx + ecx*2 - 4]
finish sub_sib_ecx

fixture sub_base_edx
    sub edx, DWORD PTR [edx]
finish sub_base_edx

fixture sub_sib_edx
    sub edx, DWORD PTR [edx + edx*2 - 4]
finish sub_sib_edx

fixture sub_base_ebx
    sub ebx, DWORD PTR [ebx]
finish sub_base_ebx

fixture sub_sib_ebx
    sub ebx, DWORD PTR [ebx + ebx*2 - 4]
finish sub_sib_ebx

fixture sub_base_esp
    sub esp, DWORD PTR [esp]
finish sub_base_esp

fixture sub_sib_esp
    sub esp, DWORD PTR [esp + ecx*4 - 4]
finish sub_sib_esp

fixture sub_base_ebp
    sub ebp, DWORD PTR [ebp]
finish sub_base_ebp

fixture sub_sib_ebp
    sub ebp, DWORD PTR [ebp + ebp*2 - 4]
finish sub_sib_ebp

fixture sub_base_esi
    sub esi, DWORD PTR [esi]
finish sub_base_esi

fixture sub_sib_esi
    sub esi, DWORD PTR [esi + esi*2 - 4]
finish sub_sib_esi

fixture sub_base_edi
    sub edi, DWORD PTR [edi]
finish sub_base_edi

fixture sub_sib_edi
    sub edi, DWORD PTR [edi + edi*2 - 4]
finish sub_sib_edi

fixture sub_absolute
    sub eax, DWORD PTR [0x4000]
finish sub_absolute

fixture sub_no_base
    sub eax, DWORD PTR [ecx*4 + 0x4000]
finish sub_no_base

fixture sub_disp8_negative
    sub eax, DWORD PTR [ebx - 128]
finish sub_disp8_negative

fixture sub_disp8_positive
    sub eax, DWORD PTR [ebx + 127]
finish sub_disp8_positive

fixture sub_disp32
    sub eax, DWORD PTR [ebx + 0x12345]
finish sub_disp32

fixture sub_base_wrap
    sub eax, DWORD PTR [ebx + 8]
finish sub_base_wrap

fixture sub_index_wrap
    sub eax, DWORD PTR [ebx + ecx*8]
finish sub_index_wrap

fixture sub_unaligned
    sub eax, DWORD PTR [ebx + ecx*2 - 1]
finish sub_unaligned

fixture cmp
    cmp eax, DWORD PTR [esi]
finish cmp

fixture cmp_prefix
    mov edx, 0xffffffff
    add edx, 1
    cmp eax, DWORD PTR [esi]
    mov ecx, 0x99999999
finish cmp_prefix

fixture cmp_base_eax
    cmp eax, DWORD PTR [eax]
finish cmp_base_eax

fixture cmp_sib_eax
    cmp eax, DWORD PTR [eax + eax*2 - 4]
finish cmp_sib_eax

fixture cmp_base_ecx
    cmp ecx, DWORD PTR [ecx]
finish cmp_base_ecx

fixture cmp_sib_ecx
    cmp ecx, DWORD PTR [ecx + ecx*2 - 4]
finish cmp_sib_ecx

fixture cmp_base_edx
    cmp edx, DWORD PTR [edx]
finish cmp_base_edx

fixture cmp_sib_edx
    cmp edx, DWORD PTR [edx + edx*2 - 4]
finish cmp_sib_edx

fixture cmp_base_ebx
    cmp ebx, DWORD PTR [ebx]
finish cmp_base_ebx

fixture cmp_sib_ebx
    cmp ebx, DWORD PTR [ebx + ebx*2 - 4]
finish cmp_sib_ebx

fixture cmp_base_esp
    cmp esp, DWORD PTR [esp]
finish cmp_base_esp

fixture cmp_sib_esp
    cmp esp, DWORD PTR [esp + ecx*4 - 4]
finish cmp_sib_esp

fixture cmp_base_ebp
    cmp ebp, DWORD PTR [ebp]
finish cmp_base_ebp

fixture cmp_sib_ebp
    cmp ebp, DWORD PTR [ebp + ebp*2 - 4]
finish cmp_sib_ebp

fixture cmp_base_esi
    cmp esi, DWORD PTR [esi]
finish cmp_base_esi

fixture cmp_sib_esi
    cmp esi, DWORD PTR [esi + esi*2 - 4]
finish cmp_sib_esi

fixture cmp_base_edi
    cmp edi, DWORD PTR [edi]
finish cmp_base_edi

fixture cmp_sib_edi
    cmp edi, DWORD PTR [edi + edi*2 - 4]
finish cmp_sib_edi

fixture cmp_absolute
    cmp eax, DWORD PTR [0x4000]
finish cmp_absolute

fixture cmp_no_base
    cmp eax, DWORD PTR [ecx*4 + 0x4000]
finish cmp_no_base

fixture cmp_disp8_negative
    cmp eax, DWORD PTR [ebx - 128]
finish cmp_disp8_negative

fixture cmp_disp8_positive
    cmp eax, DWORD PTR [ebx + 127]
finish cmp_disp8_positive

fixture cmp_disp32
    cmp eax, DWORD PTR [ebx + 0x12345]
finish cmp_disp32

fixture cmp_base_wrap
    cmp eax, DWORD PTR [ebx + 8]
finish cmp_base_wrap

fixture cmp_index_wrap
    cmp eax, DWORD PTR [ebx + ecx*8]
finish cmp_index_wrap

fixture cmp_unaligned
    cmp eax, DWORD PTR [ebx + ecx*2 - 1]
finish cmp_unaligned

fixture and
    and eax, DWORD PTR [esi]
finish and

fixture and_prefix
    mov edx, 0xffffffff
    add edx, 1
    and eax, DWORD PTR [esi]
    mov ecx, 0x99999999
finish and_prefix

fixture and_base_eax
    and eax, DWORD PTR [eax]
finish and_base_eax

fixture and_sib_eax
    and eax, DWORD PTR [eax + eax*2 - 4]
finish and_sib_eax

fixture and_base_ecx
    and ecx, DWORD PTR [ecx]
finish and_base_ecx

fixture and_sib_ecx
    and ecx, DWORD PTR [ecx + ecx*2 - 4]
finish and_sib_ecx

fixture and_base_edx
    and edx, DWORD PTR [edx]
finish and_base_edx

fixture and_sib_edx
    and edx, DWORD PTR [edx + edx*2 - 4]
finish and_sib_edx

fixture and_base_ebx
    and ebx, DWORD PTR [ebx]
finish and_base_ebx

fixture and_sib_ebx
    and ebx, DWORD PTR [ebx + ebx*2 - 4]
finish and_sib_ebx

fixture and_base_esp
    and esp, DWORD PTR [esp]
finish and_base_esp

fixture and_sib_esp
    and esp, DWORD PTR [esp + ecx*4 - 4]
finish and_sib_esp

fixture and_base_ebp
    and ebp, DWORD PTR [ebp]
finish and_base_ebp

fixture and_sib_ebp
    and ebp, DWORD PTR [ebp + ebp*2 - 4]
finish and_sib_ebp

fixture and_base_esi
    and esi, DWORD PTR [esi]
finish and_base_esi

fixture and_sib_esi
    and esi, DWORD PTR [esi + esi*2 - 4]
finish and_sib_esi

fixture and_base_edi
    and edi, DWORD PTR [edi]
finish and_base_edi

fixture and_sib_edi
    and edi, DWORD PTR [edi + edi*2 - 4]
finish and_sib_edi

fixture and_absolute
    and eax, DWORD PTR [0x4000]
finish and_absolute

fixture and_no_base
    and eax, DWORD PTR [ecx*4 + 0x4000]
finish and_no_base

fixture and_disp8_negative
    and eax, DWORD PTR [ebx - 128]
finish and_disp8_negative

fixture and_disp8_positive
    and eax, DWORD PTR [ebx + 127]
finish and_disp8_positive

fixture and_disp32
    and eax, DWORD PTR [ebx + 0x12345]
finish and_disp32

fixture and_base_wrap
    and eax, DWORD PTR [ebx + 8]
finish and_base_wrap

fixture and_index_wrap
    and eax, DWORD PTR [ebx + ecx*8]
finish and_index_wrap

fixture and_unaligned
    and eax, DWORD PTR [ebx + ecx*2 - 1]
finish and_unaligned

fixture or
    or eax, DWORD PTR [esi]
finish or

fixture or_prefix
    mov edx, 0xffffffff
    add edx, 1
    or eax, DWORD PTR [esi]
    mov ecx, 0x99999999
finish or_prefix

fixture or_base_eax
    or eax, DWORD PTR [eax]
finish or_base_eax

fixture or_sib_eax
    or eax, DWORD PTR [eax + eax*2 - 4]
finish or_sib_eax

fixture or_base_ecx
    or ecx, DWORD PTR [ecx]
finish or_base_ecx

fixture or_sib_ecx
    or ecx, DWORD PTR [ecx + ecx*2 - 4]
finish or_sib_ecx

fixture or_base_edx
    or edx, DWORD PTR [edx]
finish or_base_edx

fixture or_sib_edx
    or edx, DWORD PTR [edx + edx*2 - 4]
finish or_sib_edx

fixture or_base_ebx
    or ebx, DWORD PTR [ebx]
finish or_base_ebx

fixture or_sib_ebx
    or ebx, DWORD PTR [ebx + ebx*2 - 4]
finish or_sib_ebx

fixture or_base_esp
    or esp, DWORD PTR [esp]
finish or_base_esp

fixture or_sib_esp
    or esp, DWORD PTR [esp + ecx*4 - 4]
finish or_sib_esp

fixture or_base_ebp
    or ebp, DWORD PTR [ebp]
finish or_base_ebp

fixture or_sib_ebp
    or ebp, DWORD PTR [ebp + ebp*2 - 4]
finish or_sib_ebp

fixture or_base_esi
    or esi, DWORD PTR [esi]
finish or_base_esi

fixture or_sib_esi
    or esi, DWORD PTR [esi + esi*2 - 4]
finish or_sib_esi

fixture or_base_edi
    or edi, DWORD PTR [edi]
finish or_base_edi

fixture or_sib_edi
    or edi, DWORD PTR [edi + edi*2 - 4]
finish or_sib_edi

fixture or_absolute
    or eax, DWORD PTR [0x4000]
finish or_absolute

fixture or_no_base
    or eax, DWORD PTR [ecx*4 + 0x4000]
finish or_no_base

fixture or_disp8_negative
    or eax, DWORD PTR [ebx - 128]
finish or_disp8_negative

fixture or_disp8_positive
    or eax, DWORD PTR [ebx + 127]
finish or_disp8_positive

fixture or_disp32
    or eax, DWORD PTR [ebx + 0x12345]
finish or_disp32

fixture or_base_wrap
    or eax, DWORD PTR [ebx + 8]
finish or_base_wrap

fixture or_index_wrap
    or eax, DWORD PTR [ebx + ecx*8]
finish or_index_wrap

fixture or_unaligned
    or eax, DWORD PTR [ebx + ecx*2 - 1]
finish or_unaligned

fixture xor
    xor eax, DWORD PTR [esi]
finish xor

fixture xor_prefix
    mov edx, 0xffffffff
    add edx, 1
    xor eax, DWORD PTR [esi]
    mov ecx, 0x99999999
finish xor_prefix

fixture xor_base_eax
    xor eax, DWORD PTR [eax]
finish xor_base_eax

fixture xor_sib_eax
    xor eax, DWORD PTR [eax + eax*2 - 4]
finish xor_sib_eax

fixture xor_base_ecx
    xor ecx, DWORD PTR [ecx]
finish xor_base_ecx

fixture xor_sib_ecx
    xor ecx, DWORD PTR [ecx + ecx*2 - 4]
finish xor_sib_ecx

fixture xor_base_edx
    xor edx, DWORD PTR [edx]
finish xor_base_edx

fixture xor_sib_edx
    xor edx, DWORD PTR [edx + edx*2 - 4]
finish xor_sib_edx

fixture xor_base_ebx
    xor ebx, DWORD PTR [ebx]
finish xor_base_ebx

fixture xor_sib_ebx
    xor ebx, DWORD PTR [ebx + ebx*2 - 4]
finish xor_sib_ebx

fixture xor_base_esp
    xor esp, DWORD PTR [esp]
finish xor_base_esp

fixture xor_sib_esp
    xor esp, DWORD PTR [esp + ecx*4 - 4]
finish xor_sib_esp

fixture xor_base_ebp
    xor ebp, DWORD PTR [ebp]
finish xor_base_ebp

fixture xor_sib_ebp
    xor ebp, DWORD PTR [ebp + ebp*2 - 4]
finish xor_sib_ebp

fixture xor_base_esi
    xor esi, DWORD PTR [esi]
finish xor_base_esi

fixture xor_sib_esi
    xor esi, DWORD PTR [esi + esi*2 - 4]
finish xor_sib_esi

fixture xor_base_edi
    xor edi, DWORD PTR [edi]
finish xor_base_edi

fixture xor_sib_edi
    xor edi, DWORD PTR [edi + edi*2 - 4]
finish xor_sib_edi

fixture xor_absolute
    xor eax, DWORD PTR [0x4000]
finish xor_absolute

fixture xor_no_base
    xor eax, DWORD PTR [ecx*4 + 0x4000]
finish xor_no_base

fixture xor_disp8_negative
    xor eax, DWORD PTR [ebx - 128]
finish xor_disp8_negative

fixture xor_disp8_positive
    xor eax, DWORD PTR [ebx + 127]
finish xor_disp8_positive

fixture xor_disp32
    xor eax, DWORD PTR [ebx + 0x12345]
finish xor_disp32

fixture xor_base_wrap
    xor eax, DWORD PTR [ebx + 8]
finish xor_base_wrap

fixture xor_index_wrap
    xor eax, DWORD PTR [ebx + ecx*8]
finish xor_index_wrap

fixture xor_unaligned
    xor eax, DWORD PTR [ebx + ecx*2 - 1]
finish xor_unaligned

.org 0x600, 0x90
.Lloop:
fixture fold_loop
    add eax, DWORD PTR [esi]
    xor eax, DWORD PTR [esi + 4]
    and eax, DWORD PTR [esi + 8]
    or eax, DWORD PTR [esi + 12]
    sub eax, DWORD PTR [esi + 16]
    cmp eax, DWORD PTR [esi + 20]
    jb .Lbelow
finish fold_loop

fixture fold_above
    mov edx, 1
    jmp .Ljoined
finish fold_above

.org 0x680, 0x90
.Lbelow:
fixture fold_below
    mov edx, 2
    jmp .Ljoined
finish fold_below

.org 0x6c0, 0x90
.Ljoined:
fixture fold_joined
    add ebx, edx
    lea esi, [esi + 24]
    sub ecx, 1
    jnz .Lloop
finish fold_joined

fixture fold_finish
    jmp .Ldone
finish fold_finish

.org 0x700, 0x90
.Ldone:
fixture fold_done
    ud2
finish fold_done
