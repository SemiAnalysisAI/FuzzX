# m136: addrspace(7) seq_cst atomic loses scope bits on the buffer_atomic_* instruction itself (SC1 never set)

*Discovery method: code inspection + asm diff.*  Sibling shape to
m119 (target-side ordering / cache-control).  Distinct from m096
(same file's weak-cmpxchg poison) and from m107 (fat-ptr fence
mis-shape).

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/SIISelLowering.cpp:1473-1475`
(in `getTgtMemIntrinsic`, atomic-buffer branch):

```cpp
default:
  Info.flags = Flags | MachineMemOperand::MOLoad;
  if (!IsSPrefetch)
    Info.flags |= MachineMemOperand::MOStore;
  ...
  // XXX - Should this be volatile without known ordering?
  Info.flags |= MachineMemOperand::MOVolatile;       // <-- volatile only
  Info.memVT = MVT::getVT(CI.getArgOperand(0)->getType());
```

The MMO carries `MOVolatile` but **no atomic ordering** and **no
SSID**.  Combined with `AMDGPULowerBufferFatPointers.cpp:1656-1680`
which lowers `atomicrmw seq_cst ptr addrspace(7)` to
`fence release + raw.ptr.buffer.atomic.* + fence acquire`, the buffer
atomic MachineInstr arrives at `SIMemoryLegalizer` with
`getSuccessOrdering() == NotAtomic`.

In `SIMemoryLegalizer.cpp:827` (`constructFromMIWithMMO`), the
`OpOrdering != AtomicOrdering::NotAtomic` guard then skips SSID
merging.  At line 855, the outer `if (Ordering != AtomicOrdering::NotAtomic)`
skips `toSIAtomicScope`.  In `expandAtomicCmpxchgOrRmw` (line 2467),
`MOI.isAtomic()` returns false, so `enableRMWCacheBypass` (line 2479)
is **never called** on the buffer atomic.  The SC0/SC1 scope bits at
lines 1137-1154 (gfx940/950 RMW) are never set on the `BUFFER_ATOMIC_*`
MI.

The surrounding `buffer_wbl2 sc0 sc1` / `buffer_inv sc0 sc1` fences
are correctly placed by the fat-pointer fence rewrite, but they
implement only the cache-flush half of seq_cst.  Without `SC1` on the
atomic itself, two waves on distinct agents can each apply the RMW to
their own L2 line concurrently -- the fence pair guarantees prior
writes are visible and subsequent reads refetch, but does NOT
arbitrate the atomic at system scope.  Net: a seq_cst counter
increment on `addrspace(7)` can lose updates under cross-agent
contention.

## Reproducer

`reduced.ll`:

```llvm
define amdgpu_kernel void @rmw_sys(ptr addrspace(7) %p, i32 %v) {
  %r = atomicrmw add ptr addrspace(7) %p, i32 %v seq_cst, align 4
  ret void
}

define amdgpu_kernel void @rmw_sys_g(ptr addrspace(1) %p, i32 %v) {
  %r = atomicrmw add ptr addrspace(1) %p, i32 %v seq_cst, align 4
  ret void
}
```

`opt -passes=amdgpu-lower-buffer-fat-pointers | llc -mcpu=gfx950 -O2`:

```asm
rmw_sys:                              rmw_sys_g:
  ...                                   ...
  buffer_wbl2 sc0 sc1                   buffer_wbl2 sc0 sc1
  buffer_atomic_add ... offen           global_atomic_add ... sc1   <-- HAS sc1
  s_waitcnt vmcnt(0)                    s_waitcnt vmcnt(0)
  buffer_inv  sc0 sc1                   buffer_inv  sc0 sc1
```

`addrspace(7)` emits `buffer_atomic_add` with **no SC bits** (=
wavefront/default scope), while the equivalent `addrspace(1)` global
path correctly emits `sc1` (= system scope) on the atomic itself.
Same defect for `cmpxchg`, atomic `load`, atomic `store`.

Tellingly, `rmw_sys` and `rmw_agent` (same kernel but `syncscope("agent")`)
emit byte-identical `buffer_atomic_add ... 0 offen` -- the
system-vs-agent scope difference is lost on the atomic instruction.

Reproduced on ROCm 7.1.1 (`opt`+`llc` from `/opt/rocm-7.1.1/lib/llvm/bin/`)
-- not a HEAD-only regression.

## Suggested fix

Option 1 (preferred): in `AMDGPULowerBufferFatPointers` (the IR
lowering), attach the original `atomicrmw` ordering + SSID to the
synthesized `amdgcn.raw.ptr.buffer.atomic.*` call via metadata.  Then
extend `SIISelLowering::getTgtMemIntrinsic` (the atomic-buffer branch
at line 1473-1475) to propagate that into `Info.order` and
`Info.ssid` so the MMO carries real atomic-ordering info.

Option 2: in `SIMemoryLegalizer::expandAtomicCmpxchgOrRmw`, treat a
`BUFFER_ATOMIC_*` whose MMO has `MOVolatile` but no ordering, and
which is immediately preceded by an `ATOMIC_FENCE release|seq_cst`
and followed by an `ATOMIC_FENCE acquire|seq_cst` with the same SSID,
as carrying that SSID's scope, and call `enableRMWCacheBypass`
accordingly.  Fragile.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces. |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/{opt,llc}`) | Reproduces. |

## Why the fuzzer hasn't caught it

* The harness's oracle compares per-wave/per-lane output;
  cross-agent atomic contention requires multi-grid execution which
  the runner doesn't currently exercise.
* The asm-level divergence -- `buffer_atomic_add` without `sc1` vs
  `global_atomic_add` with `sc1` -- is observable at every
  `addrspace(7)` seq_cst RMW/cmpxchg/load/store site.  An asm-pattern
  oracle (compare `addrspace(7)` SC bits against equivalent
  `addrspace(1)` SC bits at matching seq_cst sites) would catch it
  on every fuzz seed.
