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

fixture call_forward
call forward_target
finish call_forward
.space 251, 0x90
forward_target:
nop

fixture ret_plain
ret
finish ret_plain

fixture ret_imm_zero
ret 0
finish ret_imm_zero

fixture arithmetic_call
mov eax, 0xffffffff
add eax, 1
call arithmetic_target
finish arithmetic_call
.space 243, 0x90
arithmetic_target:
nop

fixture arithmetic_ret
mov eax, 0xffffffff
add eax, 1
ret
finish arithmetic_ret

fixture call_relative_five
call relative_five_target
finish call_relative_five
.space 5, 0x90
relative_five_target:
nop

fixture nested
mov eax, 1
call outer
nop
outer:
add eax, 2
call inner
add eax, 4
ret
inner:
add eax, 8
ret
finish nested

fixture recursive_sum
mov eax, 0
call recursive
nop
recursive:
cmp ecx, 0
je base
add eax, ecx
sub ecx, 1
call recursive
ret
base:
ret
finish recursive_sum
