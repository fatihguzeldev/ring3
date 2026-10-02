.intel_syntax noprefix
.code32
.text

.macro fixture name
.globl \name
.type \name, @function
\name:
.endm
.macro end_fixture name
.size \name, .-\name
.endm

fixture lea_eax_base
    lea eax, [ebx]
end_fixture lea_eax_base
fixture lea_ecx_index2
    lea ecx, [edx*2 + 0x12345678]
end_fixture lea_ecx_index2
fixture lea_edx_scale4
    lea edx, [ebx + esi*4 - 128]
end_fixture lea_edx_scale4
fixture lea_ebx_scale8
    lea ebx, [ecx + edi*8 + 0x12345678]
end_fixture lea_ebx_scale8
fixture lea_esp_baseesp
    lea esp, [esp + eax - 1]
end_fixture lea_esp_baseesp
fixture lea_ebp_index_alias
    lea ebp, [edx + ebp*2 + 127]
end_fixture lea_ebp_index_alias
fixture lea_esi_base_alias
    lea esi, [esi + ecx*4 - 2147483648]
end_fixture lea_esi_base_alias
fixture lea_edi_both_alias
    lea edi, [edi + edi*8 + 7]
end_fixture lea_edi_both_alias
fixture lea_no_base_index1
    # SIB index ESI, scale1, no base; LLVM verifies this explicit form.
    .byte 0x8d, 0x04, 0x35, 0x21, 0x43, 0x65, 0x87
end_fixture lea_no_base_index1
fixture lea_absolute
    lea eax, [0xffffffff]
end_fixture lea_absolute
fixture lea_index8_only
    lea eax, [ebx*8 - 1]
end_fixture lea_index8_only
fixture lea_ebp_no_index
    lea eax, [ebp]
end_fixture lea_ebp_no_index
fixture lea_esp_no_index
    lea eax, [esp]
end_fixture lea_esp_no_index
fixture lea_disp32_positive
    lea eax, [ebx + 0x7fffffff]
end_fixture lea_disp32_positive
fixture lea_disp32_negative
    lea eax, [ebx - 2147483648]
end_fixture lea_disp32_negative
fixture checkpoint
    lea eax, [ebx + ecx*4 - 1]
    lea esp, [esp + eax]
    lea eax, [eax + eax*2]
end_fixture checkpoint

.org 0x200, 0x90
fixture embedded_checkpoint
    lea eax, [ebx + ecx*4 - 1]
    lea esp, [esp + eax]
    lea eax, [eax + eax*2]
    jmp .Lmissing
end_fixture embedded_checkpoint

.org 0x300, 0x90
fixture lea_fault
    lea eax, [ebx + ecx*8 - 1]
    mov edx, DWORD PTR [eax]
    jmp .Lmissing
end_fixture lea_fault

.org 0x400, 0x90
.Lmissing:
    ud2
