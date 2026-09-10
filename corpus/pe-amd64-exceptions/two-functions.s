.text
.p2align 4
.globl frame_small
.def frame_small; .scl 2; .type 32; .endef
.seh_proc frame_small
frame_small:
    subq $40, %rsp
    .seh_stackalloc 40
    .seh_endprologue
    movl $12, %eax
    addq $40, %rsp
    retq
.seh_endproc

.p2align 4
.globl frame_saved
.def frame_saved; .scl 2; .type 32; .endef
.seh_proc frame_saved
frame_saved:
    pushq %rbx
    .seh_pushreg %rbx
    subq $32, %rsp
    .seh_stackalloc 32
    .seh_endprologue
    movl $7, %ebx
    movl %ebx, %eax
    addq $32, %rsp
    popq %rbx
    retq
.seh_endproc
