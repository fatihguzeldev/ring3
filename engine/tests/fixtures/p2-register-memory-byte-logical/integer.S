.intel_syntax noprefix
.code32
.text
.globl and_register_memory_byte
.type and_register_memory_byte, @function
and_register_memory_byte:
  and al, byte ptr [esi]
.size and_register_memory_byte, .-and_register_memory_byte
.globl or_register_memory_byte
.type or_register_memory_byte, @function
or_register_memory_byte:
  or ah, byte ptr [esi]
.size or_register_memory_byte, .-or_register_memory_byte
.globl xor_register_memory_byte
.type xor_register_memory_byte, @function
xor_register_memory_byte:
  xor ch, byte ptr [esi]
.size xor_register_memory_byte, .-xor_register_memory_byte
