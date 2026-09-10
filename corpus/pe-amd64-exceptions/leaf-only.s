.text
.p2align 4
.globl leaf_probe
.def leaf_probe; .scl 2; .type 32; .endef
leaf_probe:
    movl $12, %eax
    retq
