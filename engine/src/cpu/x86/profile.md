# Ring3 IA-32 software CPU profile

This document describes the CPU implemented by the decoder, typed IR, and generated WebAssembly. The target is 32-bit protected user code with eight general registers, eight physical x87/MMX registers, and XMM0–7. It is an explicit software profile, not a physical processor model or a claim that every IA-32 encoding, application, operating system, or input has been verified.

A mnemonic below names a family. Legal operand widths, directions, prefixes, and encoded forms are selected by `decode/profile.rs` and the family lowerers. Mnemonic counts are not encoding counts or compatibility percentages. Original compiler witnesses and native tests establish the particular paths they exercise; they do not prove every program using a supported mnemonic.

## Decode and execution boundary

`decode/decoder.rs` fetches through execute permissions, consumes at most the architectural 15-byte instruction limit, and lowers the decoded instruction to `ir::Operation`. Compilation captures bounded blocks and code versions. Generated Wasm executes that IR without fetching or decoding guest instructions at runtime. Replacement and resident captures share instruction semantics and use distinct owner guards. Cold exits, bounded retirement/work, cancellation, callbacks, and executable-write invalidation are process mechanisms around the CPU.

Operand size 16/32 and address size 16/32 are distinct. `67` selects 16-bit effective offsets, including BX/BP/SI/DI combinations and low-word index/count updates. Segment prefixes retain their selected segment in the effective address; ignored segment prefixes on register-only instructions do not create a memory access. Prefix payload bytes inside immediates or displacements remain payload. Prefix acceptance is family-specific: an ignored prefix on one family does not authorize arbitrary repetitions, LOCK on registers, or another opcode.

## Integer, control, and atomic families

| Family | Implemented forms and distinctions |
|---|---|
| Data and addressing | MOV register/immediate/memory in byte, word, and dword forms; MOVZX/MOVSX byte/word extension; LEA16/32; XCHG8/16/32; BSWAP32; XLAT with address-size and segment selection. Byte registers include AL/CL/DL/BL and AH/CH/DH/BH. Word and byte publication preserve untouched parent bits. |
| Arithmetic and predicates | ADD/ADC/SUB/SBB/CMP, INC/DEC/NEG, AND/OR/XOR/TEST/NOT in admitted register, immediate, memory-source and memory-destination forms at widths 8/16/32. Carry input and signed immediate extension are distinct from truncation to the operation width. |
| Multiply and divide | One-operand unsigned/signed MUL/IMUL and DIV/IDIV at 8/16/32; admitted two-/three-operand IMUL16/32; CBW/CWDE/CWD/CDQ. Divide-by-zero and quotient overflow exit before destination publication. |
| Shift and rotate | SHL/SHR/SAR, ROL/ROR, RCL/RCR with one, immediate, or CL counts at admitted 8/16/32 forms; SHLD/SHRD16/32. Count masking and zero-count preservation are operation-specific. |
| Bits and conditions | BT/BTS/BTR/BTC16/32 with immediate/register indices and memory bit-string addressing; BSF/BSR16/32; full TZCNT32 register/memory; all sixteen SETcc, CMOVcc16/32, and Jcc conditions. No LZCNT or POPCNT capability is advertised. |
| Near control | Direct and admitted indirect JMP/CALL, RET with optional stack adjustment, operand16 near forms, LOOP/LOOPE/LOOPNE and JCXZ/JECXZ. Far transfer, interrupt, privilege transition, and 64-bit control forms are outside this profile. |
| Stack | PUSH/POP16/32 register/immediate/memory, PUSHF16/32, PUSHA/PUSHAD/POPA/POPAD, ENTER16/32 with nesting `imm8 & 31`, LEAVE16/32. Stack addressing is always full ESP under SS.B32, independently of operand or address size. |
| Flag and decimal controls | CLC/STC/CMC, CLD/STD, LAHF/SAHF, AAA/AAS/AAD/AAM/DAA/DAS. Only the user arithmetic/DF flag subset is stored; reserved bit 1 is always set. POPF/POPFD are outside this profile. |
| Exchange and atomic | XADD and CMPXCHG8/16/32 register/unlocked-memory forms and admitted explicit LOCK memory forms; implicit-locked memory XCHG8/16/32 and explicit LOCK memory XCHG8/16/32; WORD LOCK CMPXCHG/XADD and XCHG accept both single `66 F0` and `F0 66` orders. CMPXCHG8B with and without LOCK. LOCK memory ADD/ADC/SUB/SBB/AND/OR/XOR/INC/DEC/NEG/NOT8/16/32 and BTS/BTR/BTC16/32 are supported through family-specific or generic typed transactions. |
| Software identification | CPUID leaf 0 returns vendor `Ring3Engine ` and maximum leaf 1. Leaf 1 advertises FPU, TSC, CX8, CMOV, MMX, FXSR, SSE, and SSE2; other leaves return zero. RDTSC increments and reads a virtual process counter modulo 2^64. This is not a wall clock, host-cycle counter, or physical CPU timing promise. NOP/alignment-NOP and PAUSE have no scheduling effect. |

