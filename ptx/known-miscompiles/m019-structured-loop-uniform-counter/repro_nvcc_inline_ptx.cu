// CUDA inline-PTX variant of the m019-structured-loop-uniform-counter ptxas reproducer.
//
// Build this same CUDA source twice and compare the printed output from the -O0 and -O2 binaries:
//
//   nvcc -std=c++17 -O2 -arch=sm_103 -Xptxas -O0 \
//     repro_nvcc_inline_ptx.cu -o repro_nvcc_inline_ptx_o0
//
//   nvcc -std=c++17 -O2 -arch=sm_103 -Xptxas -O2 \
//     repro_nvcc_inline_ptx.cu -o repro_nvcc_inline_ptx_o2
//
// Verified on 2026-05-15 with CUDA Toolkit 13.2.1 nvcc/ptxas
// (`release 13.2, V13.2.78`, build `cuda_13.2.r13.2/compiler.37668154_0`).

#include <cuda_runtime.h>

#include <cstdint>
#include <cstdio>
#include <cstdlib>

constexpr int kThreads = 32;
constexpr int kInputWords = 32;
constexpr int kOutputWords = 128;
constexpr uint32_t kN = 32u;
constexpr uint32_t kX = 0x00000000u;
constexpr uint32_t kInput0 = 0x00000000u;
constexpr uint32_t kSentinel = 0xa5a5a5a5u;

static void check(cudaError_t err, const char* what) {
    if (err != cudaSuccess) {
        std::fprintf(stderr, "%s: %s\n", what, cudaGetErrorString(err));
        std::exit(2);
    }
}

__global__ void repro_kernel(const uint32_t* in, uint32_t* out, uint32_t n, uint32_t x) {
    asm volatile(
        "{\n\t"
        "\n\t"
        ".reg .pred  p<66>;\n\t"
        ".reg .b32   r<46>;\n\t"
        ".reg .b64   rd<6>;\n\t"
        "\n\t"
        "mov.u64 rd1, %1;\n\t"
        "mov.u32         r24, %%tid.x;\n\t"
        "mov.u32         r39, 1;\n\t"
        "mov.u32         r40, 0;\n\t"
        "mov.u32         r41, 0;\n\t"
        "mov.u32         r42, 0;\n\t"
        "mov.u32         r43, 1;\n\t"
        "mov.u32         r44, 1;\n\t"
        "mov.u32         r45, 1;\n\t"
        "\n\t"
        "\n\t"
        "structured_loop_19_header:\n\t"
        "setp.eq.u32   p41, r39, 0;\n\t"
        "@p41 bra   structured_loop_19_done;\n\t"
        "sub.u32         r39, r39, 1;\n\t"
        "setp.eq.u32   p42, r24, 32;\n\t"
        "@p42 bra   structured_if_20_then;\n\t"
        "bra             structured_if_20_else;\n\t"
        "structured_if_20_then:\n\t"
        "structured_loop_21_header:\n\t"
        "setp.eq.u32   p45, r40, 0;\n\t"
        "@p45 bra   structured_loop_21_done;\n\t"
        "sub.u32         r40, r40, 1;\n\t"
        "bra             structured_loop_21_header;\n\t"
        "structured_loop_21_done:\n\t"
        "bra             structured_if_20_done;\n\t"
        "structured_if_20_else:\n\t"
        "bra             structured_if_20_done;\n\t"
        "structured_if_20_done:\n\t"
        "setp.eq.u32   p50, r24, 31;\n\t"
        "@p50 bra   structured_if_22_then;\n\t"
        "bra             structured_if_22_else;\n\t"
        "structured_if_22_then:\n\t"
        "structured_loop_23_header:\n\t"
        "setp.eq.u32   p51, r41, 0;\n\t"
        "@p51 bra   structured_loop_23_done;\n\t"
        "sub.u32         r41, r41, 1;\n\t"
        "bra             structured_loop_23_header;\n\t"
        "structured_loop_23_done:\n\t"
        "bra             structured_if_22_done;\n\t"
        "structured_if_22_else:\n\t"
        "structured_loop_25_header:\n\t"
        "setp.eq.u32   p58, r42, 0;\n\t"
        "@p58 bra   structured_loop_25_done;\n\t"
        "sub.u32         r42, r42, 1;\n\t"
        "setp.eq.u32   p59, r24, 32;\n\t"
        "@p59 bra   structured_if_26_then;\n\t"
        "bra             structured_if_26_else;\n\t"
        "structured_if_26_then:\n\t"
        "bra             structured_if_26_done;\n\t"
        "structured_if_26_else:\n\t"
        "structured_loop_27_header:\n\t"
        "setp.eq.u32   p63, r43, 0;\n\t"
        "@p63 bra   structured_loop_27_done;\n\t"
        "sub.u32         r43, r43, 1;\n\t"
        "bra             structured_loop_27_header;\n\t"
        "structured_loop_27_done:\n\t"
        "bra             structured_if_26_done;\n\t"
        "structured_if_26_done:\n\t"
        "bra             structured_loop_25_header;\n\t"
        "structured_loop_25_done:\n\t"
        "bra             structured_if_22_done;\n\t"
        "structured_if_22_done:\n\t"
        "bra             structured_loop_19_header;\n\t"
        "structured_loop_19_done:\n\t"
        "structured_loop_28_header:\n\t"
        "setp.eq.u32   p64, r44, 0;\n\t"
        "@p64 bra   structured_loop_28_done;\n\t"
        "sub.u32         r44, r44, 1;\n\t"
        "structured_loop_29_header:\n\t"
        "setp.eq.u32   p65, r45, 0;\n\t"
        "@p65 bra   structured_loop_29_done;\n\t"
        "sub.u32         r45, r45, 1;\n\t"
        "or.b32        r1, 1, r24;\n\t"
        "bra             structured_loop_29_header;\n\t"
        "structured_loop_29_done:\n\t"
        "bra             structured_loop_28_header;\n\t"
        "structured_loop_28_done:\n\t"
        "bra             exit;\n\t"
        "\n\t"
        "exit:\n\t"
        "cvta.to.global.u64 rd4, rd1;\n\t"
        "mul.wide.u32    rd5, r24, 16;\n\t"
        "add.s64         rd4, rd4, rd5;\n\t"
        "st.global.u32   [rd4 + 0], r1;\n\t"
        "}\n"
        :
        : "l"(in), "l"(out), "r"(n), "r"(x)
        : "memory");
}

