.text
.globl _entry
_entry:
  pushl $35
  pushl $7
  call _sum
  addl $8, %esp
  movl %eax, _result
  int3

_sum:
  pushl %ebp
  movl %esp, %ebp
  movl 8(%ebp), %eax
  addl 12(%ebp), %eax
  leave
  ret

.data
_result:
  .long 0