The stored integer flags are CF/PF/AF/ZF/SF/DF/OF plus fixed bit 1 (`R3ST` accepts mask `0xCD7`). Defined arithmetic flags follow the selected operand width. Architecturally undefined flags have deterministic per-operation policies in the Wasm emitter rather than host leftovers: logical results clear CF/OF/AF and derive PF/ZF/SF; legacy bit mutation retains ZF/DF/fixed bit and publishes the old selected bit to CF, while generic atomic bit forms retain all other flags. Those non-CF flags are architecturally undefined; these are explicit route-specific deterministic policies. BSF/BSR zero inputs retain their destination. TZCNT32 always returns 32 for zero input, sets CF for zero input and ZF for zero result, and clears other arithmetic flags while preserving DF/fixed bit. TZCNT is an admitted software instruction even though CPUID does not advertise a BMI1 bundle.

### Checked transactions

Guest RAM is private and unshared. A canonical instruction helper performs one exclusive host borrow. LOCK and implicit XCHG use checked single-instruction transactions: all source bytes are read before all destination bytes are preflighted, then one checked write version is committed and bytes copied. CMPXCHG mismatch still performs the checked identity write. Zero-source addition, an already-set/clear bit, AND with all ones, OR/XOR with zero, and writing identical bytes are real writes for permission, version, and executable-currentness purposes. Restoring bytes later does not restore an earlier code snapshot's version.

Read faults precede write faults when both are possible. Late preflight failure and version exhaustion leave the instruction's CPU/RAM publication neutral. Selected-byte MASKMOV and REP element progress have their explicitly different boundaries below. This model provides atomic guest instruction effects within one synchronous context; it does not establish inter-thread x86 memory ordering, a shared-memory bus lock, host concurrency, or hardware partial-fault behavior.

Legacy scalar helpers retain their strict width-specific packets. New whole-span/state helpers use validated 40-byte R3MH v7 results for memory faults, infrastructure failures, GP, x87/SIMD exceptions, and segment exceptions. Generated code validates the packet before destination/FLAGS publication. Successful writes that invalidate captured code retire the committed instruction and stop that unit before the next instruction.

## Strings, segments, and stack state

Plain and F2/F3 repeated MOVS/STOS/LODS/CMPS/SCAS support widths 8/16/32, address size 16/32, and DF direction. Source is DS or an explicit override; destination is ES. MOVS/STOS/LODS treat either repeat prefix as unconditional repetition; CMPS/SCAS select equal/not-equal continuation. REP uses CX or ECX according to address size. Zero-count REP performs no operand/segment access. Successful elements publish index/count/RAM effects; a failing element publishes none. Partial fault/cancel/budget retains instruction EIP and retires zero; complete repetition retires one. Work accounting bounds long repetitions independently of instruction retirement. An executable write stops a stale unit before another element.

