// CUDA inline-PTX variant of the m046-cnot-underflow-ugt-fold ptxas reproducer.
//
// Build this same CUDA source twice and compare the printed output from the
// -O0 and -O2 binaries:
//
//   nvcc -std=c++17 -O2 -arch=sm_103 -Xptxas -O0 \
//     repro_nvcc_inline_ptx.cu -o repro_nvcc_inline_ptx_o0
//
//   nvcc -std=c++17 -O2 -arch=sm_103 -Xptxas -O2 \
//     repro_nvcc_inline_ptx.cu -o repro_nvcc_inline_ptx_o2
//
// Verified on 2026-05-17 with CUDA Toolkit 13.2.1 nvcc/ptxas
// (`release 13.2, V13.2.78`, build `cuda_13.2.r13.2/compiler.37668154_0`).

#include <cuda_runtime.h>

#include <cstdint>
#include <cstdio>
#include <cstdlib>

constexpr int kThreads = 32;
constexpr int kOutputWords = 128;
constexpr uint32_t kSentinel = 0xa5a5a5a5u;

static void check(cudaError_t err, const char* what) {
    if (err != cudaSuccess) {
        std::fprintf(stderr, "%s: %s\n", what, cudaGetErrorString(err));
        std::exit(2);
    }
}

__global__ void repro_kernel(uint32_t* out) {
    asm volatile(
        "{\n\t"
        ".reg .pred p<1>;\n\t"
        ".reg .b32 r<6>;\n\t"
        ".reg .b64 rd<3>;\n\t"
        "mov.u64 rd0, %0;\n\t"
        "mov.u32 r0, %%tid.x;\n\t"
        "\n\t"
        "and.b32       r1, 32, r0;\n\t"
        "cnot.b32      r2, r1;\n\t"
        "add.u32       r3, r2, r0;\n\t"
        "sub.u32       r4, r0, r3;\n\t"
        "setp.gt.u32   p0, 0, r4;\n\t"
        "selp.b32      r5, 52761, 0, p0;\n\t"
        "\n\t"
        "mul.wide.u32  rd2, r0, 16;\n\t"
        "add.s64       rd1, rd0, rd2;\n\t"
        "st.global.u32 [rd1], r5;\n\t"
        "}\n"
        :
        : "l"(out)
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
    uint32_t h_out[kOutputWords];
    for (int i = 0; i < kOutputWords; ++i) {
        h_out[i] = kSentinel;
    }

    uint32_t* d_out = nullptr;
    check(cudaMalloc(&d_out, sizeof(h_out)), "cudaMalloc output");
    check(cudaMemcpy(d_out, h_out, sizeof(h_out), cudaMemcpyHostToDevice), "cudaMemcpy output sentinel");

    repro_kernel<<<1, kThreads>>>(d_out);
    check(cudaGetLastError(), "repro_kernel launch");
    check(cudaDeviceSynchronize(), "cudaDeviceSynchronize");
    check(cudaMemcpy(h_out, d_out, sizeof(h_out), cudaMemcpyDeviceToHost), "cudaMemcpy output");
    check(cudaFree(d_out), "cudaFree output");

    std::printf("threads=%d\n", kThreads);
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
