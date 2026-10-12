.intel_syntax noprefix
.code32
.text

.globl data_bss
.type data_bss, @function
data_bss:
    mov eax, dword ptr [0x00403000]
    mov ecx, dword ptr [0x00403200]
    lea eax, [eax + 5]
    mov dword ptr [0x00403200], eax
    jmp data_done
data_done:
    ud2
.size data_bss, .-data_bss

.globl bounded_loop
.type bounded_loop, @function
bounded_loop:
    mov ecx, 3
    mov eax, 0
loop_body:
    lea eax, [eax + 2]
    sub ecx, 1
    jnz loop_body
loop_finish:
    jmp loop_done
loop_done:
    ud2
.size bounded_loop, .-bounded_loop
