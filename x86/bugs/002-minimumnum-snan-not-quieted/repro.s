	.att_syntax
	.file	"repro.ll"
	.text
	.globl	minimumnum_x_qnan               # -- Begin function minimumnum_x_qnan
	.prefalign	4, .Lfunc_end0, nop
	.type	minimumnum_x_qnan,@function
minimumnum_x_qnan:                      # @minimumnum_x_qnan
	.cfi_startproc
# %bb.0:
	movq	%xmm0, %rax
	retq
.Lfunc_end0:
	.size	minimumnum_x_qnan, .Lfunc_end0-minimumnum_x_qnan
	.cfi_endproc
                                        # -- End function
	.globl	minimumnum_f32_x_qnan           # -- Begin function minimumnum_f32_x_qnan
	.prefalign	4, .Lfunc_end1, nop
	.type	minimumnum_f32_x_qnan,@function
minimumnum_f32_x_qnan:                  # @minimumnum_f32_x_qnan
	.cfi_startproc
# %bb.0:
	movd	%xmm0, %eax
	retq
.Lfunc_end1:
	.size	minimumnum_f32_x_qnan, .Lfunc_end1-minimumnum_f32_x_qnan
	.cfi_endproc
                                        # -- End function
	.section	".note.GNU-stack","",@progbits
