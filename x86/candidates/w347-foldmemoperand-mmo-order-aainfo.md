# w347: TargetInstrInfo::foldMemoryOperand (LoadMI overload) appends LoadMI MMOs AFTER MI MMOs, losing AAInfo precedence

## Status
SUSPECTED. The MMO-ordering convention is "primary memop first" for downstream consumers that take `memoperands_begin()`. The fold path violates this whenever MI is not memoperands-empty.

## Source
`llvm/lib/CodeGen/TargetInstrInfo.cpp:854-866`
```cpp
// Copy the memoperands from the load to the folded instruction.
if (MI.memoperands_empty()) {
  NewMI->setMemRefs(MF, LoadMI.memoperands());
} else {
  // Handle the rare case of folding multiple loads.
  NewMI->setMemRefs(MF, MI.memoperands());
  for (MachineInstr::mmo_iterator I = LoadMI.memoperands_begin(),
                                  E = LoadMI.memoperands_end();
       I != E; ++I) {
    NewMI->addMemOperand(MF, *I);
  }
}
```

## Description
This is the overload of `foldMemoryOperand` taking a `LoadMI` argument (called from `LiveRangeEdit::foldAsLoad` line 156 and `InlineSpiller::foldMemoryOperand` line 1065 when rematting a foldable load into a use). The LoadMI represents the load whose memory is being folded into `MI`; LoadMI carries the meaningful `AAInfo` and metadata for the access being inlined.

When `MI.memoperands_empty()`, NewMI gets only LoadMI's MMOs — correct.

When MI ALREADY has MMOs (e.g., an RMW like `add [mem], reg` or any inline-asm with mem operands), NewMI gets MI's MMOs first then LoadMI's appended. Several X86 InstrInfo consumers assume `memoperands_begin()` returns the *primary* (newly-folded) access:
- `X86InstrInfo.cpp:8266`: `Alignment = (*LoadMI.memoperands_begin())->getAlign();` — this consumer happens to take the right MI, but symmetric consumers walking the folded NewMI's `memoperands()[0]` will see the WRONG access metadata.
- AA query infrastructure walks `MachineMemOperand::getAAInfo()` and may be order-sensitive when computing may-alias for the folded combo.

The "rare case" comment suggests this branch was added defensively without auditing downstream consumers.

## Severity
Low-frequency miscompile risk in MachineLICM / sink / post-RA scheduler when both:
1. MI already had MMOs (RMW or inline-asm fold), AND
2. NewMI's downstream consumer reads `memoperands().front()`.

Otherwise it's a missed-AA opportunity.

## Reproducer attempt
Difficult to trigger with x86 -O2 because most fold sites operate on MI that has no MMOs (plain arithmetic). RMW-targeting `foldAsLoad` with a TBAA-tagged load source is the path of interest:
```
define i32 @f(ptr %p, ptr %q) {
  %v = load i32, ptr %p, !tbaa !2     ; will be folded into use
  store i32 %v, ptr %q, !tbaa !3      ; ; "use" already has a store MMO
  ret i32 %v
}
```
But `foldAsLoad` requires single def + single use, so this exact pattern won't trigger. Inline-asm with `m` constraints and a foldable load source can.

## Fix sketch
Either:
1. Reverse the order: `setMemRefs(LoadMI.memoperands())` then append MI's. Then the new primary access metadata is first.
2. Drop the "preserve MI MMOs" branch entirely — if MI had memoperands and the fold succeeded, MI's other accesses should still hang off NewMI but as secondary.
3. Document and audit consumers that assume `memoperands()[0]` is the folded access.
