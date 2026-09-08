.text
.globl _entry
_entry:
  xorl %eax, %eax
  ret
.section .tls,"dw"
.p2align 4
template_start:
  .byte 0x12, 0x34, 0x56, 0x78, 0x9a
template_end:
.data
.p2align 2
tls_index:
  .long 0
.section .rdata,"dr"
.p2align 3
callbacks:
  .long _entry
  .long 0
.p2align 3
.globl __tls_used
__tls_used:
  .long template_start
  .long template_end
  .long tls_index
  .long callbacks
  .long 7
  .long 0
