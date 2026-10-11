.text
.globl _entry
_entry:
  xorl %eax, %eax
  ret
.data
.p2align 3
ring3_anchor:
  .long 7
.p2align 3
ring3_pointers:
  .long ring3_anchor
  .long ring3_anchor
  .zero 4096
  .long ring3_anchor