`R3SG` holds ES/CS/SS/DS/FS/GS selectors and cached bases. CPL3 configured descriptors validate selector table, type, privilege, and presence; GP/NP/SS remain distinct. MOV to/from segments, segment PUSH/POP, and LDS/LES/LSS/LFS/LGS16/32 are admitted. CS is readable/pushable but not loadable by MOV/POP. CS/SS are nonnull RPL3; ES/DS/FS/GS may be null. Access through a null data segment raises GP0 before RAM; loading a null data selector itself is legal. The first four bases are flat zero; FS/GS may use configured nonzero bases. LEA returns the offset without adding a segment base. PUSH32 of a selector zero-extends it as an explicit software choice.

The stack profile is flat SS.B32. ENTER stages ordered frame reads and writes, with reads observing preceding staged pushes, and probes final allocated ESP without initializing locals. PUSHA uses original SP/ESP for its saved slot; POPA skips that saved slot. The multi-access stack helper checks the complete transaction before committing CPU/RAM. This intentionally does not reproduce hardware partial stack writes or 16-bit SS.B=0 exception ordering.

There is no arbitrary guest GDT/LDT interpreter, descriptor limit/granularity virtualization, asynchronous interrupt-shadow behavior, or hardware TLS startup model. Configured descriptors are a process interface rather than privileged guest table management.

## x87 forms and numerical policy

The x87 decoder currently selects 155 `iced_x86::Code` forms. That source census is neither a unique mnemonic count nor an exhaustive runtime-input count. Physical storage is eight raw 10-byte extended80 slots with TOP, full tags, control/status words, and stack-fault behavior. MMX aliases this storage; conversion through host f64 is not used.

| Group | Mnemonics/forms |
|---|---|
| Data | FLD/FST/FSTP IEEE32/64; FLD/FSTP extended80; FILD16/32/64; FIST16/32 and FISTP/FISTTP16/32/64; FBLD/FBSTP packed 18-digit BCD; ST(i) load/store/exchange (FXCH). |
| Arithmetic | FADD/FMUL/FSUB/FSUBR/FDIV/FDIVR register/pop, floating-memory32/64, and integer-memory16/32 variants; FSQRT, FRNDINT, FSCALE, FXTRACT, FPREM/FPREM1. |
| Compare | FCOM/FCOMP/FCOMPP, FUCOM/FUCOMP/FUCOMPP, FCOMI/FUCOMI/FCOMIP/FUCOMIP, FTST, FXAM, and all eight FCMOV conditions; admitted float/int memory compare forms. |
| Stack and constants | FCHS/FABS; FLD1/FLDL2T/FLDL2E/FLDPI/FLDLG2/FLDLN2/FLDZ; FFREE/FFREEP; FINCSTP/FDECSTP. |
| Environment | FINIT/FNINIT, FCLEX/FNCLEX, FWAIT/FNOP; FLDCW/FSTCW/FNSTCW/FSTSW/FNSTSW including AX; FLDENV/FSTENV/FNSTENV14/28; FRSTOR/FSAVE/FNSAVE94/108. |
| Transcendental | F2XM1, FYL2X, FYL2XP1, FPATAN, FPTAN, FSINCOS, FSIN, FCOS. |

Finite arithmetic, conversions, remainder, and sqrt use integer significands with guard/sticky rounding. PC24/53/64 and all four RC modes are supported. Signed zero, infinities, quiet/signaling NaNs, unsupported extended encodings, denormal operands, invalid/zero-divide/overflow/underflow/precision and stack faults are explicit classes. Tininess is determined with the same selected precision in an unbounded exponent range; a tiny inexact value that rounds to the minimum normal still raises the required underflow/precision state. BCD preserves the sign of zero.

Newly unmasked x87 exceptions latch status and retire with instruction-specific suppression or biased register results. A subsequent waiting x87 instruction, FWAIT, or MMX instruction delivers pending #MF before memory access. Nonwaiting environment operations retain their specified behavior. Successful pointer-bearing operations capture the current code selector and selected segmented data selector/offset; a memory fault does not advance those pointers.

Transcendentals use deterministic arbitrary-precision directed mathematical bounds rounded to extended80. The implementation increases precision to resolve the selected result; an unresolved bound is a numerical infrastructure failure, not a host-float fallback. This policy does not reproduce an Intel implementation's internal 68-bit pi or promise bit-identical physical transcendental results. F2XM1 outside [-1,1] retains its input; out-of-range trigonometric magnitude at least 2^63 sets C2 and retains the input. Obsolete FENI/FNENI, FDISI/FNDISI, and FSETPM/FNSETPM are excluded.

