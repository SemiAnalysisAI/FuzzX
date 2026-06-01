# c010 — LowerVAARG advances va_list by the promoted register-type size, mismatching the caller's sub-i16 vararg packing (miscompiled second small arg)

- region: L12b-addrspace-vaarg
- file: NVPTXISelLowering.cpp 3574-3613
- kind: miscompile
- confidence(finder): 0.82

## Mechanism
For a va_arg of a sub-i16 integer type (e.g. i8, i1), the result type is illegal in NVPTX, so DAGTypeLegalizer::PromoteIntRes_VAARG (LegalizeIntegerTypes.cpp:1990) rebuilds the VAARG node with RegVT = TLI.getRegisterType(i8) = i16. By the time LowerVAARG runs, Node->getValueType(0) is i16, so VT=i16 and Ty=VT.getTypeForEVT()=i16. The pointer increment at lines 3601-3603 then uses getTypeAllocSize(Ty)=getTypeAllocSize(i16)=2:

    Tmp1 = DAG.getNode(ISD::ADD, DL, VAList.getValueType(), VAList,
                       DAG.getConstant(DAG.getDataLayout().getTypeAllocSize(Ty),
                                       DL, VAList.getValueType()));

So each i8 va_arg advances the va_list by 2 bytes. However, the NVPTX *caller* (LowerCall, lines 1588-1607) packs each sub-i32 integer variadic argument at its EVT alignment with no per-arg padding to 2 bytes: VAOffset is aligned to DAG.getEVTAlign(EltVT) where EltVT=promoteScalarIntegerPTX(i8)=i8 (align 1), and consecutive i8 args land at 1-byte stride. Empirically: caller_3i8 stores three i8 varargs at offsets 0,1,2 (__local_depot[3], st.b8 [SP], [SP+1], [SP+2]). But the callee reading two consecutive i8 va_args reads the first at offset 0 and the SECOND at offset 2 (llc emits 'ld.local.b16 %r1, [%rd2+2]' and advances va_list by 4), so it skips the byte at offset 1 where the caller stored the second argument. The second (and every subsequent consecutive sub-i16) variadic argument is therefore read from the wrong address, yielding a wrong value. The discrepancy is masked only when the next va_arg has alignment >= 2 (the re-alignment happens to recover the correct offset), so the bug shows up specifically for runs of consecutive i8/i1 variadic args.

## Trigger
nvptx64 target; a variadic callee that performs two or more consecutive va_arg reads of a sub-i16 integer type (i8 or i1), called by code that passes such args consecutively. Hits at any -O level and any sm_XX/ptx version that supports varargs (PTX>=6.0, sm>=30).

## IR
```
target triple = "nvptx64-nvidia-cuda"

; Callee: reads two consecutive i8 variadic args. The second i8 is read
; from offset 2 (ld.local.b16 [ap+2]) instead of offset 1, because the
; backend advances va_list by the promoted i16 size (2) per i8, while the
; caller packs consecutive i8 varargs at 1-byte stride.
define i8 @second_i8(ptr %ap) {
  %first  = va_arg ptr %ap, i8
  %second = va_arg ptr %ap, i8
  ret i8 %second
}

; Matching caller packs the two i8 args at offsets 0 and 1.
declare i32 @variadic(i32, ...)
define i32 @caller(i8 %a, i8 %b) {
  %r = call i32 (i32, ...) @variadic(i32 0, i8 %a, i8 %b)
  ret i32 %r
}
```

## llc cmd
`-mtriple=nvptx64 -mcpu=sm_90 -O2`
