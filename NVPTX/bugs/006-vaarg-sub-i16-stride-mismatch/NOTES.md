# 006 — LowerVAARG advances va_list by the promoted register-type size, mismatching the caller's sub-i16 vararg packing (miscompiled second small arg)

- **Kind:** miscompile
- **Reachable via:** default llc
- **Component:** NVPTXISelLowering.cpp 3574-3613  (region `L12b-addrspace-vaarg`)
- **Candidate id:** c010

## Summary

`va_arg` of i8/i1 advances the va_list by 2 bytes while the caller packs at 1-byte stride

## Mechanism / root cause

For a va_arg of a sub-i16 integer type (e.g. i8, i1), the result type is illegal in NVPTX, so DAGTypeLegalizer::PromoteIntRes_VAARG (LegalizeIntegerTypes.cpp:1990) rebuilds the VAARG node with RegVT = TLI.getRegisterType(i8) = i16. By the time LowerVAARG runs, Node->getValueType(0) is i16, so VT=i16 and Ty=VT.getTypeForEVT()=i16. The pointer increment at lines 3601-3603 then uses getTypeAllocSize(Ty)=getTypeAllocSize(i16)=2:

    Tmp1 = DAG.getNode(ISD::ADD, DL, VAList.getValueType(), VAList,
                       DAG.getConstant(DAG.getDataLayout().getTypeAllocSize(Ty),
                                       DL, VAList.getValueType()));

So each i8 va_arg advances the va_list by 2 bytes. However, the NVPTX *caller* (LowerCall, lines 1588-1607) packs each sub-i32 integer variadic argument at its EVT alignment with no per-arg padding to 2 bytes: VAOffset is aligned to DAG.getEVTAlign(EltVT) where EltVT=promoteScalarIntegerPTX(i8)=i8 (align 1), and consecutive i8 args land at 1-byte stride. Empirically: caller_3i8 stores three i8 varargs at offsets 0,1,2 (__local_depot[3], st.b8 [SP], [SP+1], [SP+2]). But the callee reading two consecutive i8 va_args reads the first at offset 0 and the SECOND at offset 2 (llc emits 'ld.local.b16 %r1, [%rd2+2]' and advances va_list by 4), so it skips the byte at offset 1 where the caller stored the second argument. The second (and every subsequent consecutive sub-i16) variadic argument is therefore read from the wrong address, yielding a wrong value. The discrepancy is masked only when the next va_arg has alignment >= 2 (the re-alignment happens to recover the correct offset), so the bug shows up specifically for runs of consecutive i8/i1 variadic args.

## Trigger

nvptx64 target; a variadic callee that performs two or more consecutive va_arg reads of a sub-i16 integer type (i8 or i1), called by code that passes such args consecutively. Hits at any -O level and any sm_XX/ptx version that supports varargs (PTX>=6.0, sm>=30).

## Reproducer

See `repro.ll` / `cmd.sh`.

```
target triple = "nvptx64-nvidia-cuda"

; Callee: reads two consecutive i8 variadic args, returns the second.
; The second i8 should come from the byte the caller placed at offset 1,
; but the backend advances va_list by the promoted i16 alloc size (2),
; so it reads the second arg from offset 2 instead.
define i8 @second_i8(ptr %ap) {
  %first  = va_arg ptr %ap, i8
  %second = va_arg ptr %ap, i8
  ret i8 %second
}

; Matching caller packs the two i8 args at offsets 0 and 1 (1-byte stride).
declare i32 @variadic(i32, ...)
define i32 @caller(i8 %a, i8 %b) {
  %r = call i32 (i32, ...) @variadic(i32 0, i8 %a, i8 %b)
  ret i32 %r
}
```

Command:

```
llc -mtriple=nvptx64 -mcpu=sm_90 -O0 -o - repro.ll
```

## Observed (wrong) output

