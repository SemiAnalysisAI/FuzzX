# 259 — X86 kcfi-arity derived from MIR live-ins undercounts / mis-encodes register arity

Component: `llvm/lib/Target/X86/X86AsmPrinter.cpp` `emitKCFITypeId` (kcfi-arity
emission, ~lines 201-231).

With `-fsanitize-kcfi-arity` (module flag `"kcfi-arity"`), the `__cfi_` prefix
encodes the number of register-passed arguments so a FineIBT-enforcing kernel
can poison the live argument registers. The documented meaning (comment ~206-213,
commit e223485c) is the **ABI/type-level** count. But the code computes it from
`MF.getRegInfo().liveins()` (~220-228) — the set of *used* physical arg
registers. At `-O2` the optimizer legitimately drops unused arg registers from
liveins, so the emitted arity **undercounts**, and because the count→register
encoding assumes a contiguous RDI-first prefix, a sparse live set can poison the
wrong registers.

## Result (HEAD 023e7decf625)
`void @handler(i32 event /*unused*/, ptr ctx /*used*/)` — ABI arity 2:
```
movl  $199571451, %ecx     # arity 1, not 2
movl  (%rsi), %eax         # ctx is in RSI (2nd reg), but arity encoding assumes RDI-first
```
Same `void(i32,i32)` signature emits `%eax` (arity 0) when both args are unused
vs `%edx` (arity 2) when both are used — the emitted security metadata depends on
optimizer liveness, contradicting its own documented ABI-arity contract.
`-stop-after=kcfi` confirms unused args → `liveins: []`.

## Severity
Security-metadata correctness bug on ordinary code (unused/sparse params) at the
kernel's -O2 build mode; degrades FineIBT's intended live-arg-register poisoning.
Medium: `-fsanitize-kcfi-arity` is experimental/non-default and the FineIBT
consumer is a not-yet-upstream kernel patch. Reproduces at HEAD; no fix landed.
