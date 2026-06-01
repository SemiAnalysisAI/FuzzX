# 053 — atom.{exch,cas}.b128 with .sys scope emitted at PTX ISA 8.3 (.sys on .b128 requires PTX 8.4) — invalid PTX

- **Kind:** other (invalid PTX)
- **Reachable via:** llc -mcpu=sm_90 -mattr=+ptx83
- **Component:** NVPTXInstPrinter.cpp NVPTXSubtarget.h:106 (hasAtomSwap128); NVPTXISelLowering.cpp:1099-1100,1108 (i128 atom custom-lowered, MaxAtomicSize=128); NVPTXIntrinsics.td:2754-2779 (ATOM_CAS_B128/ATOM_EXCH_B128 asm strings); NVPTXInstPrinter.cpp:328-349 (printAtomicCode scope -> .sys)  (round-7 area `Q03-atom-red-qualifier`)
- **Note:** "invalid PTX" entries are validated against the PTX ISA + in-tree corroboration (sibling guards/orderings); no local `ptxas` was available to execute the rejection.

## Summary

default-scope `atomicrmw xchg`/`cmpxchg i128` emits `atom.sys.{exch,cas}.b128` at PTX 8.3, but `.sys` on `.b128` atom requires PTX 8.4

## Mechanism / root cause

hasAtomSwap128() = `SmVersion >= 90 && PTXVersion >= 83` (NVPTXSubtarget.h:106). When true, NVPTXISelLowering sets MaxAtomicSizeInBitsSupported=128 and custom-lowers `atomicrmw xchg i128` / `cmpxchg i128` to NVPTXISD::ATOMIC_SWAP_B128 / ATOMIC_CMP_SWAP_B128, selected (selectAtomicSwap128, NVPTXISelDAGToDAG.cpp:2261) into ATOM_EXCH_B128 / ATOM_CAS_B128. Those instrs' asm strings are `atom${sem:sem}${scope:scope}${addsp:addsp}.exch.b128` / `...cas.b128` (NVPTXIntrinsics.td:2765,2777). A plain (un-syncscope'd) atomicrmw/cmpxchg has system scope, so getAtomicScope() -> NVPTX::Scope::System and printAtomicCode prints `.sys` (NVPTXInstPrinter.cpp:335-336). Result at -mattr=+ptx83: `atom.relaxed.sys.exch.b128` / `atom.relaxed.sys.cas.b128` under `.version 8.3`. Per NVIDIA PTX ISA 8.4 release notes (section 1.3 'Changes in PTX ISA Version 8.4'): "Extends ld, st and atom instructions with .b128 type to support .sys scope." i.e. .sys scope on a .b128 atom was first introduced in PTX ISA 8.4; at PTX 8.3 only .cta/.gpu/.cluster scopes are legal for .b128 atom. ptxas at -arch=sm_90 with ISA 8.3 therefore rejects the emitted `.sys` form. The b128 type itself is correctly gated at ptx83, but there is no separate guard requiring ptx>=84 for the .sys scope, and .sys is the *default* scope for an ordinary i128 atomicrmw/cmpxchg. The in-tree test llvm/test/CodeGen/NVPTX/atomics-b128.ll only exercises ptx82 (rejected) and ptx84 (the .sys form), skipping the broken ptx83 boundary entirely.

## Trigger

Any well-defined `atomicrmw xchg i128` or `cmpxchg i128` on a generic/global/shared pointer with default (system) scope, compiled with -mcpu=sm_90 (or sm_90a) -mattr=+ptx83. No UB. .gpu/.cta/.cluster syncscopes are fine at 8.3; only the default .sys is invalid.

## Reproducer

```
target triple = "nvptx64-nvidia-cuda"

define i128 @xchg128(ptr %p, i128 %v) {
  %r = atomicrmw xchg ptr %p, i128 %v monotonic
  ret i128 %r
}

define i128 @cas128(ptr %p, i128 %c, i128 %v) {
  %r = cmpxchg ptr %p, i128 %c, i128 %v monotonic monotonic
  %x = extractvalue { i128, i1 } %r, 0
  ret i128 %x
}
```

Command: `llc -mtriple=nvptx64 -mcpu=sm_90 -mattr=+ptx83 -o - repro.ll`

## Verification

Reproduced with the built llc (crash/emitted-PTX matches the claim; finder confidence 0.85).
