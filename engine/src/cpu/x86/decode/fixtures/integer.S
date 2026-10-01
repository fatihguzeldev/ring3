.intel_syntax noprefix
.code32
.text
.globl ring3_integer_fixture
.type ring3_integer_fixture, @function
ring3_integer_fixture:
fixture_mov_imm:
    mov eax, 0x12345678
fixture_mov_reg:
    mov edi, eax
fixture_mov_load:
    mov eax, DWORD PTR [ebx + 16]
fixture_mov_store:
    mov DWORD PTR [ebx], eax
fixture_sib_load:
    mov edx, DWORD PTR [ebp + ecx * 4 - 32]
fixture_sib_store:
    mov DWORD PTR [esp + eax * 2 + 0x12345678], edx
fixture_movzx_byte:
    movzx eax, BYTE PTR [esi + 7]
fixture_movzx_word:
    movzx ecx, WORD PTR [edi - 2]
fixture_movsx_byte:
    movsx edx, BYTE PTR [ebx + ecx * 8 + 3]
fixture_movsx_word:
    movsx esi, WORD PTR [ebp + edx * 2 - 128]
fixture_lea:
    lea edi, [ebx + ecx * 4 - 32]
fixture_add_signed_imm8:
    add eax, -1
fixture_sub_imm8:
    sub DWORD PTR [ebx + 4], 127
fixture_cmp_imm32:
    cmp eax, 0x12345678
fixture_test:
    test eax, ebx
fixture_and:
    and eax, 0xff
fixture_or:
    or DWORD PTR [esp], edx
fixture_xor:
    xor ecx, ecx
fixture_inc:
    inc eax
fixture_dec:
    dec DWORD PTR [ebx]
fixture_not:
    not edx
fixture_neg:
    neg DWORD PTR [ebp + 8]
fixture_push_reg:
    push eax
fixture_push_memory:
    push DWORD PTR [esp + 4]
fixture_push_signed_imm8:
    push -1
fixture_pop_reg:
    pop ecx
fixture_pop_memory:
    pop DWORD PTR [edi]
fixture_call_direct:
    call .Lleaf
fixture_call_indirect_reg:
    call eax
fixture_call_indirect_memory:
    call DWORD PTR [ebx + 8]
fixture_jump_indirect_reg:
    jmp edx
fixture_jump_indirect_memory:
    jmp DWORD PTR [esp + 4]
fixture_jcc_short:
    je .Lshort
fixture_jump_short:
    jmp .Lshort
.Lshort:
fixture_jcc_near:
    jne .Lleaf
fixture_jump_near:
    jmp .Lleaf
fixture_padding:
    # padding makes both forward transfers use their near encoding.
    .fill 128, 1, 0x90
.Lleaf:
fixture_ret:
    ret
fixture_ret_imm:
    ret 12
.size ring3_integer_fixture, . - ring3_integer_fixture
.section .note.GNU-stack, "", @progbits
