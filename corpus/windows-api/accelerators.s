.section .rsrc,"dr"
.p2align 2
.Lroot:
  .long 0, 0
  .short 0, 0, 0, 1
  .long 9, 0x80000000 + .Lnames - .Lroot
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
  .short 9, 65, 100, 0xdead
  .short 0, 120, 200, 0xbeef
  .short 0x89, 65, 100, 0
.Lend:
