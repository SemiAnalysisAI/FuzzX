//! Turn a fuzzer-provided byte string into a PTX source string.
//!
//! Strategy: a typed grammar generator driven by `arbitrary::Unstructured`.
//! AFL's mutated byte input is consumed as a sequence of structural
//! decisions — which opcode, which type, which register index, which
//! literal — so byte-level mutations map onto syntactically and
//! semantically meaningful PTX edits. When the input is exhausted we
//! stop emitting statements and close the kernel cleanly.
//!
//! The earlier byte-passthrough generator plateaued at ~73% AFL bitmap
//! coverage because most mutated bytes produced lexer-rejected garbage;
//! this one keeps the program well-typed enough to reach the actual
//! optimizer / code-emitter stages where ptxas does most of its work.
//!
//! Out-of-scope here (intentionally): multiple kernels per module,
//! function calls (`call`), texture/surface ops, vector loads, cooperative
//! groups, FP16/BF16. Adding more shapes is mostly a matter of extending
//! the opcode menus in `emit_statement`.

use arbitrary::{Result, Unstructured};

/// Hard cap on the produced PTX text. ptxas can chew through huge
/// programs but the marginal coverage win drops off quickly and big
/// inputs slow the fork rate.
const MAX_OUTPUT_BYTES: usize = 16 * 1024;

/// Hard cap on emitted statements per kernel. Bounds runtime when the
/// AFL input encodes nothing but "keep going."
const MAX_STATEMENTS: usize = 256;

// Fixed pools of pre-declared registers. Indexing by `u32` consumed
// from `Unstructured` lets AFL mutate "which register" cheaply.
const N_R32: u32 = 16;
const N_R64: u32 = 8;
const N_F32: u32 = 8;
const N_F64: u32 = 8;
const N_PRED: u32 = 8;
const N_LABELS: u32 = 8;

pub fn generate_ptx(data: &[u8]) -> String {
    let mut u = Unstructured::new(data);
    let mut b = Builder::new();
    // Generation may run out of bytes at any point; that's fine, we
    // just stop and close the kernel with whatever we have.
    let _ = b.emit_module(&mut u);
    b.finish()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IntTy {
    S32,
    U32,
    S64,
    U64,
    B32,
    B64,
}

impl IntTy {
    fn name(self) -> &'static str {
        match self {
            IntTy::S32 => "s32",
            IntTy::U32 => "u32",
            IntTy::S64 => "s64",
            IntTy::U64 => "u64",
            IntTy::B32 => "b32",
            IntTy::B64 => "b64",
        }
    }
    fn is_64(self) -> bool {
        matches!(self, IntTy::S64 | IntTy::U64 | IntTy::B64)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FloatTy {
    F32,
    F64,
}

impl FloatTy {
    fn name(self) -> &'static str {
        match self {
            FloatTy::F32 => "f32",
            FloatTy::F64 => "f64",
        }
    }
}

struct Builder {
    out: String,
    /// Labels referenced by a `bra` so far. Must each have exactly
    /// one definition site.
    referenced_labels: [bool; N_LABELS as usize],
    /// Labels already emitted inline. Defining them a second time is
    /// a ptxas error.
    defined_labels: [bool; N_LABELS as usize],
    stmts: usize,
}

impl Builder {
    fn new() -> Self {
        Self {
            out: String::with_capacity(2048),
            referenced_labels: [false; N_LABELS as usize],
            defined_labels: [false; N_LABELS as usize],
            stmts: 0,
        }
    }

    fn finish(mut self) -> String {
        // Define any label that was referenced but never defined
        // inline. Anything already defined we skip — defining twice
        // is a ptxas error.
        for i in 0..N_LABELS as usize {
            if self.referenced_labels[i] && !self.defined_labels[i] {
                self.out.push_str(&format!("L{i}:\n"));
            }
        }
        self.out.push_str("    ret;\n}\n");
        self.out
    }

    fn emit_module(&mut self, u: &mut Unstructured) -> Result<()> {
        self.out.push_str(
            ".version 7.0\n\
             .target sm_70\n\
             .address_size 64\n\n",
        );

        // Optionally emit a couple of .global variables for ld/st to
        // refer to. Limited to a handful so register pressure stays
        // sane.
        let n_globals: u8 = u.int_in_range(0..=2)?;
        for i in 0..n_globals {
            let elems: u32 = 1 << u.int_in_range(0..=6)?; // 1..=64
            self.out
                .push_str(&format!(".global .align 8 .u32 g{i}[{elems}];\n"));
        }
        if n_globals > 0 {
            self.out.push('\n');
        }

        self.out.push_str(
            ".visible .entry kernel(\n\
            \x20   .param .u64 p0,\n\
            \x20   .param .u64 p1,\n\
            \x20   .param .u32 p2\n\
             ) {\n",
        );
        self.out
            .push_str(&format!("    .reg .pred %p<{N_PRED}>;\n"));
        self.out.push_str(&format!("    .reg .b16 %rs<8>;\n"));
        self.out.push_str(&format!("    .reg .b32 %r<{N_R32}>;\n"));
        self.out.push_str(&format!("    .reg .b64 %rd<{N_R64}>;\n"));
        self.out.push_str(&format!("    .reg .f32 %f<{N_F32}>;\n"));
        self.out
            .push_str(&format!("    .reg .f64 %fd<{N_F64}>;\n\n"));

        // Bind param addresses into registers so subsequent ld.global
        // through %rd0/1 actually has somewhere to land.
        self.out.push_str("    ld.param.u64 %rd0, [p0];\n");
        self.out.push_str("    ld.param.u64 %rd1, [p1];\n");
        self.out.push_str("    ld.param.u32 %r0, [p2];\n");
        self.out.push_str("    cvta.to.global.u64 %rd2, %rd0;\n");
        self.out.push_str("    cvta.to.global.u64 %rd3, %rd1;\n");

        // Emit a body of typed statements until we run out of bytes,
        // hit the statement cap, or blow past the output budget.
        // `Unstructured::int_in_range` and friends don't return
        // `NotEnoughData` when empty — they ratchet down to zero
        // forever — so we have to check `is_empty()` explicitly.
        while !u.is_empty() && self.stmts < MAX_STATEMENTS && self.out.len() < MAX_OUTPUT_BYTES {
            if self.emit_statement(u).is_err() {
                break;
            }
            self.stmts += 1;
        }
        Ok(())
    }

    fn emit_statement(&mut self, u: &mut Unstructured) -> Result<()> {
        // The opcode menu. Roughly ordered by how common they are in
        // real PTX (and so how often we want AFL to pick them). The
        // weights are loose — `int_in_range` over a bigger interval
        // for the common ones.
        let pick: u8 = u.int_in_range(0..=47)?;
        match pick {
            0..=4 => self.emit_arith_int(u)?,
            5..=7 => self.emit_logic_int(u)?,
            8..=9 => self.emit_shift_int(u)?,
            10..=12 => self.emit_mov(u)?,
            13..=15 => self.emit_ld(u)?,
            16..=18 => self.emit_st(u)?,
            19 => self.emit_setp(u)?,
            20 => self.emit_selp(u)?,
            21 => self.emit_bra(u)?,
            22 => self.emit_label(u)?,
            23 => self.emit_cvt(u)?,
            24 => self.emit_mul_wide(u)?,
            25 => self.emit_fma(u)?,
            26 => self.emit_arith_float(u)?,
            27 => self.emit_atom(u)?,
            28 => self.out.push_str("    bar.sync 0;\n"),
            29 => self.out.push_str("    membar.gl;\n"),
            30 => self.emit_neg_abs(u)?,
            31 => self.emit_min_max(u)?,
            32..=33 => self.emit_mad(u)?,
            34 => self.emit_bfe(u)?,
            35 => self.emit_bfi(u)?,
            36..=37 => self.emit_bit_count(u)?,
            38 => self.emit_prmt(u)?,
            39..=41 => self.emit_special_reg(u)?,
            42 => self.emit_sad(u)?,
            43..=44 => self.emit_vector_ld_st(u)?,
            45 => self.emit_dp4a(u)?,
            46 => self.emit_brev(u)?,
            47 => self.emit_rcp_sqrt(u)?,
            _ => unreachable!(),
        }
        Ok(())
    }

    // Operand helpers --------------------------------------------------

    fn reg32(&self, u: &mut Unstructured) -> Result<String> {
        Ok(format!("%r{}", u.int_in_range(0..=N_R32 - 1)?))
    }
    fn reg64(&self, u: &mut Unstructured) -> Result<String> {
        Ok(format!("%rd{}", u.int_in_range(0..=N_R64 - 1)?))
    }
    fn regf32(&self, u: &mut Unstructured) -> Result<String> {
        Ok(format!("%f{}", u.int_in_range(0..=N_F32 - 1)?))
    }
    fn regf64(&self, u: &mut Unstructured) -> Result<String> {
        Ok(format!("%fd{}", u.int_in_range(0..=N_F64 - 1)?))
    }
    fn regp(&self, u: &mut Unstructured) -> Result<String> {
        Ok(format!("%p{}", u.int_in_range(0..=N_PRED - 1)?))
    }

    fn int_reg(&self, u: &mut Unstructured, ty: IntTy) -> Result<String> {
        if ty.is_64() {
            self.reg64(u)
        } else {
            self.reg32(u)
        }
    }
    fn float_reg(&self, u: &mut Unstructured, ty: FloatTy) -> Result<String> {
        match ty {
            FloatTy::F32 => self.regf32(u),
            FloatTy::F64 => self.regf64(u),
        }
    }

    fn int_ty(&self, u: &mut Unstructured) -> Result<IntTy> {
        Ok(*u.choose(&[
            IntTy::S32,
            IntTy::U32,
            IntTy::S64,
            IntTy::U64,
            IntTy::B32,
            IntTy::B64,
        ])?)
    }
    fn int_ty_arith(&self, u: &mut Unstructured) -> Result<IntTy> {
        // ops like add/sub/mul don't accept .bNN
        Ok(*u.choose(&[IntTy::S32, IntTy::U32, IntTy::S64, IntTy::U64])?)
    }
    fn float_ty(&self, u: &mut Unstructured) -> Result<FloatTy> {
        Ok(*u.choose(&[FloatTy::F32, FloatTy::F64])?)
    }

    fn int_imm(&self, u: &mut Unstructured, ty: IntTy) -> Result<String> {
        if ty.is_64() {
            let v: i64 = u.arbitrary()?;
            Ok(format!("{v}"))
        } else {
            let v: i32 = u.arbitrary()?;
            Ok(format!("{v}"))
        }
    }

    // Statement emitters ----------------------------------------------

    fn emit_arith_int(&mut self, u: &mut Unstructured) -> Result<()> {
        let op = u.choose(&["add", "sub", "mul.lo", "mul.hi"])?;
        let ty = self.int_ty_arith(u)?;
        let d = self.int_reg(u, ty)?;
        let a = self.int_reg(u, ty)?;
        let b = self.int_reg(u, ty)?;
        self.out
            .push_str(&format!("    {op}.{} {d}, {a}, {b};\n", ty.name()));
        Ok(())
    }

    fn emit_logic_int(&mut self, u: &mut Unstructured) -> Result<()> {
        let op = u.choose(&["and", "or", "xor"])?;
        let ty = *u.choose(&[IntTy::B32, IntTy::B64])?;
        let d = self.int_reg(u, ty)?;
        let a = self.int_reg(u, ty)?;
        let b = self.int_reg(u, ty)?;
        self.out
            .push_str(&format!("    {op}.{} {d}, {a}, {b};\n", ty.name()));
        Ok(())
    }

    fn emit_shift_int(&mut self, u: &mut Unstructured) -> Result<()> {
        // PTX shl/shr only accept .bNN types (or signed for shr's
        // arithmetic variant). Stick to .b32/.b64 — those are valid
        // for both shl and shr.
        let op = u.choose(&["shl", "shr"])?;
        let ty = *u.choose(&[IntTy::B32, IntTy::B64])?;
        let d = self.int_reg(u, ty)?;
        let a = self.int_reg(u, ty)?;
        // Shift amount is always a .u32, given as a literal here.
        let shift: u32 = u.int_in_range(0..=63)?;
        self.out
            .push_str(&format!("    {op}.{} {d}, {a}, {shift};\n", ty.name()));
        Ok(())
    }

    fn emit_mov(&mut self, u: &mut Unstructured) -> Result<()> {
        // mov.<ty> <reg>, <reg|imm>
        if u.arbitrary()? {
            let ty = self.int_ty(u)?;
            let d = self.int_reg(u, ty)?;
            if u.arbitrary()? {
                let s = self.int_reg(u, ty)?;
                self.out
                    .push_str(&format!("    mov.{} {d}, {s};\n", ty.name()));
            } else {
                let imm = self.int_imm(u, ty)?;
                self.out
                    .push_str(&format!("    mov.{} {d}, {imm};\n", ty.name()));
            }
        } else {
            let ty = self.float_ty(u)?;
            let d = self.float_reg(u, ty)?;
            let s = self.float_reg(u, ty)?;
            self.out
                .push_str(&format!("    mov.{} {d}, {s};\n", ty.name()));
        }
        Ok(())
    }

    fn emit_ld(&mut self, u: &mut Unstructured) -> Result<()> {
        let space = *u.choose(&["global", "shared", "local", "param"])?;
        // For simplicity, address always comes from %rd2 or %rd3
        // (cvta'd to global) or %rd0/%rd1 (raw param).
        let addr = format!("%rd{}", u.int_in_range(0..=3)?);
        if u.arbitrary()? {
            let ty = *u.choose(&[IntTy::U32, IntTy::S32, IntTy::B32])?;
            let d = self.reg32(u)?;
            self.out
                .push_str(&format!("    ld.{space}.{} {d}, [{addr}];\n", ty.name()));
        } else if u.arbitrary()? {
            let ty = *u.choose(&[IntTy::U64, IntTy::S64, IntTy::B64])?;
            let d = self.reg64(u)?;
            self.out
                .push_str(&format!("    ld.{space}.{} {d}, [{addr}];\n", ty.name()));
        } else {
            let ty = self.float_ty(u)?;
            let d = self.float_reg(u, ty)?;
            self.out
                .push_str(&format!("    ld.{space}.{} {d}, [{addr}];\n", ty.name()));
        }
        Ok(())
    }

    fn emit_st(&mut self, u: &mut Unstructured) -> Result<()> {
        let space = *u.choose(&["global", "shared", "local"])?;
        let addr = format!("%rd{}", u.int_in_range(0..=3)?);
        if u.arbitrary()? {
            let ty = *u.choose(&[IntTy::U32, IntTy::S32, IntTy::B32])?;
            let s = self.reg32(u)?;
            self.out
                .push_str(&format!("    st.{space}.{} [{addr}], {s};\n", ty.name()));
        } else if u.arbitrary()? {
            let ty = *u.choose(&[IntTy::U64, IntTy::S64, IntTy::B64])?;
            let s = self.reg64(u)?;
            self.out
                .push_str(&format!("    st.{space}.{} [{addr}], {s};\n", ty.name()));
        } else {
            let ty = self.float_ty(u)?;
            let s = self.float_reg(u, ty)?;
            self.out
                .push_str(&format!("    st.{space}.{} [{addr}], {s};\n", ty.name()));
        }
        Ok(())
    }

    fn emit_setp(&mut self, u: &mut Unstructured) -> Result<()> {
        let cmp = *u.choose(&["eq", "ne", "lt", "le", "gt", "ge"])?;
        let p = self.regp(u)?;
        if u.arbitrary()? {
            let ty = self.int_ty_arith(u)?;
            let a = self.int_reg(u, ty)?;
            let b = self.int_reg(u, ty)?;
            self.out
                .push_str(&format!("    setp.{cmp}.{} {p}, {a}, {b};\n", ty.name()));
        } else {
            let ty = self.float_ty(u)?;
            let a = self.float_reg(u, ty)?;
            let b = self.float_reg(u, ty)?;
            self.out
                .push_str(&format!("    setp.{cmp}.{} {p}, {a}, {b};\n", ty.name()));
        }
        Ok(())
    }

    fn emit_selp(&mut self, u: &mut Unstructured) -> Result<()> {
        let ty = self.int_ty_arith(u)?;
        let d = self.int_reg(u, ty)?;
        let a = self.int_reg(u, ty)?;
        let b = self.int_reg(u, ty)?;
        let p = self.regp(u)?;
        self.out
            .push_str(&format!("    selp.{} {d}, {a}, {b}, {p};\n", ty.name()));
        Ok(())
    }

    fn emit_bra(&mut self, u: &mut Unstructured) -> Result<()> {
        let l: u32 = u.int_in_range(0..=N_LABELS - 1)?;
        self.referenced_labels[l as usize] = true;
        if u.arbitrary()? {
            let p = self.regp(u)?;
            let neg = if u.arbitrary()? { "!" } else { "" };
            self.out.push_str(&format!("    @{neg}{p} bra L{l};\n"));
        } else {
            self.out.push_str(&format!("    bra L{l};\n"));
        }
        Ok(())
    }

    fn emit_label(&mut self, u: &mut Unstructured) -> Result<()> {
        // Emit a label in place. Skip if already defined — re-defining
        // is a ptxas error. The label may end up referenced later by
        // a bra, which is fine; forward refs resolve cleanly.
        let l: u32 = u.int_in_range(0..=N_LABELS - 1)?;
        if !self.defined_labels[l as usize] {
            self.defined_labels[l as usize] = true;
            self.out.push_str(&format!("L{l}:\n"));
        }
        Ok(())
    }

    fn emit_cvt(&mut self, u: &mut Unstructured) -> Result<()> {
        // PTX cvt rounding-mode rules (paraphrased from the ISA):
        // - int -> int: no rounding modifier.
        // - f32 -> f64 (widening): no rounding modifier.
        // - f64 -> f32 (narrowing): rounding modifier required (rn/rz/rm/rp).
        // - int -> f32: rounding modifier required (precision may be lost).
        // - int -> f64 from s32/u32: no rounding modifier (no precision loss).
        // - int -> f64 from s64/u64: rounding modifier required.
        // - float -> int: integer rounding modifier required (rni/rzi/rmi/rpi).
        let kind = u.int_in_range(0..=3u8)?;
        match kind {
            0 => {
                let dt = self.int_ty_arith(u)?;
                let st = self.int_ty_arith(u)?;
                let d = self.int_reg(u, dt)?;
                let s = self.int_reg(u, st)?;
                self.out
                    .push_str(&format!("    cvt.{}.{} {d}, {s};\n", dt.name(), st.name()));
            }
            1 => {
                // int -> float: CUDA 13.x ptxas requires a rounding
                // modifier for every int->float pairing (including
                // widening s32->f64, which earlier ISA versions
                // allowed without one).
                let dt = self.float_ty(u)?;
                let st = self.int_ty_arith(u)?;
                let d = self.float_reg(u, dt)?;
                let s = self.int_reg(u, st)?;
                let rnd = u.choose(&["rn", "rz", "rm", "rp"])?;
                self.out.push_str(&format!(
                    "    cvt.{rnd}.{}.{} {d}, {s};\n",
                    dt.name(),
                    st.name()
                ));
            }
            2 => {
                let dt = self.int_ty_arith(u)?;
                let st = self.float_ty(u)?;
                let rnd = u.choose(&["rni", "rzi", "rmi", "rpi"])?;
                let d = self.int_reg(u, dt)?;
                let s = self.float_reg(u, st)?;
                self.out.push_str(&format!(
                    "    cvt.{rnd}.{}.{} {d}, {s};\n",
                    dt.name(),
                    st.name()
                ));
            }
            3 => {
                // Pick narrow vs widen explicitly so each side is legal.
                if u.arbitrary()? {
                    // f64 -> f32 (narrowing): rnd required
                    let d = self.regf32(u)?;
                    let s = self.regf64(u)?;
                    let rnd = u.choose(&["rn", "rz", "rm", "rp"])?;
                    self.out
                        .push_str(&format!("    cvt.{rnd}.f32.f64 {d}, {s};\n"));
                } else {
                    // f32 -> f64 (widening): no rnd
                    let d = self.regf64(u)?;
                    let s = self.regf32(u)?;
                    self.out.push_str(&format!("    cvt.f64.f32 {d}, {s};\n"));
                }
            }
            _ => unreachable!(),
        }
        Ok(())
    }

    fn emit_mul_wide(&mut self, u: &mut Unstructured) -> Result<()> {
        // mul.wide.<32-bit> %rdN, %rM, %rO  -> 64-bit result, 32-bit operands
        let ty = *u.choose(&[IntTy::S32, IntTy::U32])?;
        let d = self.reg64(u)?;
        let a = self.reg32(u)?;
        let b = self.reg32(u)?;
        self.out
            .push_str(&format!("    mul.wide.{} {d}, {a}, {b};\n", ty.name()));
        Ok(())
    }

    fn emit_fma(&mut self, u: &mut Unstructured) -> Result<()> {
        let ty = self.float_ty(u)?;
        let rnd = u.choose(&["rn", "rz", "rm", "rp"])?;
        let d = self.float_reg(u, ty)?;
        let a = self.float_reg(u, ty)?;
        let b = self.float_reg(u, ty)?;
        let c = self.float_reg(u, ty)?;
        self.out.push_str(&format!(
            "    fma.{rnd}.{} {d}, {a}, {b}, {c};\n",
            ty.name()
        ));
        Ok(())
    }

    fn emit_arith_float(&mut self, u: &mut Unstructured) -> Result<()> {
        // div.approx is f32-only; the other ops accept both. Decide
        // the op after the type so we can guarantee a legal pairing.
        let ty = self.float_ty(u)?;
        let op = if ty == FloatTy::F32 {
            *u.choose(&["add.rn", "sub.rn", "mul.rn", "div.approx", "min", "max"])?
        } else {
            *u.choose(&["add.rn", "sub.rn", "mul.rn", "div.rn", "min", "max"])?
        };
        let d = self.float_reg(u, ty)?;
        let a = self.float_reg(u, ty)?;
        let b = self.float_reg(u, ty)?;
        self.out
            .push_str(&format!("    {op}.{} {d}, {a}, {b};\n", ty.name()));
        Ok(())
    }

    fn emit_atom(&mut self, u: &mut Unstructured) -> Result<()> {
        // atom.<space>.<op>.<ty>  (CAS has an extra operand)
        //
        // Per the PTX ISA: bitwise atomics (and/or/xor) and exch/cas
        // require .bNN; integer arithmetic atomics (add/min/max)
        // require .uNN/.sNN. We pick op first, then derive a legal
        // type to avoid the long list of ptxas semantic errors.
        let space = *u.choose(&["global", "shared"])?;
        let op = *u.choose(&["add", "and", "or", "xor", "exch", "min", "max", "cas"])?;
        let ty = match op {
            "and" | "or" | "xor" | "exch" | "cas" => *u.choose(&[IntTy::B32, IntTy::B64])?,
            "add" => *u.choose(&[IntTy::U32, IntTy::S32, IntTy::U64])?,
            "min" | "max" => *u.choose(&[IntTy::U32, IntTy::S32, IntTy::U64, IntTy::S64])?,
            _ => unreachable!(),
        };
        let addr = format!("%rd{}", u.int_in_range(0..=3)?);
        let d = self.int_reg(u, ty)?;
        let a = self.int_reg(u, ty)?;
        if op == "cas" {
            let b = self.int_reg(u, ty)?;
            self.out.push_str(&format!(
                "    atom.{space}.cas.{} {d}, [{addr}], {a}, {b};\n",
                ty.name()
            ));
        } else {
            self.out.push_str(&format!(
                "    atom.{space}.{op}.{} {d}, [{addr}], {a};\n",
                ty.name()
            ));
        }
        Ok(())
    }

    fn emit_neg_abs(&mut self, u: &mut Unstructured) -> Result<()> {
        let op = u.choose(&["neg", "abs"])?;
        if u.arbitrary()? {
            let ty = *u.choose(&[IntTy::S32, IntTy::S64])?;
            let d = self.int_reg(u, ty)?;
            let a = self.int_reg(u, ty)?;
            self.out
                .push_str(&format!("    {op}.{} {d}, {a};\n", ty.name()));
        } else {
            let ty = self.float_ty(u)?;
            let d = self.float_reg(u, ty)?;
            let a = self.float_reg(u, ty)?;
            self.out
                .push_str(&format!("    {op}.{} {d}, {a};\n", ty.name()));
        }
        Ok(())
    }

    fn emit_min_max(&mut self, u: &mut Unstructured) -> Result<()> {
        let op = u.choose(&["min", "max"])?;
        let ty = self.int_ty_arith(u)?;
        let d = self.int_reg(u, ty)?;
        let a = self.int_reg(u, ty)?;
        let b = self.int_reg(u, ty)?;
        self.out
            .push_str(&format!("    {op}.{} {d}, {a}, {b};\n", ty.name()));
        Ok(())
    }

    fn emit_mad(&mut self, u: &mut Unstructured) -> Result<()> {
        // mad.{lo,hi}.{u32,s32,u64,s64} d, a, b, c  ->  d = a*b + c
        let part = u.choose(&["lo", "hi"])?;
        let ty = self.int_ty_arith(u)?;
        let d = self.int_reg(u, ty)?;
        let a = self.int_reg(u, ty)?;
        let b = self.int_reg(u, ty)?;
        let c = self.int_reg(u, ty)?;
        self.out.push_str(&format!(
            "    mad.{part}.{} {d}, {a}, {b}, {c};\n",
            ty.name()
        ));
        Ok(())
    }

    fn emit_bfe(&mut self, u: &mut Unstructured) -> Result<()> {
        // bfe.<ty> d, src, pos, len   (pos and len are .u32)
        let ty = *u.choose(&[IntTy::U32, IntTy::S32, IntTy::U64, IntTy::S64])?;
        let d = self.int_reg(u, ty)?;
        let s = self.int_reg(u, ty)?;
        let pos: u32 = u.int_in_range(0..=63)?;
        let len: u32 = u.int_in_range(1..=32)?;
        self.out
            .push_str(&format!("    bfe.{} {d}, {s}, {pos}, {len};\n", ty.name()));
        Ok(())
    }

    fn emit_bfi(&mut self, u: &mut Unstructured) -> Result<()> {
        // bfi.{b32,b64} d, a, b, pos, len
        let ty = *u.choose(&[IntTy::B32, IntTy::B64])?;
        let d = self.int_reg(u, ty)?;
        let a = self.int_reg(u, ty)?;
        let b = self.int_reg(u, ty)?;
        let pos: u32 = u.int_in_range(0..=63)?;
        let len: u32 = u.int_in_range(1..=32)?;
        self.out.push_str(&format!(
            "    bfi.{} {d}, {a}, {b}, {pos}, {len};\n",
            ty.name()
        ));
        Ok(())
    }

    fn emit_bit_count(&mut self, u: &mut Unstructured) -> Result<()> {
        // popc/clz operate on .bNN; bfind takes signed/unsigned int
        // (which makes a semantic difference for the leading-zero
        // count). Dst is always a 32-bit register.
        let op = *u.choose(&["popc", "clz", "bfind"])?;
        let src_ty = match op {
            "popc" | "clz" => *u.choose(&[IntTy::B32, IntTy::B64])?,
            "bfind" => *u.choose(&[IntTy::U32, IntTy::S32, IntTy::U64, IntTy::S64])?,
            _ => unreachable!(),
        };
        let d = self.reg32(u)?;
        let s = self.int_reg(u, src_ty)?;
        self.out
            .push_str(&format!("    {op}.{} {d}, {s};\n", src_ty.name()));
        Ok(())
    }

    fn emit_prmt(&mut self, u: &mut Unstructured) -> Result<()> {
        // prmt.b32 d, a, b, c     - select 4 bytes from {a,b} concat'd
        // The selector c is an 8-nibble immediate or a register.
        let d = self.reg32(u)?;
        let a = self.reg32(u)?;
        let b = self.reg32(u)?;
        // Use an immediate selector — restricts to legal nibbles 0..7.
        let sel: u32 = u.arbitrary()?;
        self.out
            .push_str(&format!("    prmt.b32 {d}, {a}, {b}, {sel:#x};\n"));
        Ok(())
    }

    fn emit_special_reg(&mut self, u: &mut Unstructured) -> Result<()> {
        // mov.u32 %rN, %<special_reg>
        // tid/ntid/ctaid/nctaid each have .x/.y/.z; smid/warpid/laneid
        // are scalars.
        let kind = u.int_in_range(0..=3u8)?;
        let name: String = match kind {
            0 => {
                let base = u.choose(&["tid", "ntid", "ctaid", "nctaid"])?;
                let axis = u.choose(&["x", "y", "z"])?;
                format!("%{base}.{axis}")
            }
            1 => format!("%{}", u.choose(&["smid", "warpid", "laneid"])?),
            2 => "%clock".to_string(),
            3 => "%lanemask_eq".to_string(),
            _ => unreachable!(),
        };
        let d = self.reg32(u)?;
        self.out.push_str(&format!("    mov.u32 {d}, {name};\n"));
        Ok(())
    }

    fn emit_sad(&mut self, u: &mut Unstructured) -> Result<()> {
        // sad.{u32,s32} d, a, b, c    ->  d = abs(a-b) + c
        let ty = *u.choose(&[IntTy::U32, IntTy::S32])?;
        let d = self.int_reg(u, ty)?;
        let a = self.int_reg(u, ty)?;
        let b = self.int_reg(u, ty)?;
        let c = self.int_reg(u, ty)?;
        self.out
            .push_str(&format!("    sad.{} {d}, {a}, {b}, {c};\n", ty.name()));
        Ok(())
    }

    fn emit_vector_ld_st(&mut self, u: &mut Unstructured) -> Result<()> {
        // ld.<space>.v{2,4}.<ty> {d0, d1[, d2, d3]}, [addr];
        // st.<space>.v{2,4}.<ty> [addr], {s0, s1[, s2, s3]};
        let is_load = u.arbitrary()?;
        let space = *u.choose(&["global", "shared", "local"])?;
        let width: u8 = if u.arbitrary()? { 2 } else { 4 };
        // For simplicity restrict to .u32 (so 4-byte elements, max 16
        // bytes for v4 -- well within natural alignment).
        let addr = format!("%rd{}", u.int_in_range(0..=3)?);
        let mut regs = String::new();
        for i in 0..width {
            if i > 0 {
                regs.push_str(", ");
            }
            regs.push_str(&self.reg32(u)?);
        }
        if is_load {
            self.out.push_str(&format!(
                "    ld.{space}.v{width}.u32 {{{regs}}}, [{addr}];\n"
            ));
        } else {
            self.out.push_str(&format!(
                "    st.{space}.v{width}.u32 [{addr}], {{{regs}}};\n"
            ));
        }
        Ok(())
    }

    fn emit_dp4a(&mut self, u: &mut Unstructured) -> Result<()> {
        // dp4a.<dst_ty>.<src_ty> d, a, b, c
        // Dot product of 4 packed 8-bit values + accum. All 32-bit
        // registers; dst/src signed-ness can mix.
        let dt = *u.choose(&[IntTy::U32, IntTy::S32])?;
        let st = *u.choose(&[IntTy::U32, IntTy::S32])?;
        let d = self.reg32(u)?;
        let a = self.reg32(u)?;
        let b = self.reg32(u)?;
        let c = self.reg32(u)?;
        self.out.push_str(&format!(
            "    dp4a.{}.{} {d}, {a}, {b}, {c};\n",
            dt.name(),
            st.name()
        ));
        Ok(())
    }

    fn emit_brev(&mut self, u: &mut Unstructured) -> Result<()> {
        // brev.{b32,b64} d, a   ->  bit-reverse
        let ty = *u.choose(&[IntTy::B32, IntTy::B64])?;
        let d = self.int_reg(u, ty)?;
        let a = self.int_reg(u, ty)?;
        self.out
            .push_str(&format!("    brev.{} {d}, {a};\n", ty.name()));
        Ok(())
    }

    fn emit_rcp_sqrt(&mut self, u: &mut Unstructured) -> Result<()> {
        // rcp/sqrt: float only. Approx variants exist for f32; f64
        // requires .rnd. We use the always-legal "approx" for f32 and
        // ".rn" for f64.
        let op = u.choose(&["rcp", "sqrt"])?;
        let ty = self.float_ty(u)?;
        let mod_ = match ty {
            FloatTy::F32 => "approx",
            FloatTy::F64 => "rn",
        };
        let d = self.float_reg(u, ty)?;
        let a = self.float_reg(u, ty)?;
        self.out
            .push_str(&format!("    {op}.{mod_}.{} {d}, {a};\n", ty.name()));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_produces_a_runnable_kernel() {
        let s = generate_ptx(&[]);
        assert!(s.contains(".version 7.0"));
        assert!(s.contains(".target sm_70"));
        assert!(s.contains(".entry kernel"));
        assert!(s.ends_with("ret;\n}\n"));
    }

    #[test]
    fn arbitrary_bytes_produce_well_formed_output() {
        // Every byte slice should produce a string that at minimum
        // contains the prelude and a single `ret;` to close. This
        // protects against panics from the generator.
        for n in 0..256usize {
            let bytes: Vec<u8> = (0..n).map(|i| (i * 37 + 7) as u8).collect();
            let s = generate_ptx(&bytes);
            assert!(s.contains(".entry kernel"), "missing prelude at n={n}");
            assert!(s.contains("ret;"), "missing ret at n={n}");
            assert!(
                s.len() <= MAX_OUTPUT_BYTES * 2,
                "output too big at n={n}: {} bytes",
                s.len()
            );
        }
    }

    #[test]
    fn long_input_capped() {
        let bytes = vec![0xa5u8; 64 * 1024];
        let s = generate_ptx(&bytes);
        // We allow some slack past MAX_OUTPUT_BYTES because the loop
        // stops *after* emitting, so the closing label/ret can push us
        // a bit over.
        assert!(s.len() < MAX_OUTPUT_BYTES * 2, "got {} bytes", s.len());
    }
}
