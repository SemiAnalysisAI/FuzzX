use ptx_fuzz_exec::{compile, Cuda};
use ptx_fuzz_execgen::{output_len, KERNEL_NAME, N_THREADS, TARGET_ARCH};

fn main() -> anyhow::Result<()> {
    let ptx_path = std::env::args()
        .nth(1)
        .expect("usage: inspect_outputs <ptx> <input.bin>");
    let in_path = std::env::args()
        .nth(2)
        .expect("usage: inspect_outputs <ptx> <input.bin>");
    let ptx = std::fs::read_to_string(&ptx_path)?;
    let input = std::fs::read(&in_path)?;
    let cuda = Cuda::init(0)?;
    let arch = format!("-arch={TARGET_ARCH}");
    for opt in ["-O0", "-O3"] {
        let cubin = compile(&ptx, &[arch.as_str(), opt])?;
        let out = cuda.launch(
            &cubin,
            KERNEL_NAME,
            (1, 1, 1),
            (N_THREADS, 1, 1),
            &input,
            output_len(),
            N_THREADS,
        )?;
        println!("=== {opt} ===");
        for tid in 0..N_THREADS as usize {
            // print each of 4 output slots
            let s = (0..4)
                .map(|k| {
                    let off = tid * 16 + k * 4;
                    u32::from_ne_bytes(out[off..off + 4].try_into().unwrap())
                })
                .collect::<Vec<_>>();
            println!(
                "  tid {tid:2}: [{:08x} {:08x} {:08x} {:08x}]",
                s[0], s[1], s[2], s[3]
            );
        }
    }
    Ok(())
}
