.text
.globl _entry
_entry:
  movl $7, %eax
  addl $35, %eax
.Lloop:
  jmp .Lloop
