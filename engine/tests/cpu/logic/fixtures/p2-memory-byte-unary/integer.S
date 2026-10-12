.intel_syntax noprefix
.text
.global inc_memory
.type inc_memory,@function
inc_memory:
  inc byte ptr [esi]
.size inc_memory,.-inc_memory
.global dec_memory
.type dec_memory,@function
dec_memory:
  dec byte ptr [esi]
.size dec_memory,.-dec_memory
.global neg_memory
.type neg_memory,@function
neg_memory:
  neg byte ptr [esi]
.size neg_memory,.-neg_memory
.global not_memory
.type not_memory,@function
not_memory:
  not byte ptr [esi]
.size not_memory,.-not_memory
