.intel_syntax noprefix
.code32
.text
.globl adc_memory_reg
.type adc_memory_reg, @function
adc_memory_reg:
  adc byte ptr [esi], ah
.size adc_memory_reg, .-adc_memory_reg
.globl adc_memory_imm
.type adc_memory_imm, @function
adc_memory_imm:
  adc byte ptr [esi], 0x80
.size adc_memory_imm, .-adc_memory_imm
.globl sbb_memory_reg
.type sbb_memory_reg, @function
sbb_memory_reg:
  sbb byte ptr [esi], ah
.size sbb_memory_reg, .-sbb_memory_reg
.globl sbb_memory_imm
.type sbb_memory_imm, @function
sbb_memory_imm:
  sbb byte ptr [esi], 0x80
.size sbb_memory_imm, .-sbb_memory_imm
