.section .rsrc,"dr"
.p2align 2
.Lroot:
  .long 0, 0
  .short 0, 0, 0, 2
  .long 3, 0x80000000 + .Limages - .Lroot
  .long 14, 0x80000000 + .Lgroups - .Lroot
.Limages:
  .long 0, 0
  .short 0, 0, 0, 2
  .long 1, 0x80000000 + .Llanguage4 - .Lroot
  .long 2, 0x80000000 + .Llanguage8 - .Lroot
.Lgroups:
  .long 0, 0
  .short 0, 0, 0, 1
  .long 7, 0x80000000 + .LlanguageGroup - .Lroot
.Llanguage4:
  .long 0, 0
  .short 0, 0, 0, 1
  .long 1033, .Ldata4 - .Lroot
.Llanguage8:
  .long 0, 0
  .short 0, 0, 0, 1
  .long 1033, .Ldata8 - .Lroot
.LlanguageGroup:
  .long 0, 0
  .short 0, 0, 0, 1
  .long 1033, .LdataGroup - .Lroot
.Ldata4:
  .rva .Lbitmap4
  .long .Lend4 - .Lbitmap4, 0, 0
.Ldata8:
  .rva .Lbitmap8
  .long .Lend8 - .Lbitmap8, 0, 0
.LdataGroup:
  .rva .Lgroup
  .long .LgroupEnd - .Lgroup, 0, 0
.Lbitmap4:
  .long 40, 32, 64
  .short 1, 4
  .long 0, 512, 0, 0, 0, 0
  .fill 16, 4, 0x00112233
  .fill 512, 1, 0x12
  .fill 128, 1, 0x80
.Lend4:
.Lbitmap8:
  .long 40, 32, 64
  .short 1, 8
  .long 0, 0, 0, 0, 0, 0
  .fill 256, 4, 0x00553311
  .fill 1024, 1, 5
  .fill 128, 1, 0
.Lend8:
.Lgroup:
  .short 0, 1, 2
  .byte 32, 32, 16, 0
  .short 1, 4
  .long .Lend4 - .Lbitmap4
  .short 1
  .byte 32, 32, 0, 0
  .short 1, 8
  .long .Lend8 - .Lbitmap8
  .short 2
.LgroupEnd:
