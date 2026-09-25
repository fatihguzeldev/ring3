.intel_syntax noprefix
.text
# iterative max heap; locals hold exclusive heap count and next parent.
push ebp
mov ebp, esp
push ebx
push esi
push edi
sub esp, 16
mov ebx, [ebp + 8]
mov eax, [ebp + 12]
mov [ebp - 16], eax
shr eax, 1
mov [ebp - 20], eax
.Lnext:
cmp dword ptr [ebp - 20], 0
je .Lextract
dec dword ptr [ebp - 20]
mov esi, [ebp - 20]
jmp .Lsift
.Lextract:
dec dword ptr [ebp - 16]
jz .Ldone
xor esi, esi
mov edi, [ebp - 16]
call .Lswap
.Lsift:
lea edi, [esi * 2 + 1]
cmp edi, [ebp - 16]
jae .Lnext
lea edx, [edi + 1]
cmp edx, [ebp - 16]
jae .Lparent
mov eax, edi
call .Lcompare
test eax, eax
jge .Lparent
inc edi
.Lparent:
mov eax, esi
mov edx, edi
call .Lcompare
test eax, eax
jge .Lnext
call .Lswap
mov esi, edi
jmp .Lsift
.Ldone:
add esp, 16
pop edi
pop esi
pop ebx
pop ebp
xor eax, eax
ret
# indices become element addresses only; no exclusive end pointer wraps.
.Lcompare:
imul eax, dword ptr [ebp + 16]
add eax, ebx
imul edx, dword ptr [ebp + 16]
add edx, ebx
push edx
push eax
call dword ptr [ebp + 20]
add esp, 8
ret
.Lswap:
push esi
push edi
imul esi, dword ptr [ebp + 16]
add esi, ebx
imul edi, dword ptr [ebp + 16]
add edi, ebx
mov ecx, [ebp + 16]
.Lbyte:
mov al, [esi]
mov dl, [edi]
mov [esi], dl
mov [edi], al
inc esi
inc edi
dec ecx
jnz .Lbyte
pop edi
pop esi
ret
