.text
.globl entry
entry:
  xorl %eax, %eax
  retq

.section .rdata,"dr"
.p2align 3
.space 16
.globl _load_config_used
_load_config_used:
  .long 112
  .long 0x87654321
  .short 9, 11
  .long 0x80000001
  .long 0x40000002
  .long 0x10203040
  .space 88
