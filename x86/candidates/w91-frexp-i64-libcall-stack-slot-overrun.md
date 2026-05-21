# w91 — `llvm.frexp.f64.i64` allocates 8-byte stack slot but libcall writes 4; high bits leak caller `%rax`

Component: SelectionDAG/TargetLowering `expandMultipleResultFPLibCall` (used by LegalizeDAG case ISD::FFREXP / FMODF)

Severity: silent miscompile + uninitialized-memory read (info leak)

## Source

`llvm/lib/CodeGen/SelectionDAG/TargetLowering.cpp:13245` —
`TargetLowering::expandMultipleResultFPLibCall`. The output pointer slot for
each non-`CallRetResNo` result is materialised with:

```cpp
for (auto [ResNo, ST] : llvm::enumerate(ResultStores)) {
  if (ResNo == CallRetResNo)
    continue;
  EVT ResVT = Node->getValueType(ResNo);                            // <-- IR type
  SDValue ResultPtr = ST ? ST->getBasePtr() : DAG.CreateStackTemporary(ResVT);
  ResultPtrs[ResNo] = ResultPtr;
  Args.emplace_back(ResultPtr, PointerTy);
}
```

and after the libcall returns:

```cpp
SDValue LoadResult = DAG.getLoad(Node->getValueType(ResNo), DL, CallChain,
                                 ResultPtr, PtrInfo);
```

The stack slot **size and the load width are both `Node->getValueType(ResNo)`**.
For `llvm.frexp.f64.i64` (an LLVM-IR-legal intrinsic — LangRef permits any
integer width for the exponent return), `ResVT = i64`, so we allocate 8 bytes
and read 8 bytes back. But the libcall `frexp(double, int*)` is a glibc/POSIX
signature with `int`-sized exponent — on x86-64 SysV that is 4 bytes. The
upper 4 bytes of the stack slot are never written by the callee.

Unlike `SoftenFloatRes_FFREXP` (LegalizeFloatTypes.cpp:783) and
`FPOWI`/`STRICT_FPOWI` (LegalizeDAG.cpp:5074-5084), which both perform:

```cpp
if (DAG.getLibInfo().getIntSize() != ExpVT.getSizeInBits()) {
  DAG.getContext()->emitError("... does not match sizeof(int)");
  return POISON;
}
```

`expandMultipleResultFPLibCall` has **no sizeof(int) guard** and silently
emits the libcall with a mismatched stack slot.

## x86-64 runtime evidence

```ll
target triple = "x86_64-unknown-linux-gnu"
declare {double, i64} @llvm.frexp.f64.i64(double)
define i64 @frexp_i64(double %x) {
  %r = call {double, i64} @llvm.frexp.f64.i64(double %x)
  %e = extractvalue {double, i64} %r, 1
  ret i64 %e
}
```

`llc -mtriple=x86_64-unknown-linux-gnu` emits:

```asm
frexp_i64:
  pushq  %rax                  ; reserve 8 bytes (also stack-realigns); RAX is caller-saved garbage
  movq   %rsp,    %rdi         ; int* arg points at the 8-byte slot
  callq  frexp@PLT             ; writes only the LOW 4 bytes (an `int`)
  movq   (%rsp),  %rax         ; reads the FULL 8 bytes back
  popq   %rcx
  retq
```

- The `pushq %rax` saves whatever the caller had in `%rax` into the stack
  slot.
- `frexp` writes only `[%rsp]..[%rsp+3]` (the int exponent).
- `[%rsp+4]..[%rsp+7]` still contain whatever the caller had in the high
  half of `%rax` — uninitialized to the IR semantics, but in practice
  attacker-controllable or at least info-leaking. (E.g., in a security
  context, this leaks half of any 64-bit value that was in the caller's
  `%rax`, typically the previous return value.)
- `movq (%rsp), %rax` returns those leaked bytes as the high 32 bits of
  the `i64` exponent.

A trivial C++ harness around this routine confirms different runs yield
different upper-32 results depending on what was in `%rax` (e.g., the
result of the previous syscall, the dispatch routine's saved value, etc.).

## Affected intrinsics / paths

Same pattern fires for:
- `llvm.frexp.<fty>.iN` for any `N != sizeof(int) * 8` (LangRef-legal).
- `llvm.modf.<fty>.iN` is structurally similar via FMODF (TLI helper shared).
- Soft-vector lowerings of these.

`expandMultipleResultFPLibCall` is the choke point because both `ISD::FFREXP`
case (LegalizeDAG.cpp:5037) and `ISD::FMODF` (LegalizeDAG.cpp:5036) go
through it without first validating the int width.

## Why this is distinct from #011

- #011 (and the strict-FLDEXP variant) is about an i64 exponent passed
  *as a value* in `%rsi` and silently truncated to `%esi` by the call ABI.
- This bug is about an i64 exponent slot passed *by pointer* (output arg),
  where the libcall writes 4 bytes and the LLVM-emitted load reads 8.
  The pathology is on the *return* side, not the argument side, and it
  manifests as uninitialized-stack info leak rather than truncation.

## Fix sketch

```cpp
// Before allocating output slots:
unsigned IntBits = DAG.getLibInfo().getIntSize();
for (auto [ResNo, ST] : llvm::enumerate(ResultStores)) {
  if (ResNo == CallRetResNo) continue;
  EVT ResVT = Node->getValueType(ResNo);
  if (ResVT.isInteger() && ResVT.getSizeInBits() != IntBits) {
    // Libcall would write `int` bytes into a slot of different size.
    DAG.getContext()->emitError(
        "FFREXP/FMODF exponent type does not match sizeof(int) in libcall");
    return false;
  }
  ...
}
```

Alternatively, route i64 exponents through `expandFrexp` (the local
SDAG-builder expansion) rather than the libcall whenever the int-width
predicate fails.
