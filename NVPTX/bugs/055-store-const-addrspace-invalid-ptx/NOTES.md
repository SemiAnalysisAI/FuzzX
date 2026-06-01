# 055 — Scalar store to constant address space (AS 4) emits invalid st.const PTX

- **Kind:** other (invalid PTX)
- **Reachable via:** default llc
- **Component:** NVPTXISelDAGToDAG.cpp 1383-1435 (tryStore); cf. guard present at 1443-1446 in tryStoreVector  (round-7 area `A02-ldst-cvta-special-as`)
- **Note:** "invalid PTX" entries are validated against the PTX ISA + in-tree corroboration (sibling guards/orderings); no local `ptxas` was available to execute the rejection.

## Summary

a scalar `store` through a constant-space (AS 4) pointer emits `st.const`, which is not a valid PTX store

## Mechanism / root cause

NVPTXDAGToDAGISel::tryStore() computes `const auto CodeAddrSpace = getAddrSpace(ST);` and passes it straight to ST_i16/ST_i32/ST_i64 with no validation. For a `store` to `addrspace(4)` (Const), CodeAddrSpace == NVPTX::AddressSpace::Const, and the InstPrinter 'addsp' modifier (NVPTXInstPrinter.cpp:356,362) emits `.const`, producing `st.const.b32 [addr], val`. The PTX ISA `st` instruction supports only `.global/.local/.param/.shared` (and shared sub-spaces) state spaces; `.const` is read-only and has NO `st` form, so ptxas rejects `st.const`. The asymmetry is the smoking gun: the VECTOR store path tryStoreVector() at lines 1443-1446 explicitly does `if (CodeAddrSpace == NVPTX::AddressSpace::Const) report_fatal_error("Cannot store to pointer that points to constant memory space");` — but the SCALAR tryStore() omits that exact guard. So `store <2 x i32>` to const fatal-errors gracefully (fine) while `store i32` to const silently emits invalid PTX.

## Trigger

A plain (non-UB, well-defined) `store iN %v, ptr addrspace(4) %p` for a scalar integer/float type, any sm/PTX version.

## Reproducer

```
target triple = "nvptx64-nvidia-cuda"
define void @store_const(ptr addrspace(4) %p, i32 %v) {
  store i32 %v, ptr addrspace(4) %p
  ret void
}
```

Command: `llc -mtriple=nvptx64 -o - repro.ll`

## Verification

Reproduced with the built llc (crash/emitted-PTX matches the claim; finder confidence 0.9).
