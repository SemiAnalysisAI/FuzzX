# 026 — Kernel integer parameter with non-fundamental bit width emits invalid `.param .u<N>` type (e.g. .u48, .u24, .u3, .u17)

- **Kind:** other (invalid PTX)
- **Reachable via:** default llc
- **Component:** NVPTXAsmPrinter.cpp 1486-1493 (emitFunctionParamList kernel scalar-int path); helper getPTXFundamentalTypeStr at 1256-1296  (round-4 area `U07-asmprinter-param-retval`)
- **Candidate id:** r4_01

## Summary

ptx_kernel integer param of non-fundamental width (i48/i24/i3...) emits `.param .u48` etc. — not a legal PTX type (ptxas rejects)

## Mechanism / root cause

In emitFunctionParamList, the non-pointer scalar parameter path for KERNEL functions emits the parameter type directly from getPTXFundamentalTypeStr(Ty) with NO size promotion:

  // non-pointer scalar to kernel func
  O << "\t.param .";
  if (Ty->isIntegerTy(1)) O << "u8";
  else                    O << getPTXFundamentalTypeStr(Ty);

getPTXFundamentalTypeStr() returns "u"+utostr(NumBits) for any IntegerType with NumBits<=64 (lines 1262-1265). So an i48 kernel param becomes `.param .u48`, i24 -> `.u24`, i3 -> `.u3`, i17 -> `.u17`, i33 -> `.u33`, i40/i56 -> `.u40`/`.u56`, etc. PTX has only the fundamental integer types .u8/.u16/.u32/.u64 (and .s8.. / .b8..); `.u48` and friends are not legal types and ptxas rejects the module. Compare the NON-kernel path immediately below (lines 1498-1506) which correctly does `Size = promoteScalarArgumentSize(ITy->getBitWidth())` and emits `.param .b<Size>` (valid). The kernel path simply forgot the promotion. The loader is also inconsistent with the decl: for k_i48 the body emits `ld.param.b64` (8-byte slot) from a param declared `.u48`. So for any kernel function whose integer parameter bit width is not exactly 8/16/32/64, the backend silently emits unassemblable PTX for well-defined IR.

## Trigger

A ptx_kernel function taking an integer parameter whose bit width is not a valid PTX fundamental size (i2-i7, i17, i24, i33, i40, i48, i56, etc.). Non-kernel functions with the same params are fine (use promoteScalarArgumentSize).

## Reproducer

```
define ptx_kernel void @k_i48(i48 %x, ptr %o) {
  store i48 %x, ptr %o
  ret void
}
```

Command: `llc -mtriple=nvptx64 -mcpu=sm_70 -o - repro.ll`

## Observed (wrong) output

```
.visible .entry k_i48(
	.param .u48 k_i48_param_0,
	.param .u64 .ptr .align 1 k_i48_param_1
)
{
	.reg .b64 	%rd<5>;
// %bb.0:
	ld.param.b64 	%rd1, [k_i48_param_0];
	...

(`.param .u48` is not a legal PTX type; ptxas would reject the module. Also verified i24->.u24, i17->.u17, i3->.u3.)
```

## Expected

The kernel integer parameter should be size-promoted to a valid PTX fundamental type, matching the non-kernel path and the body's ld.param.b64. Correct declaration:

	.param .u64 k_i48_param_0,

i.e. promoteScalarArgumentSize(48)=64. (For comparison, an identical non-kernel function emits `.param .b64`, and a kernel i32 param correctly emits `.param .u32`.) PTX only permits integer types .u8/.u16/.u32/.u64 (and .s*/.b* equivalents); .u48/.u24/.u17/.u3 are invalid.

## Verification

Verified empirically with the built llc (reproduced directly by the orchestrator). 

> CONFIRMED REAL BUG, but it does not fit the schema's miscompile/segfault/assertion buckets — it is an "invalid/unassemblable PTX output" bug, so I set real=true with the closest non-crash kind ("not-a-bug" is the only remaining enum value; the descriptive truth is in this field). The mechanism is exactly as described and verified against source + llc output.

Source confirmation: NVPTXAsmPrinter.cpp:1486-1494 (kernel non-pointer scalar path) emits `getPTXFundamentalTypeStr(Ty)` with NO size promotion. getPTXFundamentalTypeStr (1262-1265) returns "u"+utostr(NumBits) for any IntegerType with NumBits<=64. shouldPassAsArray (NVPTXUtilities.h:64-67) only diverts scalars >=128 bits (plus aggregates/vectors/half/bfloat), so i48/i24/i17/i3 fall through to the scalar kernel path. The non-kernel path directly below (1499-1506) correctly applies promoteScalarArgumentSize and emits `.param .b<Size>` (32/64/128).

Empirical confirmation: `llc -mtriple=nvptx64 -mcpu=sm_70` on a well-defined ptx_kernel taking i48 emits `.param .u48 k_i48_param_0`. PTX fundamental integer types are only .u8/.u16/.u32/.u64 (and .s*/.b* equivalents); .u48/.u24/.u17/.u3 are not valid PTX types and ptxas rejects the m
