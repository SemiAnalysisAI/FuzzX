# GVN computeLoadStoreVN (MSSA mode) ignores volatile/atomic ordering

**File:** `llvm/lib/Transforms/Scalar/GVN.cpp:620-636` (`GVNPass::ValueTable::computeLoadStoreVN`)

## Reasoning

When MSSA is enabled (cl::opt `-enable-gvn-memoryssa`, or pass option
`<memoryssa>`), `computeLoadStoreVN` builds the expression for a load/store
using only `{Type, Opcode, operand VNs, clobbering MemoryAccess VN}`. It does
NOT include `isVolatile()`, `isAtomic()`, `getOrdering()`, or
`getSyncScopeID()`. Two loads at the same address with the same clobbering
memory state but different ordering or volatility are therefore assigned the
same value number, and the second can be replaced by the first.

When the SECOND load is non-atomic/non-volatile and the FIRST load is atomic,
`processLoad` (`isUnordered()` check at line 2163 passes) finds the first load
as leader and replaces the second with it. That direction is benign
(non-atomic replaced by a value that came from a stronger atomic).

The REVERSE direction is also blocked: the volatile/atomic load fails
`isUnordered()` at line 2163, so `processLoad` returns false for it. So the
defect doesn't trigger a true miscompile under the default `processLoad`
flow.

However, the missing ordering/volatility/sync-scope in the VN key still
violates the invariant that "equal VNs implies equivalent semantics" and is
a latent footgun: any future code path (PRE, scalar PRE leader lookup,
sinking, etc.) that uses VN equivalence without re-checking `isUnordered()`
on the candidate replacement will silently miscompile.

## IR repro (illustrates the conflation, currently not a miscompile by chance)

```
define i32 @f(ptr %p) {
entry:
  %a = load atomic i32, ptr %p monotonic, align 4
  %b = load i32, ptr %p, align 4
  %r = add i32 %a, %b
  ret i32 %r
}
```

Run: `opt -enable-gvn-memoryssa -passes=gvn -S`

Observed (the non-atomic load `%b` is eliminated, replaced by `%a`):
```
  %a = load atomic i32, ptr %p monotonic, align 4
  %r = add i32 %a, %a
```

This particular replacement is semantically safe in isolation, but
demonstrates the conflation. A hardened fix should include
`{isVolatile, getOrdering, getSyncScopeID}` in the Expression so the VNs
diverge.