```
; CALLEE second_i8 (-O0): first va_arg reads offset 0, advances va_list by 2;
; second va_arg reads from base+2 (offset 2) and advances by 2.
	ld.param.b64 	%rd1, [second_i8_param_0];
	ld.b64 	%rd2, [%rd1];
	add.s64 	%rd3, %rd2, 2;        // va_list += 2 after first i8
	st.b64 	[%rd1], %rd3;
	ld.b64 	%rd4, [%rd1];          // %rd4 = base+2
	add.s64 	%rd5, %rd4, 2;        // va_list += 2 after second i8
	st.b64 	[%rd1], %rd5;
	ld.local.b16 	%r1, [%rd4];      // reads SECOND i8 from offset 2 (wrong!)
	st.param.b32 	[func_retval0], %r1;

; CALLER caller(i8 %a, i8 %b) (-O0): 2-byte depot; %a at offset 0, %b at offset 1.
	.local .align 1 .b8 	__local_depot1[2];
	ld.param.b8 	%rs2, [caller_param_1];
	ld.param.b8 	%rs1, [caller_param_0];
	st.b8 	[%SP], %rs1;             // %a -> offset 0
	st.b8 	[%SP+1], %rs2;           // %b -> offset 1  (second arg stored here)

; => caller stores 2nd i8 at offset 1, callee reads 2nd i8 from offset 2 (OOB).
```

## Expected

The callee must read each consecutive i8 variadic argument at the same 1-byte stride the caller uses. After the first va_arg (offset 0), the va_list should advance by 1 byte, so the second i8 va_arg is read from offset 1 (where the caller stored it via `st.b8 [%SP+1], %b`). For input caller(10, 20), second_i8 should return 20. As emitted, it reads offset 2 (out of bounds of the 2-byte caller depot) and returns garbage. Fix: LowerVAARG should advance va_list (and load) using the original value type's alloc size (1 for i8), not the promoted register type's (i16 -> 2); i.e. use the pre-promotion EVT/store size that matches LowerCall's packing.

## Verification

Verified empirically with the built llc (independent verify + adversarial
refute both `confirmed`, finder confidence 0.82, verify confidence 0.95).

> Confirmed genuine miscompile. The NVPTX caller and callee disagree on the ABI layout for runs of consecutive sub-i16 integer variadic arguments.

Source confirmation (NVPTXISelLowering.cpp):
- LowerVAARG (3580-3603): VT = Node->getValueType(0). For a va_arg of i8, DAGTypeLegalizer::PromoteIntRes_VAARG rebuilds the VAARG with the register type i16, so VT=i16, Ty=i16, and the va_list increment at 3601-3603 is getTypeAllocSize(i16)=2. The i8 is also loaded as i16 (ld.local.b16). So the callee uses a 2-byte stride per i8 va_arg.
- LowerCall (1588-1607): for variadic args VAOffset is aligned to getEVTAlign(EltVT). For i8, EltVT=i8 (align 1), and the store width is i8, so consecutive i8 varargs are packed at 1-byte stride.

Empirical confirmation with the built llc (-mtriple=nvptx64 -mcpu=sm_90, both -O0 and -O2):
- caller(i8 %a, i8 %b): allocates __local_depot1[2] (2 bytes, align 1) and stores `st.b8 [%SP], %a` (offset 0) and `st.b8 [%SP+1], %b` (offset 1). Second i8 is at offset 1.
- second_i8: first va_arg reads offset 0 and advances va_list by 2; second va_arg reads `ld.local.b16 [base+2]` (offset 2) and advances by 2. Second i8 is read from offset 2, NOT offset 1.

So the second argument byte the caller placed at offset 1 is skipped; the callee reads offset 2, which is out of bounds of the 2-byte depot. With concrete defined input caller(10,20), second_i8 should return 20 (the byte at offset 1) but instead returns whatever is at offset 2 (uninitialized/OOB), so the result is w

## Independent cross-check (caller vs callee disagree)

Callee reading two consecutive `i8` `va_arg`s:
```
ld.local.b16 %rs1, [%rd3];      ; arg0 at offset 0
add.s64      %rd4, %rd3, 2;     ; advance va_list by 2
ld.local.b16 %rs2, [%rd3+2];    ; arg1 at offset 2  <-- stride 2
```
Caller passing two `i8` varargs:
```
.local .align 1 .b8 __local_depot0[2];   ; only 2 bytes total -> stride 1 (offsets 0,1)
```
The caller writes arg1 at **offset 1**; the callee reads arg1 from **offset 2**,
which is past the caller's 2-byte outgoing-args block entirely. Definitive ABI
mismatch: every consecutive sub-i16 variadic argument after the first is read
from the wrong address.
