.section .rsrc,"dr"
.p2align 2
.Lroot:
  .long 0, 0
  .short 0, 0, 0, 1
  .long 6, 0x80000000 + .Lnames - .Lroot
.Lnames:
  .long 0, 0
  .short 0, 0, 0, 1
  .long 1, 0x80000000 + .Llanguages - .Lroot
.Llanguages:
  .long 0, 0
  .short 0, 0, 0, 1
  .long 1033, .Ldata - .Lroot
.Ldata:
  .rva .Lpayload
  .long .Lend - .Lpayload, 0, 0
.Lpayload:
  .short 5, 65, 108, 112, 104, 97
  .short 4, 66, 101, 116, 97
  .zero 28
.Lend:
