.text
.globl _entry
_entry:
  xorl %eax, %eax
  ret
.section .rsrc,"dr"
.p2align 2
.Lroot:
  .long 0, 0
  .short 0, 0, 1, 1
  .long 0x80000000 + .Lname - .Lroot
  .long 0x80000000 + .Lids - .Lroot
  .long 10
  .long 0x80000000 + .Lids - .Lroot
.Lids:
  .long 0, 0
  .short 0, 0, 0, 1
  .long 7
  .long 0x80000000 + .Llanguages - .Lroot
.Llanguages:
  .long 0, 0
  .short 0, 0, 0, 1
  .long 1033
  .long .Ldata - .Lroot
.Ldata:
  .rva .Lpayload
  .long 4, 0, 0
.Lname:
  .short 3, 82, 51, 937
.Lpayload:
  .byte 0x12, 0x34, 0x56, 0x78
