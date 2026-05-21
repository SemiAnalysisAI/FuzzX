# 218 — Verifier null-deref crash on malformed value-profile `!prof`

Component: `llvm/lib/IR/Verifier.cpp` lines ~5432-5438 (`visitProfMetadata` VP path)

`visitProfMetadata` walks the value-profile operand list and unconditionally calls `mdconst::dyn_extract<ConstantInt>(MD->getOperand(I))->getZExtValue()`. If any operand is a non-ConstantInt (string, metadata-as-value, null), the dyn_extract returns nullptr, and `->getZExtValue()` deref-crashes.

`isValueProfileMD` (`llvm/lib/IR/ProfDataUtils.cpp:54-70`) only validates that operand 0 is the string `"VP"` and that there are at least 5 operands. Per-element type/null guards are missing.

The branch-weights path right above (lines 5404-5410) DOES null-check each operand — the VP path forgot.

## Reproducer

```ll
!0 = !{!"VP", i32 0, i64 100, !"oops", i64 50}
```

`opt -S input.ll` crashes immediately with a stack trace. Also crashes round-tripped through bitcode (`opt -disable-verify in.ll -o x.bc; opt x.bc -S`).

## Severity

Crash. Reachable via crafted IR (e.g., via custom IR construction or modified bitcode). Should be a verifier error, not a crash.

## Fix

Mirror the branch-weights path: null-check each `mdconst::dyn_extract<ConstantInt>(...)` before deref.
