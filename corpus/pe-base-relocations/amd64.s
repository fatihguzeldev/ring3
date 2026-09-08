.text
.globl entry
entry:
  xorl %eax, %eax
  ret
.data
.p2align 3
ring3_anchor:
  .long 7
.p2align 3
ring3_pointers:
  .quad ring3_anchor
  .quad ring3_anchor
  .zero 4096
  .quad ring3_anchor