## MMX, SSE, and SSE2 forms

IA-32 legacy encodings use MM0–7, XMM0–7 and 32-bit GPRs. The following is an explicit mnemonic-label inventory from the SIMD decoder. Multiple labels share semantics and one label may have several register/memory encodings; the 181-label census must not be presented as 181 tested encodings.

### Move / scalar / high-low

MOVNTPS, MOVNTPD, MOVNTDQ, MOVNTQ, MOVNTI, MOVAPS, MOVAPD, MOVDQA, MOVUPS, MOVUPD, MOVDQU, MOVQ, MOVDQ2Q, MOVQ2DQ, MOVD, MOVSS, MOVSD, MOVLPS, MOVLPD, MOVHPS, MOVHPD.

### Packed integer and bitwise

PADDB, PADDW, PADDD, PADDQ, PSUBB, PSUBW, PSUBD, PSUBQ, PADDSB, PADDSW, PADDUSB, PADDUSW, PSUBSB, PSUBSW, PSUBUSB, PSUBUSW, PAND, ANDPS, ANDPD, PANDN, ANDNPS, ANDNPD, POR, ORPS, ORPD, PXOR, XORPS, XORPD, PCMPEQB, PCMPEQW, PCMPEQD, PCMPGTB, PCMPGTW, PCMPGTD, PMULLW, PMULHW, PMULHUW, PMULUDQ, PMADDWD, PAVGB, PAVGW, PMINUB, PMAXUB, PMINSW, PMAXSW, PSADBW, PSLLW, PSLLD, PSLLQ, PSRLW, PSRLD, PSRLQ, PSRAW, PSRAD.

### Shuffle / pack / extraction

MOVLHPS, MOVHLPS, PSLLDQ, PSRLDQ, PSHUFD, PSHUFW, PSHUFHW, PSHUFLW, SHUFPS, SHUFPD, PUNPCKLBW, PUNPCKLWD, PUNPCKLDQ, UNPCKLPS, PUNPCKLQDQ, UNPCKLPD, PUNPCKHBW, PUNPCKHWD, PUNPCKHDQ, UNPCKHPS, PUNPCKHQDQ, UNPCKHPD, PACKSSWB, PACKSSDW, PACKUSWB, PINSRW, PEXTRW, PMOVMSKB, MOVMSKPS, MOVMSKPD.

### Floating arithmetic and comparison

ADDPS, ADDSS, ADDPD, ADDSD, SUBPS, SUBSS, SUBPD, SUBSD, MULPS, MULSS, MULPD, MULSD, DIVPS, DIVSS, DIVPD, DIVSD, SQRTPS, SQRTSS, SQRTPD, SQRTSD, MINPS, MINSS, MINPD, MINSD, MAXPS, MAXSS, MAXPD, MAXSD, CMPPS, CMPSS, CMPPD, CMPSD, COMISS, UCOMISS, COMISD, UCOMISD, RCPPS, RCPSS, RSQRTPS, RSQRTSS.

### Conversion

CVTDQ2PS, CVTDQ2PD, CVTPI2PD, CVTPI2PS, CVTPS2DQ, CVTPS2PI, CVTTPS2DQ, CVTTPS2PI, CVTPD2DQ, CVTPD2PI, CVTTPD2DQ, CVTTPD2PI, CVTPS2PD, CVTPD2PS, CVTSI2SS, CVTSI2SD, CVTSS2SI, CVTTSS2SI, CVTSD2SI, CVTTSD2SI, CVTSS2SD, CVTSD2SS.

### State, masked stores, hints and fences

MASKMOVQ, MASKMOVDQU, SFENCE, LFENCE, MFENCE, PREFETCHNTA, PREFETCHT0, PREFETCHT1, PREFETCHT2, FXSAVE, FXRSTOR, EMMS, LDMXCSR, STMXCSR.

