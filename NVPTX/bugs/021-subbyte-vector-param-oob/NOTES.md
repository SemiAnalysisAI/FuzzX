# 021 — Sub-byte vector params (<N x i1>) emit out-of-bounds param stores/loads, corrupting values and adjacent params

- **Kind:** miscompile
- **Reachable via:** default llc
- **Component:** NVPTXISelLowering.cpp 324-331 (ComputePTXValueVTs flatten); 4144-4177 (LowerFormalArguments load); 1558-1628 (LowerCall store)  (round-3 area `T06-formalargs-ext`)
- **Candidate id:** r3_09

## Summary

`<N x i1>` param is declared ceil(N/8) bytes but elements are loaded/stored at byte offsets 0..N-1 (out of the slot)

## Mechanism / root cause

For a parameter of type <N x i1>, the PTX .param storage is declared with size DL.getTypeAllocSize(<N x i1>) = ceil(N/8) bytes (e.g. 1 byte for <2..8 x i1>). But ComputePTXValueVTs (lines 308-331) flattens the vector via ComputeValueVTs into N separate scalar i1 entries, then in the i8-handling block forces RegisterVT=i8 and assigns Offsets[I] = Off + I*RegisterVT.getStoreSize() = 0,1,2,...,N-1. The comment at lines 324-327 explicitly flags this: "This is horribly incorrect for cases where the vector elements are not a multiple of bytes (ex i1) ... no one has complained." LowerFormalArguments (line 4155-4159) then loads each element with VecAddr = ArgSymbol + Offsets[I] (i.e. param+1, param+2, ...), and LowerCall (line 1609-1623) stores at the same offsets. Both the caller's store loop and the callee's load loop access bytes 0..N-1 of a param declared with only ceil(N/8) bytes, i.e. up to N - ceil(N/8) bytes past the end of the .param slot. In the PTX param state space each .param declaration is independent fixed-size storage, so these accesses are out-of-bounds; ptxas behavior is undefined and may silently corrupt the adjacent parameter slot or read garbage, producing a result different from the well-defined IR semantics.

## Trigger

Any function with a <N x i1> argument where N >= 2 (so N > ceil(N/8)), as caller and/or callee. Confirmed: <2 x i1>, <4 x i1>, <8 x i1>. Valid, non-UB IR.

## Reproducer

```
target triple = "nvptx64-nvidia-cuda"

; Kernel ABI miscompile: <8 x i1> param is declared 1 byte (packed, 8 bits) per
; getTypeAllocSize and the canonical in-memory/global layout, but the generated
; PTX reads element 7 from byte offset +7 of a 1-byte param slot. The host driver
; only ever populates the single declared byte, so byte +7 is out of bounds.
; Defined input: host passes 1 byte = 0x80 (only element 7 set) => must return 1.
define ptx_kernel void @kern(<8 x i1> %a, ptr %out) {
  %e7 = extractelement <8 x i1> %a, i32 7
  %z7 = zext i1 %e7 to i32
  store i32 %z7, ptr %out
  ret void
}

; Confirms canonical layout: <8 x i1> is 1 packed byte as a global.
@g = global <8 x i1> zeroinitializer
```

Command: `llc -mtriple=nvptx64 -mcpu=sm_70 -o - repro.ll`

## Observed (wrong) output

```
.visible .global .align 1 .b8 g[1];
.visible .entry kern(
	.param .align 1 .b8 kern_param_0[1],
	.param .u64 .ptr .align 1 kern_param_1
)
{
	ld.param.b64 	%rd1, [kern_param_1];
	cvta.to.global.u64 	%rd2, %rd1;
	ld.param.b8 	%r1, [kern_param_0+7];   // OOB: reads byte 7 of a 1-byte param slot
	and.b32 	%r2, %r1, 1;
	st.global.b32 	[%rd2], %r2;
	ret;
}

; Same defect for <4 x i1> (kern4_param_0[1] but `ld.param.b8 [kern4_param_0+3]`),
; and for device calls the caller spreads/packs across [param0+0..7] while declaring
; `.param .align 1 .b8 param0[1]` (1 byte) — store/load offsets 0..7 into a 1-byte slot.
```

## Expected

The <8 x i1> kernel param occupies exactly 1 byte (getTypeAllocSize, matching the global g[1] and the packed `st.b8` in-memory layout), with the 8 elements packed as bits 0..7 of that byte. The host driver fills only that 1 byte. The correct lowering must load the single declared byte and extract bit 7, e.g.:
	ld.param.b8 	%r1, [kern_param_0];
	bfe/shr 	%r2, %r1, 7, 1;   // (byte0 >> 7) & 1
	st.global.b32 	[%rd2], %r2;
i.e. all 8 elements come from byte 0 of the 1-byte slot, never from offsets +1..+7 which lie past the declared (host-supplied) parameter storage. For input byte 0x80 the result must be 1.

## Verification

Independent verify + adversarial refute, both `confirmed` (verify confidence 0.9).

> Confirmed real miscompile. Source: ComputePTXValueVTs (NVPTXISelLowering.cpp 308-331) flattens <N x i1> into N scalar i1 entries and assigns Offsets[I] = Off + I*RegisterVT.getStoreSize() = 0,1,...,N-1, with the comment at 324-327 explicitly admitting this is "horribly incorrect" for i1. The .param slot, however, is declared with DL.getTypeAllocSize = ceil(N/8) bytes (1505/1514; emitted as [1] in PTX). The load loop (4158-4159) and store loop (1609-1610) then access byte offsets 0..N-1 of that ceil(N/8)-byte slot.

Canonical layout proven: globals g[1]/g4[1]/g2[1] are 1 byte each, and `store <8 x i1>` lowers to a single packed `st.b8` of (b0|b1<<1|...|b7<<7). So the ABI/IR contract for the 1-byte param slot is: 8 bits packed into byte 0.

Decisive miscompile = kernel ABI. For `define ptx_kernel void @kern(<8 x i1> %a, ptr %out)` extracting element 7, llc emits `.param .align 1 .b8 kern_param_0[1]` (1-byte host-facing slot) but `ld.param.b8 [kern_param_0+7]` — reading byte 7 of a slot the host driver only ever fills with 1 byte. Concrete defined input: host passes the single byte 0x80 (only element 7 set). IR semantics (extractelement %a,7; zext) require result 1, computed as (byte0>>7)&1. The PTX instead reads out-of-bounds byte 7 (undefined/garbage), not byte0 bit7. Same defect for <4 x i1> reading [+3]. The input is a plain, fully-defined function parameter (no undef/poison/f
