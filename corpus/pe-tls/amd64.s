.text
.globl entry
entry:
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
  .quad entry
  .quad 0
.p2align 3
.globl _tls_used
_tls_used:
  .quad template_start
  .quad template_end
  .quad tls_index
  .quad callbacks
  .long 7
  .long 0
