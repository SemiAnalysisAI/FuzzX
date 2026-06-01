# 035 — computeKnownBitsForPRMT recurses with un-incremented Depth, defeating the MaxRecursionDepth guard (stack overflow / exponential compile-time blowup)

- **Kind:** crash (stack overflow / exponential)
- **Reachable via:** default llc
- **Component:** NVPTXISelLowering.cpp 7708-7709 (function computeKnownBitsForPRMT, 7698-7727)  (round-6 area `W24-knownbits-demanded2`)
- **Note:** "invalid PTX" entries are validated against the PTX ISA + strong in-tree corroboration (sibling guards / orderings); no local `ptxas` was available to execute the rejection.

## Summary

`computeKnownBitsForPRMT` recurses with un-incremented `Depth`, defeating the recursion-depth guard → stack overflow on a long prmt chain (and exponential compile time on a branching one)

## Mechanism / root cause

computeKnownBitsForPRMT computes known bits of an NVPTXISD::PRMT by recursing into its two i32 operands:

  KnownBits AKnown = DAG.computeKnownBits(A, Depth);   // line 7708
  KnownBits BKnown = DAG.computeKnownBits(B, Depth);   // line 7709

It passes the INCOMING Depth, not Depth + 1. SelectionDAG::computeKnownBits guards recursion solely via `if (Depth >= MaxRecursionDepth) return Known;` at the top (SelectionDAG.cpp:3369, MaxRecursionDepth=6). The generic dispatcher calls TLI->computeKnownBitsForTargetNode(Op, Known, DemandedElts, *this, Depth) with the same Depth (SelectionDAG.cpp:4536), and every other recursion site in computeKnownBits uses `Depth + 1`. Because the PRMT handler re-enters computeKnownBits at the SAME Depth, Depth never advances across a chain of PRMT nodes, so the depth guard never fires. Two consequences from one defect:
  (1) Linear PRMT chain (each prmt's op0 = previous prmt, op1 = constant): unbounded recursion depth -> stack overflow.
  (2) Branching chain (each prmt feeds BOTH operands of the next; computeKnownBits is uncached): 2^N work -> exponential compile-time hang.
The sibling demanded-bits path (simplifyDemandedBitsForPRMT, lines 7828/7830) correctly uses Depth + 1; only computeKnownBitsForPRMT is wrong. Fix: pass Depth + 1 on lines 7708-7709.

## Trigger

Any valid IR where a chain of @llvm.nvvm.prmt results feeds into each other and the final result reaches a known-bits query (e.g. masked by `and`, which triggers SimplifyDemandedBits in DAGCombine). Reachable with the bare default triple, no -mcpu needed. Linear chain of ~100000 prmts crashes; a branching chain (op0==op1==prev prmt) shows clean 2x-per-link exponential time growth (N=18:0.10s, 20:0.30s, 22:1.14s, 24:4.46s) confirming the missing depth increment / lack of memoization.

## Reproducer

```
target triple = "nvptx64-nvidia-cuda"
declare i32 @llvm.nvvm.prmt(i32, i32, i32)
define i32 @prmt_chain(i32 %x) {
  %v0 = call i32 @llvm.nvvm.prmt(i32 %x, i32 0, i32 17)
  %v1 = call i32 @llvm.nvvm.prmt(i32 %v0, i32 0, i32 17)
  ; ... repeat to ~100000 links (each %vK uses %v(K-1) as op0) ...
  %a = and i32 %vLAST, 255   ; forces a computeKnownBits/SimplifyDemandedBits query on the PRMT chain
  ret i32 %a
}
```

Command: `llc -mtriple=nvptx64 -o - repro.ll`

## Note

For a runnable repro, generate a long linear prmt chain (≈60k links) ending in `and i32 %last, 255`; `scratch/` has a generator. A short branching chain (each prmt feeding both operands of the next) shows the exponential blowup directly (n=20→0.27s, 22→1.07s, 24→4.24s).

## Verification

Reproduced with the built llc (emitted PTX / crash matches the claim; finder confidence 0.83, confirmed_with_llc=True).
