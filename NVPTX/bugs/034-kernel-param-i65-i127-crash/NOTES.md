# 034 — ptx_kernel with an integer parameter of width 65-127 bits crashes the AsmPrinter (llvm_unreachable "Integer too large")

- **Kind:** crash (assert/abort)
- **Reachable via:** default llc
- **Component:** NVPTXAsmPrinter.cpp 1486-1494 (kernel scalar-int param path in emitFunctionParamList) -> 1256-1267 (getPTXFundamentalTypeStr), abort at line 1266  (round-6 area `W20-callconv-return`)
- **Note:** "invalid PTX" entries are validated against the PTX ISA + strong in-tree corroboration (sibling guards / orderings); no local `ptxas` was available to execute the rejection.

## Summary

ptx_kernel integer parameter of width 65–127 bits hits `llvm_unreachable("Integer too large")` in the AsmPrinter param printer

## Mechanism / root cause

In emitFunctionParamList, the kernel non-pointer integer scalar parameter path prints the PTX type directly from getPTXFundamentalTypeStr(Ty) with NO size promotion:

  // non-pointer scalar to kernel func
  O << "\t.param .";
  if (Ty->isIntegerTy(1)) O << "u8";
  else                    O << getPTXFundamentalTypeStr(Ty);

getPTXFundamentalTypeStr (NVPTXAsmPrinter.cpp:1256) only handles integer widths up to 64:

  unsigned NumBits = cast<IntegerType>(Ty)->getBitWidth();
  if (NumBits == 1) return "pred";
  if (NumBits <= 64) { return "u" + utostr(NumBits); }
  llvm_unreachable("Integer too large");   // line 1266

shouldPassAsArray() only routes integers with scalar size >= 128 to the .b8[] array path, so a scalar integer in [65,127] bits (e.g. i65, i72, i96, i127) is treated as a fundamental scalar kernel param and reaches getPTXFundamentalTypeStr with NumBits in 65..127, hitting llvm_unreachable. This aborts the compiler (SIGABRT, exit 134) while printing the kernel's parameter list. Note the SDAG body for the same kernel already lowered fine (it uses ld.param.v2.b64 on a 16-byte slot), and the equivalent NON-kernel device function path (lines 1498-1506) handles these widths correctly via promoteScalarArgumentSize -> .param .b128. So only the kernel param-declaration printer crashes. This is distinct from #026 (kernel int params of width <=64 such as i48/i24 emit invalid .u48/.u24 but do NOT crash, since getPTXFundamentalTypeStr returns "u"+NumBits for NumBits<=64) and from #025 (an assert in LowerCall's ArgDeclare lambda on the call site, a different function and code path). llvm_unreachable does not degrade to a graceful report_fatal_error: with assertions enabled it aborts, and in a no-assert release build it becomes __builtin_unreachable() (genuine UB), so it is not a clean fatal-error exit.

## Trigger

A ptx_kernel function with a non-pointer integer parameter whose bit width is in [65,127] (i65, i72, i96, i100, i127, ...). Confirmed for i65/i72/i96/i100/i127. i128 (>=128) takes the array path and does not crash; non-kernel functions with the same params emit .param .b128 and do not crash. The IR is valid (passes opt -passes=verify) and contains no UB.

## Reproducer

```
define ptx_kernel void @kern(i65 %x, ptr %out) {
  %t = trunc i65 %x to i64
  store i64 %t, ptr %out
  ret void
}
```

Command: `llc -mtriple=nvptx64 -mcpu=sm_70 -o - repro.ll`

## Verification

Reproduced with the built llc (emitted PTX / crash matches the claim; finder confidence 0.9, confirmed_with_llc=True).