Packed integer forms include wrapping and signed/unsigned saturating arithmetic, width-specific compares/multiply/shifts, shuffles/unpacks/packs, extraction/insertion, and movemask. Legal MMX/XMM widths and operand directions are validated in decoded descriptors. Direct Wasm SIMD implements the selected move/integer/bit/shuffle hot paths; other typed descriptors use a pure execution helper. That helper executes decoded IR, not a runtime x86 interpreter.

Scalar/packed single and double arithmetic/conversion use exact IEEE32/64 APFloat operations and integer sqrt; rounding, signed zero, NaN priority/payload, invalid integer conversion, and lane exception aggregation are explicit. No silent host-f64 or relaxed-SIMD substitute is used. MXCSR rounding/masks/sticky bits, DAZ, and FTZ are modeled. Lane-local pre-/post-computation exception priority is applied before instruction aggregation; unmasked exceptions suppress destination publication. Tiny exact results under an unmasked underflow condition and tiny inexact results rounding to the minimum normal have dedicated literal tests. MIN/MAX preserve their instruction-specific source selection and signaling behavior rather than using a host min/max operation.

RCPPS/RCPSS/RSQRTPS/RSQRTSS are the declared approximation exception: a deterministic RN approximation within the architectural error allowance, with prescribed denormal/flush behavior and ignored RC. Their results are not a physical CPU lookup-table identity promise.

MOVNTQ does not impose a mandatory 8-byte GP alignment; aligned 128-bit MOVAPS/MOVAPD/MOVDQA and non-temporal MOVNTPS/MOVNTPD/MOVNTDQ retain their required alignment. MASKMOVQ/MASKMOVDQU preflight only bytes selected by each mask byte's sign bit and commit selected bytes as one write. Mask zero performs no operand or segment access and no write/version transition. Selected overflow/permission faults cannot partially store. MMX writes mark the written physical x87 register's exponent/sign field as all ones, preserving other slots; EMMS empties tags. Pending x87 #MF is checked before MMX memory access.

FXSAVE/FXRSTOR use the IA-32 512-byte image, required alignment, raw80/MMX/XMM/control state, and abridged-tag reconstruction from register values. FXSAVE preserves reserved/tail bytes outside the written architectural prefix. MXCSR reserved bits are rejected. Fences are ordering no-ops in this synchronous private unshared context; prefetch hints perform no guest memory access. CLFLUSH and its separate CPUID.CLFSH capability are deliberately absent. SSE3+, VEX/EVEX/AVX, XMM8+, YMM/ZMM and 64-bit GPR forms are outside this profile.

## State, faults, and process ownership

The arena contains strict versioned integer (`R3ST`), exit (`R3EX`), x87 (`R3FP`), SIMD (`R3SV`), segment (`R3SG`), helper, transfer, and repetition-work records. Reserved bits, sizes, headers, and incompatible state are validated. Context begin/finish/abort saves and restores extended x87/SIMD/segment state together with the integer context; pending calls reject unauthorized mutation. An invalid record or stale/foreign owner must not become a partial CPU update.

Memory permissions are checked by guest address space, including execute-only instruction fetch and full-span source/write preflight. Alignment GP precedes page access where required. Divide error, GP, NP, SS, pending x87 #MF, SIMD exceptions, memory faults, infrastructure refusal and stale-code exits are distinct software results. There is no privileged CR0/CR4 enable, TS/EM/lazy-FPU/task-switch model, IRQ13 delivery, paging hardware, #AC control, interrupt delivery, device I/O, or cache coherence model. CPUID flags describe this software execution profile, not those omitted controls.

## Architecture owners

