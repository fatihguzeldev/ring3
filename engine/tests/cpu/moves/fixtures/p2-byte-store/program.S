.intel_syntax noprefix
.text
.code32
.global byte_terminal_sequence, byte_call, byte_exit_canary, byte_canary_done
.type byte_terminal_sequence,@function
byte_terminal_sequence:
    lea ebx, [0x00403000]
.global byte_after_lea
byte_after_lea:
    mov eax, dword ptr [ebx]
    mov ecx, dword ptr [ebx + 4]
    mov byte ptr [ebx + 8], al
    mov byte ptr [ebx + 9], ah
    mov byte ptr [ebx + 10], cl
    mov byte ptr [ebx + 11], ch
    mov byte ptr [ebx + 12], 0x80
    mov byte ptr [ebx + 13], 0xff
.global byte_checkpoint
byte_checkpoint:
    mov edx, dword ptr [ebx + 8]
    mov ecx, dword ptr [ebx + 12]
    xor edx, ecx
    mov dword ptr [ebx + 16], edx
    push edx
byte_call:
    call dword ptr [0x00404050]
.type byte_exit_canary,@function
byte_exit_canary:
    mov dword ptr [ebx + 20], 0xdeadbeef
    jmp .Lbyte_canary_done
byte_canary_done:
.Lbyte_canary_done:
    ud2
.size byte_exit_canary,byte_canary_done-byte_exit_canary
.size byte_terminal_sequence,byte_exit_canary-byte_terminal_sequence

.macro finish name
    jmp .L\name\()_done
    .global \name\()_done
\name\()_done:
.L\name\()_done:
    ud2
    .size \name,\name\()_done-\name
.endm

.org 0x80, 0xcc
.global all_sources
.type all_sources,@function
all_sources:
    .byte 0x88, 0x05
    .long 0x2000
    mov byte ptr [0x2001], cl
    mov byte ptr [0x2002], dl
    mov byte ptr [0x2003], bl
    mov byte ptr [0x2004], ah
    mov byte ptr [0x2005], ch
    mov byte ptr [0x2006], dh
    mov byte ptr [0x2007], bh
    mov byte ptr [0x2008], 0
    mov byte ptr [0x2009], 0x80
    mov byte ptr [0x200a], 0xff
finish all_sources

.org 0xd0, 0xcc
.global effective_addresses
.type effective_addresses,@function
effective_addresses:
    mov byte ptr [eax], ah
    mov byte ptr [ebx + ecx * 4 + 0xbb7732e6], ch
    mov byte ptr [esp - 1], bh
    mov byte ptr [ecx * 2 + 0xddbbb980], dl
    mov byte ptr [esi + 0x10], 0xff
finish effective_addresses

.org 0x100, 0xcc
.global endpoint_page
.type endpoint_page,@function
endpoint_page:
    mov byte ptr [0x1fff], ah
finish endpoint_page
.org 0x110, 0xcc
.global endpoint_top
.type endpoint_top,@function
endpoint_top:
    mov byte ptr [0xffffffff], bh
finish endpoint_top

.macro prefix name, at
    .org \at, 0xcc
    .global \name, \name\()_store
    .type \name,@function
\name:
    lea edx, [edx + 1]
\name\()_store:
.endm
prefix fault_unmapped, 0x120
    mov byte ptr [ebx], ah
finish fault_unmapped
prefix fault_permission, 0x140
    mov byte ptr [esi + 4], 0x80
finish fault_permission

.org 0x160, 0xcc
.global smc_changed, smc_changed_store
.type smc_changed,@function
smc_changed:
    nop
smc_changed_store:
    mov byte ptr [0x8f00], 0x40
.global smc_changed_canary
smc_changed_canary:
    mov edi, 0xdeadbeef
finish smc_changed
.org 0x180, 0xcc
.global smc_same, smc_same_store
.type smc_same,@function
smc_same:
    nop
smc_same_store:
    mov byte ptr [0x8f00], 0x90
.global smc_same_canary
smc_same_canary:
    mov edi, 0xdeadbeef
finish smc_same