static uint64_t fnv1a(const uint32_t* words, int n) {
    uint64_t h = 1469598103934665603ull;
    for (int i = 0; i < n; ++i) {
        uint32_t v = words[i];
        for (int b = 0; b < 4; ++b) {
            h ^= static_cast<unsigned char>(v >> (8 * b));
            h *= 1099511628211ull;
        }
    }
    return h;
}

int main() {
    uint32_t h_in[kInputWords] = {};
    uint32_t h_out[kOutputWords];
    for (int i = 0; i < kOutputWords; ++i) {
        h_out[i] = kSentinel;
    }
    h_in[0] = kInput0;

    uint32_t* d_in = nullptr;
    uint32_t* d_out = nullptr;
    check(cudaMalloc(&d_in, sizeof(h_in)), "cudaMalloc input");
    check(cudaMalloc(&d_out, sizeof(h_out)), "cudaMalloc output");
    check(cudaMemcpy(d_in, h_in, sizeof(h_in), cudaMemcpyHostToDevice), "cudaMemcpy input");
    check(cudaMemcpy(d_out, h_out, sizeof(h_out), cudaMemcpyHostToDevice), "cudaMemcpy output sentinel");

    repro_kernel<<<1, kThreads>>>(d_in, d_out, kN, kX);
    check(cudaGetLastError(), "repro_kernel launch");
    check(cudaDeviceSynchronize(), "cudaDeviceSynchronize");
    check(cudaMemcpy(h_out, d_out, sizeof(h_out), cudaMemcpyDeviceToHost), "cudaMemcpy output");
    check(cudaFree(d_out), "cudaFree output");
    check(cudaFree(d_in), "cudaFree input");

    std::printf("threads=%d n=%u x=0x%08x input0=0x%08x\n", kThreads, kN, kX, kInput0);
    bool any = false;
    for (int i = 0; i < kOutputWords; ++i) {
        if (h_out[i] != kSentinel) {
            any = true;
            std::printf("out[%d]=0x%08x\n", i, h_out[i]);
        }
    }
    if (!any) {
        std::printf("no output words changed\n");
    }
    std::printf("hash=0x%016llx\n", static_cast<unsigned long long>(fnv1a(h_out, kOutputWords)));
    return 0;
}