| Boundary | Source modules |
|---|---|
| Fetch, profile and lowering | `decode/decoder.rs`, `profile.rs`, `operands.rs`, `lower.rs`, `integer.rs`, `flow.rs`, and family `x87.rs`, `simd.rs`, `atomic.rs`, `strings.rs`, `stack.rs`, `segments.rs` |
| Typed operations/state | `ir.rs`, `state.rs`, `atomic.rs`, `stack.rs`, `segments.rs`, `simd.rs` plus `simd/{packed,float}.rs`, `x87.rs` plus `x87/{value,execute,transcendental}.rs` |
| Compilation and currentness | `cpu/dbt/{region,artifact,cold,resident}.rs` and `cpu/dbt/wasm/{emitter,dispatcher,control}.rs` |
| Generated semantics | `cpu/dbt/wasm/integer.rs`, `integer_system.rs`, family `atomic.rs`, `strings.rs`, `stack.rs`, `segments.rs`, `x87.rs`, `simd.rs` and `simd/direct.rs`; checked `memory/*` and strict `cpu_result.rs` |
| ABI and host transactions | `abi/x86/{state,exit,x87,simd,segments}.rs`, `abi/cpu_helper.rs`, `abi/arena.rs`; `memory/space.rs`; `process/{cpu_memory,cpu_integer,x87,simd,stack,segments,call}.rs` and bound export wrappers |

Paths in this table are relative to `engine/src`; paths in the first two rows begin at `cpu/x86`. Shared integration files are the interface between these owners, not alternate implementations of the family semantics.

## Evidence and coverage limits

Independent literal expectations and actual generated-Wasm paths are separate proof surfaces. Numerical tests must use raw values, primary rules, or independently derived algebra rather than calling the implementation to generate its own expected result. Native decode/compile tests establish typed shape, consumed length, execute fetch boundaries, limits and publication neutrality; actual-Wasm tests establish execution, helper protocol, retirement, flags, fault/retry, and currentness on named fixtures.

Representative permanent evidence includes:

- `tests/cpu_integer_completion.rs` and `cpu_integer_completion_wasm.rs`: atomic widths/source aliases, CMPXCHG8B, software CPUID/RDTSC and segmented/address16 forms; older dedicated arithmetic/shift/divide/bit/near-control tests retain their distinct boundaries.
- `tests/cpu_prefixed_moffs_near_control.rs` and the `prefix-boundaries` mode of `fixtures/p2-integer-completion/run.mjs`: WORD address16 moffs, FS/GS offsets, ignored near-control prefixes, return slots and observed-context continuation.
- `tests/cpu_x87_helper_protocol.rs`, `cpu_profile_boundaries_wasm.rs` and their fixtures: malformed helper FLAGS, strict new state records, work-record alias refusal, and BCD signed-zero publication.
- `tests/x87_numeric_boundaries.rs`, `x87_arena.rs`, `cpu_x87_extended_wasm.rs` and `fixtures/p2-x87-extended/run.mjs`: raw80 values, PC/RC, exception/tininess, environment/selector pointers, BCD, and selected transcendental literal cases.
- `tests/simd_semantics.rs`, `cpu_simd_wasm.rs` and `fixtures/p2-simd/run.mjs`: packed/scalar values, lane exceptions, NaN/rounding/tininess, alignment, selected MASKMOV, shared MMX/x87 state and state save/restore.
- `tests/cpu_rep_strings.rs`, `cpu_rep_segments_wasm.rs`, `cpu_segments.rs`, `cpu_segments_wasm.rs`, `cpu_stack_frames.rs` and `cpu_stack_frames_wasm.rs`: repetition progress, segment/null/fault rules, and transactional stack frames.
- `tests/cpu_extended_context_wasm.rs` and `fixtures/p2-extended-context/run.mjs`: dirty extended-state finish/abort restoration and pending-call refusal neutrality.
- Retained original compiler consumers under `fixtures/p2-{locked-fullmask32,locked-dynamic-flags32,returned-atomic-flags32,bitmap-dispatch32}`: original object/text/provenance, natural calls, guest-consumed results and bounded whole-consumer paths. Source-specific checksums/chronology remain narrower than arbitrary application support.

`fixtures/p2-simd/benchmark.mjs` measures one explicit packed-integer register chain, including the generated run/guard calls. Cold and warmed measurements, helper crossings, final algebra and generated module hashes are recorded separately; that chain is not a whole-CPU throughput estimate.

The root-owned execution logs and frozen artifacts determine which current tests passed. A source inventory or unexecuted test is not a passing result. There is no CPU completion percentage, exhaustive encoded-form/input proof, hardware-transcendental identity, whole-Windows compatibility, game-support, or cross-context concurrency claim implied by this document.
