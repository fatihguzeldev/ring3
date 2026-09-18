.text
.globl _entry
_entry:
  xorl %eax, %eax
  retl

.section .rdata,"dr"
.p2align 3
.space 16
.globl __load_config_used
__load_config_used:
  .long 72
  .long 0x12345678
  .short 3, 7
  .long 0x00001000
  .long 0x00002000
  .long 0x00abcdef
  .space 48
