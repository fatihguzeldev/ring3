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

fixture zx_byte_al
    movzx eax, al
end_fixture zx_byte_al

fixture zx_byte_cl
    movzx ecx, cl
end_fixture zx_byte_cl

fixture zx_byte_dl
    movzx edx, dl
end_fixture zx_byte_dl

fixture zx_byte_bl
    movzx ebx, bl
end_fixture zx_byte_bl

fixture zx_byte_ah
    movzx eax, ah
end_fixture zx_byte_ah

fixture zx_byte_ch
    movzx ebp, ch
end_fixture zx_byte_ch

fixture zx_byte_dh
    movzx esi, dh
end_fixture zx_byte_dh

fixture zx_byte_bh
    movzx edi, bh
end_fixture zx_byte_bh

fixture zx_word_ax
    movzx eax, ax
end_fixture zx_word_ax

fixture zx_word_cx
    movzx ecx, cx
end_fixture zx_word_cx

fixture zx_word_dx
    movzx edx, dx
end_fixture zx_word_dx

fixture zx_word_bx
    movzx ebx, bx
end_fixture zx_word_bx

fixture zx_word_sp
    movzx esp, sp
end_fixture zx_word_sp

fixture zx_word_bp
    movzx ebp, bp
end_fixture zx_word_bp

fixture zx_word_si
    movzx esi, si
end_fixture zx_word_si

fixture zx_word_di
    movzx edi, di
end_fixture zx_word_di

fixture sx_byte_al
    movsx eax, al
end_fixture sx_byte_al

fixture sx_byte_cl
    movsx ecx, cl
end_fixture sx_byte_cl

fixture sx_byte_dl
    movsx edx, dl
end_fixture sx_byte_dl

fixture sx_byte_bl
    movsx ebx, bl
end_fixture sx_byte_bl

fixture sx_byte_ah
    movsx eax, ah
end_fixture sx_byte_ah

fixture sx_byte_ch
    movsx ebp, ch
end_fixture sx_byte_ch

fixture sx_byte_dh
    movsx esi, dh
end_fixture sx_byte_dh

fixture sx_byte_bh
    movsx edi, bh
end_fixture sx_byte_bh

fixture sx_word_ax
    movsx eax, ax
end_fixture sx_word_ax

fixture sx_word_cx
    movsx ecx, cx
end_fixture sx_word_cx

fixture sx_word_dx
    movsx edx, dx
end_fixture sx_word_dx

fixture sx_word_bx
    movsx ebx, bx
end_fixture sx_word_bx

fixture sx_word_sp
    movsx esp, sp
end_fixture sx_word_sp

fixture sx_word_bp
    movsx ebp, bp
end_fixture sx_word_bp

fixture sx_word_si
    movsx esi, si
end_fixture sx_word_si

fixture sx_word_di
    movsx edi, di
end_fixture sx_word_di

fixture checkpoint
    movsx eax, ah
    movzx esp, sp
    movsx esp, sp
    movzx eax, al
end_fixture checkpoint

fixture sx_ah_jc
    movsx eax, ah
    jc .Lcarry
end_fixture sx_ah_jc
    .space 2, 0x90
.Lcarry:
fixture zx_sp_jno
    movzx esp, sp
    jno .Loverflow
end_fixture zx_sp_jno
    .space 2, 0x90
.Loverflow:
fixture sx_di_jz
    movsx edi, di
    jz .Lzero
end_fixture sx_di_jz
    .space 2, 0x90
.Lzero:
fixture zx_cl_js
    movzx ecx, cl
    js .Lsign
end_fixture zx_cl_js
    .space 2, 0x90
.Lsign:

.org 0x200, 0x90
fixture composition
    movzx eax, cl
    movsx edx, ch
    movsx ebx, cx
    add eax, edx
    xor eax, ebx
    movzx esi, ax
    movsx edi, si
    jmp .Ldone
end_fixture composition

.org 0x300, 0x90
fixture embedded_checkpoint
    movsx eax, ah
    movzx esp, sp
    movsx esp, sp
    movzx eax, al
    jmp .Ldone
end_fixture embedded_checkpoint

.org 0x400, 0x90
fixture extension_fault
    movsx eax, ah
    mov edx, DWORD PTR [esi]
    jmp .Ldone
end_fixture extension_fault

.org 0x500, 0x90
fixture composition_done
.Ldone:
    ud2
end_fixture composition_done
