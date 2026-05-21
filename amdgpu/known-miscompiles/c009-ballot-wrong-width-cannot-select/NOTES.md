# c009: `llvm.amdgcn.ballot.<N>` with `<N> != WavefrontSize` ICEs in ISel for non-constant arguments

*Discovery method: code inspection (during `amdgcn.ballot/icmp/fcmp`
wave-size audit).*  Distinct from c007 (constant-fold ICE).

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/SIISelLowering.cpp:7811-7852`
(`SITargetLowering::lowerBALLOTIntrinsic`):

The lowering emits `AMDGPUISD::SETCC` *directly in the user-requested
return type* (lines 7826 and 7849-7851) without first emitting the
wave-sized SETCC and `getZExtOrTrunc`'ing to the user VT.  The
sibling `lowerICMPIntrinsic` / `lowerFCMPIntrinsic` do this
correctly (line 7779 for icmp; 7808 for fcmp).

ISel has no pattern matching a wave-mask SETCC at the wrong width,
so it aborts:

```
LLVM ERROR: Cannot select: i32 = AMDGPUISD::SETCC ..., setne:ch
```

Reproduces on:
* `ballot.i32` / wave64 (gfx950)
* `ballot.i64` / wave32 (gfx1030 `+wavefrontsize32`)

Only the constant-1 `isOne()` fast-path at lines 7836-7841 bothers
to check the active wave width.  All other inputs (`%c =
icmp/fcmp/load i1`) reach the broken general-path code.

## Reproducer

`reduced.ll`:

```llvm
define amdgpu_kernel void @t(ptr addrspace(1) %p, i32 %x) {
  %c = icmp eq i32 %x, 0
  %r = call i32 @llvm.amdgcn.ballot.i32(i1 %c)   ; i32 on wave64 -> ICE
  store i32 %r, ptr addrspace(1) %p, align 4
  ret void
}
```

`llc -mtriple=amdgcn -mcpu=gfx950 -O2 reduced.ll`:

```
LLVM ERROR: Cannot select: i32 = AMDGPUISD::SETCC ..., setne:ch
```

## Suggested fix

In `lowerBALLOTIntrinsic` (SIISelLowering.cpp:7811-7852), mirror
the structure of `lowerICMPIntrinsic`:

```cpp
EVT WaveMaskVT = EVT::getIntegerVT(*DAG.getContext(),
                                   Subtarget->getWavefrontSize());
SDValue WaveMask = DAG.getNode(AMDGPUISD::SETCC, DL, WaveMaskVT, ...);
return DAG.getZExtOrTrunc(WaveMask, DL, VT);
```

Alternatively, the IR verifier could reject mismatched widths
(`int_amdgcn_ballot` return type width != WavefrontSize).

Sibling shape to:
* c003-permlane16-isel-ice -- intrinsic without target gate.
* c007-fcmp-i32-wave64-fold-ice -- the *constant-fold* version of
  this defect.  Both wave-size-mismatch routes ICE; c007 fires only
  when constant folding produces a single bit, c009 fires for the
  general non-constant path.
* c008-amdgcn-class-bf16-isel-ice -- different intrinsic, same
  family ("lowering ignores type-vs-target invariant").

## Why the fuzzer hasn't caught it

* The fuzzer's existing c007 suppression vetoes `amdgcn_fcmp_i32`
  on wave64 with equal constant operands.  Per `MEMORY.md`
  (Prefer-random-over-idioms), the suppression should be widened to
  veto *any* `amdgcn_ballot` / `amdgcn_icmp` / `amdgcn_fcmp` call
  whose return type width != WavefrontSize, regardless of operand
  constancy.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | ICEs at -O2 (non-constant arg). |
| ROCm 7.1.1 | Same defect. |
