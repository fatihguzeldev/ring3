.intel_syntax noprefix
.code32
.text
.global byte_load_sequence, byte_load_call, byte_load_canary, byte_load_done
.type byte_load_sequence,@function
byte_load_sequence:
    mov eax, 0x12345678
    mov ecx, 0x23456789
    mov edx, 0x3456789a
    mov ebx, 0x456789ab
    lea esi, [0x00403000]
    lea edi, [0x00403010]
    mov al, byte ptr [esi]
.global byte_after_al
byte_after_al:
    mov byte ptr [edi], al
    mov cl, byte ptr [esi + 1]
.global byte_after_cl
byte_after_cl:
    mov byte ptr [edi + 1], cl
    mov dl, byte ptr [esi + 2]
.global byte_after_dl
byte_after_dl:
    mov byte ptr [edi + 2], dl
    mov bl, byte ptr [esi + 3]
.global byte_after_bl
byte_after_bl:
    mov byte ptr [edi + 3], bl
    mov ah, byte ptr [esi + 4]
.global byte_after_ah
byte_after_ah:
    mov byte ptr [edi + 4], ah
    mov ch, byte ptr [esi + 5]
.global byte_after_ch
byte_after_ch:
    mov byte ptr [edi + 5], ch
    mov dh, byte ptr [esi + 6]
.global byte_after_dh
byte_after_dh:
    mov byte ptr [edi + 6], dh
    mov bh, byte ptr [esi + 7]
.global byte_after_bh
byte_after_bh:
    mov byte ptr [edi + 7], bh
    xor eax, ecx
    xor eax, edx
    xor eax, ebx
    mov dword ptr [edi + 8], eax
    push eax
byte_load_call:
    call dword ptr [0x00404050]
.type byte_load_canary,@function
byte_load_canary:
    mov dword ptr [edi + 12], 0xa5a5a5a5
    jmp .Lbyte_load_done
byte_load_done:
.Lbyte_load_done:
    ud2
.size byte_load_sequence,byte_load_canary-byte_load_sequence
.size byte_load_canary,byte_load_done-byte_load_canary

.macro finish name
    jmp .L\name\()_done
    .global \name\()_done
\name\()_done:
.L\name\()_done:
    ud2
    .size \name,\name\()_done-\name
.endm

.org 0x100, 0xcc
.global all_byte_destinations, all_byte_destinations_checkpoint
.type all_byte_destinations,@function
all_byte_destinations:
    .byte 0x8a, 0x05
    .long 0x2000
    mov cl, byte ptr [0x2001]
    mov dl, byte ptr [0x2002]
    mov bl, byte ptr [0x2003]
    mov ah, byte ptr [0x2004]
    mov ch, byte ptr [0x2005]
    mov dh, byte ptr [0x2006]
    mov bh, byte ptr [0x2007]
all_byte_destinations_checkpoint:
    movzx esi, byte ptr [0x3000]
    movsx edi, word ptr [0x3002]
    .byte 0x88, 0x05
    .long 0x3004
finish all_byte_destinations

.org 0x150, 0xcc
.global original_effective_addresses
.type original_effective_addresses,@function
original_effective_addresses:
    mov al, byte ptr [eax]
    mov ah, byte ptr [eax - 0x31]
    mov cl, byte ptr [ebx + ecx * 4 - 0x10]
    .byte 0x8a, 0x34, 0x55
    .long -0x20
    mov bh, byte ptr [esp - 1]
finish original_effective_addresses

.org 0x180, 0xcc
.global byte_endpoint_page
.type byte_endpoint_page,@function
byte_endpoint_page:
    mov ah, byte ptr [0x1fff]
finish byte_endpoint_page

.org 0x190, 0xcc
.global byte_endpoint_top
.type byte_endpoint_top,@function
byte_endpoint_top:
    mov bh, byte ptr [0xffffffff]
finish byte_endpoint_top

.macro fault name, at
    .org \at, 0xcc
    .global \name, \name\()_load
    .type \name,@function
\name:
    lea edx, [edx + 1]
\name\()_load:
    mov ah, byte ptr [ebx]
finish \name
.endm
fault byte_fault_permission, 0x1a0
fault byte_fault_unmapped, 0x1b0
