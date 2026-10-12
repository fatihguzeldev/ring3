.intel_syntax noprefix
.code32
.text
.globl adc_register_memory_byte
.type adc_register_memory_byte, @function
adc_register_memory_byte:
  adc al, byte ptr [esi]
.size adc_register_memory_byte, .-adc_register_memory_byte
.globl sbb_register_memory_byte
.type sbb_register_memory_byte, @function
sbb_register_memory_byte:
  sbb ah, byte ptr [esi]
.size sbb_register_memory_byte, .-sbb_register_memory_byte
