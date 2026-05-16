// CUDA inline-PTX variant of the m042-vsub4-else-ifconvert-fold ptxas reproducer.
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
constexpr uint32_t kInput0 = 0x4bbb416fu;
constexpr uint32_t kInput1 = 0xe9f2bb28u;
constexpr uint32_t kInput2 = 0x882a34e1u;
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
        ".reg .pred  p<21>;\n\t"
        ".reg .b32   r<22>;\n\t"
        ".reg .b64   rd<8>;\n\t"
        "\n\t"
        "mov.u64 rd0, %0;\n\t"
        "mov.u64 rd1, %1;\n\t"
        "mov.u32 r0, %2;\n\t"
        "mov.u32         r20, %%tid.x;\n\t"
        "cvta.to.global.u64 rd2, rd0;\n\t"
        "mul.wide.u32    rd3, r20, 4;\n\t"
        "add.s64         rd2, rd2, rd3;\n\t"
        "ld.global.u32   r2, [rd2];\n\t"
        "mov.u32         r1, r20;\n\t"
        "mov.u32         r3, r0;\n\t"
        "mov.u32         r4, 4;\n\t"
        "mov.u32         r5, r20;\n\t"
        "mov.u32         r6, r2;\n\t"
        "mov.u32         r7, r0;\n\t"
        "mov.u32         r8, 8;\n\t"
        "mov.u32         r9, r20;\n\t"
        "mov.u32         r10, r2;\n\t"
        "mov.u32         r11, r0;\n\t"
        "mov.u32         r12, 12;\n\t"
        "mov.u32         r13, r20;\n\t"
        "mov.u32         r14, r2;\n\t"
        "mov.u32         r15, r0;\n\t"
        "mov.u32         r16, 16;\n\t"
        "mov.u32         r17, r20;\n\t"
        "mov.u32         r18, r2;\n\t"
        "mov.u32         r19, r0;\n\t"
        "\n\t"
        "setp.ne.u32   p5, 2, r17;\n\t"
        "@p5 bra   structured_if_1_then;\n\t"
        "bra             structured_if_1_else;\n\t"
        "structured_if_1_then:\n\t"
        "bra             structured_if_1_done;\n\t"
        "structured_if_1_else:\n\t"
        "vsub4.u32.u32.u32        r4, r15, r17, r9;\n\t"
        "mad.lo.u32    r1, r4, r2, 46474;\n\t"
        "structured_if_1_done:\n\t"
        "\n\t"
        "exit:\n\t"
        "cvta.to.global.u64 rd4, rd1;\n\t"
        "mul.wide.u32    rd5, r20, 16;\n\t"
        "add.s64         rd4, rd4, rd5;\n\t"
        "st.global.u32   [rd4 + 4], r1;\n\t"
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
    h_in[1] = kInput1;
    h_in[2] = kInput2;

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

    std::printf("threads=%d n=%u x=0x%08x input0=0x%08x input1=0x%08x input2=0x%08x\n", kThreads, kN, kX, kInput0, kInput1, kInput2);
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
