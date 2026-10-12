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

fixture zx_byte_eax
    movzx eax, BYTE PTR [eax]
end_fixture zx_byte_eax
fixture sx_byte_ecx
    movsx ecx, BYTE PTR [ebx + esi*2 - 1]
end_fixture sx_byte_ecx
fixture zx_word_edx
    movzx edx, WORD PTR [edx + ecx*4 + 127]
end_fixture zx_word_edx
fixture sx_word_ebx
    movsx ebx, WORD PTR [ebx + ebx*8 - 128]
end_fixture sx_word_ebx
fixture zx_byte_esp
    movzx esp, BYTE PTR [esp]
end_fixture zx_byte_esp
fixture sx_byte_ebp
    movsx ebp, BYTE PTR [ebp + edi]
end_fixture sx_byte_ebp
fixture zx_word_esi
    movzx esi, WORD PTR [esi*4 + 0x4000]
end_fixture zx_word_esi
fixture sx_word_edi
    movsx edi, WORD PTR [0x4000]
end_fixture sx_word_edi

fixture zx_byte
    movzx eax, BYTE PTR [esi]
end_fixture zx_byte
fixture sx_byte
    movsx eax, BYTE PTR [esi]
end_fixture sx_byte
fixture zx_word
    movzx eax, WORD PTR [esi]
end_fixture zx_word
fixture sx_word
    movsx eax, WORD PTR [esi]
end_fixture sx_word
fixture prefix_fault
    lea eax, [ebx + ecx*4 - 1]
    movsx edx, WORD PTR [esi]
    nop
end_fixture prefix_fault
fixture scripted_byte
    mov edx, 0x12345678
    movsx eax, BYTE PTR [esi]
    mov ecx, 0x99999999
end_fixture scripted_byte
fixture scripted_word
    mov edx, 0x12345678
    movsx eax, WORD PTR [esi]
    mov ecx, 0x99999999
end_fixture scripted_word

.org 0x200, 0x90
fixture composition
    movzx eax, BYTE PTR [esi]
    movsx edx, BYTE PTR [esi + 1]
    movsx ebx, WORD PTR [esi + 2]
    add eax, edx
    xor eax, ebx
    movzx edi, WORD PTR [esi + 4]
    jmp .Ldone
end_fixture composition

.org 0x500, 0x90
fixture composition_done
.Ldone:
    ud2
end_fixture composition_done
