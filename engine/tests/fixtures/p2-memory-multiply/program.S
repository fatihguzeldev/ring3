.intel_syntax noprefix
.text
.code32
.global memory_terminal_sequence, memory_after_jno, memory_after_jo, memory_exit_canary
.type memory_terminal_sequence,@function
memory_terminal_sequence:
    mov ebx, 0x00403000
    mov eax, 7
    imul eax, dword ptr [ebx]
    mov dword ptr [ebx + 12], eax
    imul ecx, dword ptr [ebx + 4], -128
    jno .Lmemory_after_jno
    mov eax, 0xdeadbeef
.type memory_after_jno,@function
memory_after_jno:
.Lmemory_after_jno:
    mov edi, ebx
    imul edi, dword ptr [edi + 8], 0x00010000
    jo .Lmemory_after_jo
    mov eax, 0xbaadf00d
.size memory_after_jno,memory_after_jo-memory_after_jno
.type memory_after_jo,@function
memory_after_jo:
.Lmemory_after_jo:
    add eax, ecx
    xor eax, edi
    mov dword ptr [ebx + 16], eax
    push eax
    call dword ptr [0x00404050]
.size memory_after_jo,memory_exit_canary-memory_after_jo
.type memory_exit_canary,@function
memory_exit_canary:
    mov dword ptr [ebx + 20], 0xdeadbeef
    jmp .Lmemory_canary_done
memory_canary_done:
.Lmemory_canary_done:
    .byte 0x0f, 0x0b
.size memory_exit_canary,memory_canary_done-memory_exit_canary
.size memory_terminal_sequence,.-memory_terminal_sequence

.org 0x100, 0xcc
.global micro_none_alias, micro_none_imul, micro_none_done
.type micro_none_alias,@function
micro_none_alias:
    lea edx, [edx + 1]
micro_none_imul:
    imul eax, dword ptr [eax + ecx * 2 - 2]
    jmp .Lmicro_none_done
micro_none_done:
.Lmicro_none_done:
    .byte 0x0f, 0x0b
.size micro_none_alias,.-micro_none_alias

.org 0x140, 0xcc
.global micro_imm8_alias, micro_imm8_imul, micro_imm8_done
.type micro_imm8_alias,@function
micro_imm8_alias:
    lea edx, [edx + 1]
micro_imm8_imul:
    imul ecx, dword ptr [ebx + ecx * 4 - 2], -128
    jmp .Lmicro_imm8_done
micro_imm8_done:
.Lmicro_imm8_done:
    .byte 0x0f, 0x0b
.size micro_imm8_alias,.-micro_imm8_alias

.org 0x180, 0xcc
.global micro_zero_alias, micro_zero_imul, micro_zero_done
.type micro_zero_alias,@function
micro_zero_alias:
    lea edx, [edx + 1]
micro_zero_imul:
    .byte 0x69, 0x64, 0x24, 0x08
    .long 0
    jmp .Lmicro_zero_done
micro_zero_done:
.Lmicro_zero_done:
    .byte 0x0f, 0x0b
.size micro_zero_alias,.-micro_zero_alias

.org 0x1c0, 0xcc
.global micro_last_dword, micro_last_imul, micro_last_done
.type micro_last_dword,@function
micro_last_dword:
    lea edx, [edx + 1]
micro_last_imul:
    imul edi, dword ptr [edi], 0x00010000
    jmp .Lmicro_last_done
micro_last_done:
.Lmicro_last_done:
    .byte 0x0f, 0x0b
.size micro_last_dword,.-micro_last_dword
