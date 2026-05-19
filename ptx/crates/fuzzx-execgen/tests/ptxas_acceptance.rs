//! Generate many programs and verify ptxas accepts every one at both -O0 and
//! -O3. Generator bugs that emit syntactically-invalid PTX, or that produce
//! something one opt level rejects, show up here long before they pollute the
//! divergence inbox of the real fuzzer.

use fuzzx_exec::compile;
use fuzzx_execgen::{bytes_from_seed, generate_from_bytes, TARGET_ARCH};

const PROGRAM_BYTES: usize = 4096;

#[test]
fn ptxas_accepts_warp_size_constant_at_both_opt_levels() {
    let arch_flag = format!("-arch={TARGET_ARCH}");
    let ptx = format!(
        r#".version 8.8
.target {TARGET_ARCH}
.address_size 64

.visible .entry warp_size_smoke(
    .param .u64 out_ptr
)
{{
    .reg .b32 %r<1>;
    .reg .b64 %rd<1>;

    ld.param.u64 %rd0, [out_ptr];
    mov.u32 %r0, WARP_SZ;
    st.global.u32 [%rd0], %r0;
    ret;
}}
"#
    );

    for opt in ["-O0", "-O3"] {
        compile(&ptx, &[arch_flag.as_str(), opt]).unwrap();
    }
}

#[test]
fn ptxas_accepts_membar_at_both_opt_levels() {
    let arch_flag = format!("-arch={TARGET_ARCH}");
    let ptx = format!(
        r#".version 8.8
.target {TARGET_ARCH}
.address_size 64

.visible .entry membar_smoke(
    .param .u64 out_ptr
)
{{
    .reg .b32 %r<1>;
    .reg .b64 %rd<1>;

    ld.param.u64 %rd0, [out_ptr];
    membar.cta ;
    membar.gl  ;
    membar.sys ;
    mov.u32 %r0, 1;
    st.global.u32 [%rd0], %r0;
    ret;
}}
"#
    );

    for opt in ["-O0", "-O3"] {
        compile(&ptx, &[arch_flag.as_str(), opt]).unwrap();
    }
}

#[test]
fn ptxas_accepts_random_programs_at_both_opt_levels() {
    let arch_flag = format!("-arch={TARGET_ARCH}");
    let mut failures: Vec<(u64, String, String)> = Vec::new();

    for seed in 0u64..50 {
        let bytes = bytes_from_seed(seed, PROGRAM_BYTES);
        let ptx = match generate_from_bytes(&bytes) {
            Ok(p) => p,
            Err(e) => {
                failures.push((seed, "generator".into(), format!("{e}")));
                continue;
            }
        };
        for opt in ["-O0", "-O3"] {
            if let Err(e) = compile(&ptx, &[arch_flag.as_str(), opt]) {
                failures.push((seed, opt.into(), format!("{e}\n--- ptx ---\n{ptx}")));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} ptxas rejections out of 50 seeds (showing first 3):\n{}",
        failures.len(),
        failures
            .iter()
            .take(3)
            .map(|(seed, opt, msg)| format!("seed={seed} opt={opt}:\n{msg}"))
            .collect::<Vec<_>>()
            .join("\n\n=====\n\n"),
    );
}
