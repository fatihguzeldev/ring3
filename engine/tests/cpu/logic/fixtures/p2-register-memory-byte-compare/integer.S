.intel_syntax noprefix
.text
.globl cmp_al_memory
.type cmp_al_memory,@function
cmp_al_memory:
  .byte 0x3a, 0x06
.size cmp_al_memory, .-cmp_al_memory
.section .note.GNU-stack,"",@progbits
