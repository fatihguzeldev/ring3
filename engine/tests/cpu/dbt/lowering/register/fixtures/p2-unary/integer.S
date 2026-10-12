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

fixture inc_short
    inc eax
end_fixture inc_short
fixture inc_modrm
    .byte 0xff, 0xc0 # INC EAX, explicit FF /0 encoding
end_fixture inc_modrm
fixture dec_short
    dec eax
end_fixture dec_short
fixture dec_modrm
    .byte 0xff, 0xc8 # DEC EAX, explicit FF /1 encoding
end_fixture dec_modrm
fixture not_eax
    not eax
end_fixture not_eax
fixture neg_eax
    neg eax
end_fixture neg_eax
fixture inc_ecx
    inc ecx
end_fixture inc_ecx
fixture dec_edx
    dec edx
end_fixture dec_edx
fixture not_ebx
    not ebx
end_fixture not_ebx
fixture neg_esp
    neg esp
end_fixture neg_esp
fixture inc_ebp_modrm
    .byte 0xff, 0xc5
end_fixture inc_ebp_modrm
fixture dec_esi
    dec esi
end_fixture dec_esi
fixture not_edi
    not edi
end_fixture not_edi
fixture checkpoint
    inc eax
    dec esp
    not eax
    neg esp
end_fixture checkpoint

fixture inc_jc
    inc eax
    jc .Linc_taken
end_fixture inc_jc
    .space 2, 0x90
.Linc_taken:
fixture dec_jnc
    dec eax
    jnc .Ldec_taken
end_fixture dec_jnc
    .space 2, 0x90
.Ldec_taken:
fixture neg_jc
    neg eax
    jc .Lneg_taken
end_fixture neg_jc
    .space 2, 0x90
.Lneg_taken:
fixture not_jc
    not eax
    jc .Lnot_taken
end_fixture not_jc
    .space 2, 0x90
.Lnot_taken:

.org 0x200, 0x90
fixture counter_entry
    mov eax, 0
end_fixture counter_entry
fixture counter_guard
    test ecx, ecx
    je .Lcounter_done
end_fixture counter_guard
fixture counter_body
.Lcounter_body:
    inc eax
    dec ecx
    jnz .Lcounter_body
end_fixture counter_body
fixture counter_done
.Lcounter_done:
    ud2
end_fixture counter_done

.org 0x300, 0x90
fixture embedded_checkpoint
    inc eax
    dec esp
    not eax
    neg esp
    jmp .Lmissing
end_fixture embedded_checkpoint

.org 0x400, 0x90
fixture unary_fault
    neg eax
    mov edx, DWORD PTR [esi]
    jmp .Lmissing
end_fixture unary_fault

.org 0x500, 0x90
.Lmissing:
    ud2
