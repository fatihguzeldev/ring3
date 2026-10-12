.intel_syntax noprefix
.code32
.text
.globl moffs_load
.type moffs_load,@function
moffs_load:
    mov al, byte ptr [0x89abcdef]
.size moffs_load,.-moffs_load
.globl moffs_store
.type moffs_store,@function
moffs_store:
    mov byte ptr [0x89abcdef], al
.size moffs_store,.-moffs_store
