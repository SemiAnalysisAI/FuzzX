# m121: `AMDGPULowerKernelAttributes` UDiv->block_count upgrade silently drops volatile/atomic load value

*Discovery method: code inspection.*  Sibling shape to m108 (same
file: `hidden_grid_dims` folded from kernel-static metadata).

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/AMDGPULowerKernelAttributes.cpp:421-444`:

The V5+ "upgrade old block-size calc" rewrite (guarded by
`if (IsV5OrAbove)`, matching legacy V4-style `dispatch_ptr` loads
still present in V5+ IR) pattern-matches:

```cpp
m_UDiv(m_ZExtOrSelf(m_Load(m_GEP(amdgcn_dispatch_ptr, GRID_SIZE_X + I*4))),
       m_Value())
```

`m_Load(...)` accepts **any** `LoadInst`, including `volatile` and
`atomic` ones.  The rewrite then `replaceAllUsesWith`'s the UDiv with
a freshly built non-volatile, non-atomic load of `HIDDEN_BLOCK_COUNT_X`
from the implicitarg pointer (a **different** memory location).

The original `volatile`/`atomic` load survives as a dead
side-effecting op (so DCE keeps it), but its **value** is silently
dropped from the computation.

Contrast line 205 (`if (!Load || !Load->isSimple()) continue;`) which
correctly filters all *other* candidate loads in the same pass.  Only
the UDiv-pattern load is left unfiltered.

## Reproducer

`reduced.ll`:

```llvm
define amdgpu_kernel void @k(ptr addrspace(1) %out) {
  %dp  = call ptr addrspace(4) @llvm.amdgcn.dispatch.ptr()
  %gxp = getelementptr i8, ptr addrspace(4) %dp, i64 12
  %gx  = load volatile i32, ptr addrspace(4) %gxp, align 4   ; volatile!

  %gsp = getelementptr i8, ptr addrspace(4) %dp, i64 4
  %gs  = load i16, ptr addrspace(4) %gsp, align 2
  %gs32 = zext i16 %gs to i32

  %bc = udiv i32 %gx, %gs32              ; UDiv-pattern matches
  store i32 %bc, ptr addrspace(1) %out
  ret void
}
```

`opt -mtriple=amdgcn-amd-amdhsa -mcpu=gfx950 --amdhsa-code-object-version=5 -passes=amdgpu-lower-kernel-attributes -S reduced.ll`:

```llvm
  %gx  = load volatile i32, ptr addrspace(4) %gxp, align 4     ; preserved but dead
  %gs32 = zext i16 %gs to i32                                   ; preserved but dead
  %0 = getelementptr inbounds i8, ptr addrspace(4) %iap, i64 0  ; NEW
  %1 = load i32, ptr addrspace(4) %0, !invariant.load, !noundef ; NEW (non-volatile)
  store i32 %1, ptr addrspace(1) %out                           ; stores block_count
                                                                ; NOT udiv(volatile, gs)
```

`llc` then emits asm that reads from `implicitarg.ptr + 0xc`
(block_count) and ignores the volatile load value entirely.

Atomic variant (acquire load on `dispatch_ptr.grid_size_x`) exhibits
the same behavior -- the pass treats both `volatile` and atomic as
plain loads.

## Suggested fix

After the `match(...)` succeeds, retrieve the matched `LoadInst` (via
a captured `m_Load(m_GEP(..., LoadCapture))` binding or by walking
the UDiv operand) and `continue` if `!Load->isSimple()`.  Or, more
conservatively, wrap with `m_OneUse` so a side-effect-carrying load
with multiple consumers is rejected.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces. |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/opt`) | Same fold present. |

## Why the fuzzer hasn't caught it

* The IR fuzzer rarely emits `load volatile` or `load atomic` on
  `dispatch_ptr.grid_size_x` (uncommon shape).
* The interpreter oracle treats `volatile` semantics opaquely.
* Per `MEMORY.md` (Prefer-random-over-idioms), the right hook is to
  emit `load volatile i32 from implicitarg.ptr + {0,4,8,12,16,20}`
  followed by `udiv` so this pattern is reachable.
