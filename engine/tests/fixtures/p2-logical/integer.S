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

# Explicit alternate encodings are independently disassembled by LLVM.
fixture and_rm_reg
    and eax, ebx
end_fixture and_rm_reg
fixture and_reg_rm
    .byte 0x23, 0xc3 # AND EAX, EBX; register destination opcode direction
end_fixture and_reg_rm
fixture and_eax_imm32
    and eax, 0x80000103
end_fixture and_eax_imm32
fixture and_modrm_imm32
    .byte 0x81, 0xe0, 0x03, 0x01, 0x00, 0x80
end_fixture and_modrm_imm32
fixture and_imm8_min
    and eax, -128
end_fixture and_imm8_min
fixture and_imm8_minus1
    and eax, -1
end_fixture and_imm8_minus1
fixture and_imm8_max
    and eax, 127
end_fixture and_imm8_max

fixture or_rm_reg
    or eax, ebx
end_fixture or_rm_reg
fixture or_reg_rm
    .byte 0x0b, 0xc3
end_fixture or_reg_rm
fixture or_eax_imm32
    or eax, 0x80000103
end_fixture or_eax_imm32
fixture or_modrm_imm32
    .byte 0x81, 0xc8, 0x03, 0x01, 0x00, 0x80
end_fixture or_modrm_imm32
fixture or_imm8_min
    or eax, -128
end_fixture or_imm8_min
fixture or_imm8_minus1
    or eax, -1
end_fixture or_imm8_minus1
fixture or_imm8_max
    or eax, 127
end_fixture or_imm8_max

fixture xor_rm_reg
    xor eax, ebx
end_fixture xor_rm_reg
fixture xor_reg_rm
    .byte 0x33, 0xc3
end_fixture xor_reg_rm
fixture xor_eax_imm32
    xor eax, 0x80000103
end_fixture xor_eax_imm32
fixture xor_modrm_imm32
    .byte 0x81, 0xf0, 0x03, 0x01, 0x00, 0x80
end_fixture xor_modrm_imm32
fixture xor_imm8_min
    xor eax, -128
end_fixture xor_imm8_min
fixture xor_imm8_minus1
    xor eax, -1
end_fixture xor_imm8_minus1
fixture xor_imm8_max
    xor eax, 127
end_fixture xor_imm8_max

fixture test_rm_reg
    test eax, ebx
end_fixture test_rm_reg
fixture test_eax_imm32
    test eax, 0x80000103
end_fixture test_eax_imm32
fixture test_modrm_imm32
    .byte 0xf7, 0xc0, 0x03, 0x01, 0x00, 0x80
end_fixture test_modrm_imm32

fixture and_esp_same
    and esp, esp
end_fixture and_esp_same
fixture or_esp_eax
    or esp, eax
end_fixture or_esp_eax
fixture xor_eax_esp
    xor eax, esp
end_fixture xor_eax_esp
fixture test_esp_same
    test esp, esp
end_fixture test_esp_same
fixture checkpoint
    and eax, ebx
    or esp, eax
    test eax, eax
end_fixture checkpoint

.macro condition name, mnemonic
fixture \name
    test eax, eax
    \mnemonic .Ltaken\@
end_fixture \name
    .space 2, 0x90
.Ltaken\@:
.endm
condition jcc_0, jo
condition jcc_1, jno
condition jcc_2, jb
condition jcc_3, jae
condition jcc_4, je
condition jcc_5, jne
condition jcc_6, jbe
condition jcc_7, ja
condition jcc_8, js
condition jcc_9, jns
condition jcc_10, jp
condition jcc_11, jnp
condition jcc_12, jl
condition jcc_13, jge
condition jcc_14, jle
condition jcc_15, jg

.org 0x200, 0x90
fixture mask_entry
    xor eax, eax
end_fixture mask_entry
fixture mask_guard
.Lguard:
    test ecx, ecx
    je .Ldone
end_fixture mask_guard
fixture mask_body
    mov edx, ecx
    and edx, 255
    or edx, 256
    xor edx, ebx
    add eax, edx
    sub ecx, 1
    jmp .Lguard
end_fixture mask_body
fixture mask_done
.Ldone:
    ud2
end_fixture mask_done

.org 0x300, 0x90
fixture logical_fault
    xor eax, eax
    mov edx, DWORD PTR [esi]
    jmp .Lmissing
end_fixture logical_fault
.org 0x400, 0x90
.Lmissing:
    ud2
