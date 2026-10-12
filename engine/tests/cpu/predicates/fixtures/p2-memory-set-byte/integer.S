.text
.code32
.globl sete_memory
.type sete_memory,@function
sete_memory:
    .byte 0x0f,0x94,0x06
.size sete_memory,.-sete_memory
.globl setge_memory_ignored
.type setge_memory_ignored,@function
setge_memory_ignored:
    .byte 0x0f,0x9d,0x3c,0x24
.size setge_memory_ignored,.-setge_memory_ignored
