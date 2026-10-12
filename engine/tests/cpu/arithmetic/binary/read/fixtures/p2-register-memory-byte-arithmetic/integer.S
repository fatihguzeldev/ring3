.intel_syntax noprefix
.code32
.text
.globl add_register_memory_byte
.type add_register_memory_byte, @function
add_register_memory_byte:
  add al, byte ptr [esi]
.size add_register_memory_byte, .-add_register_memory_byte
.globl sub_register_memory_byte
.type sub_register_memory_byte, @function
sub_register_memory_byte:
  sub ah, byte ptr [esi]
.size sub_register_memory_byte, .-sub_register_memory_byte
