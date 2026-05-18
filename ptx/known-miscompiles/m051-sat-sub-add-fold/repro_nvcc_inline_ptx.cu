// CUDA inline-PTX variant of the m051-sat-sub-add-fold ptxas reproducer.
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
// Verified on 2026-05-18 with CUDA Toolkit 13.0 nvcc/ptxas
// (`release 13.0, V13.0.88`, build `cuda_13.0.r13.0/compiler.36424714_0`).

#include <cuda_runtime.h>

#include <cstdint>
#include <cstdio>
#include <cstdlib>

constexpr int kThreads = 32;
constexpr int kInputWords = 32;
constexpr int kOutputWords = 128;
constexpr uint32_t kSentinel = 0xa5a5a5a5u;

static void check(cudaError_t err, const char* what) {
    if (err != cudaSuccess) {
        std::fprintf(stderr, "%s: %s\n", what, cudaGetErrorString(err));
        std::exit(2);
    }
}

__global__ void repro_kernel(const uint32_t* in, uint32_t* out) {
    asm volatile(
        "{\n\t"
        ".reg .b32 r<9>;\n\t"
        ".reg .b64 rd<8>;\n\t"
        "mov.u64 rd0, %0;\n\t"
        "mov.u64 rd1, %1;\n\t"
        "mov.u32 r8, %%tid.x;\n\t"
        "cvta.to.global.u64 rd2, rd0;\n\t"
        "mul.wide.u32 rd3, r8, 4;\n\t"
        "add.s64 rd2, rd2, rd3;\n\t"
        "ld.global.u32 r2, [rd2];\n\t"
        "mov.u32 r5, r8;\n\t"
        "mov.u32 r6, r2;\n\t"
        "add.u32 r5, r5, r6;\n\t"
        "add.u32 r2, r5, r6;\n\t"
        "sub.sat.s32 r4, r2, r6;\n\t"
        "add.u32 r1, r4, r6;\n\t"
        "cvta.to.global.u64 rd4, rd1;\n\t"
        "mul.wide.u32 rd5, r8, 16;\n\t"
        "add.s64 rd4, rd4, rd5;\n\t"
        "st.global.u32 [rd4 + 4], r1;\n\t"
        "}\n"
        :
        : "l"(in), "l"(out)
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
    uint32_t h_in[kInputWords] = {
        0x28267ecdu, 0xc65df886u, 0x6495723fu, 0x02ccebf8u,
        0xa10465b1u, 0x3f3bdf6au, 0xdd735923u, 0x7baad2dcu,
        0x19e24c95u, 0xb819c64eu, 0x56514007u, 0xf488b9c0u,
        0x92c03379u, 0x30f7ad32u, 0xcf2f26ebu, 0x6d66a0a4u,
        0x0b9e1a5du, 0xa9d59416u, 0x480d0dcfu, 0xe6448788u,
        0x847c0141u, 0x22b37afau, 0xc0eaf4b3u, 0x5f226e6cu,
        0xfd59e825u, 0x9b9161deu, 0x39c8db97u, 0xd8005550u,
        0x7637cf09u, 0x146f48c2u, 0xb2a6c27bu, 0x50de3c34u,
    };
    uint32_t h_out[kOutputWords];
    for (int i = 0; i < kOutputWords; ++i) {
        h_out[i] = kSentinel;
    }

    uint32_t* d_in = nullptr;
    uint32_t* d_out = nullptr;
    check(cudaMalloc(&d_in, sizeof(h_in)), "cudaMalloc input");
    check(cudaMalloc(&d_out, sizeof(h_out)), "cudaMalloc output");
    check(cudaMemcpy(d_in, h_in, sizeof(h_in), cudaMemcpyHostToDevice), "cudaMemcpy input");
    check(cudaMemcpy(d_out, h_out, sizeof(h_out), cudaMemcpyHostToDevice), "cudaMemcpy output sentinel");

    repro_kernel<<<1, kThreads>>>(d_in, d_out);
    check(cudaGetLastError(), "repro_kernel launch");
    check(cudaDeviceSynchronize(), "cudaDeviceSynchronize");
    check(cudaMemcpy(h_out, d_out, sizeof(h_out), cudaMemcpyDeviceToHost), "cudaMemcpy output");
    check(cudaFree(d_out), "cudaFree output");
    check(cudaFree(d_in), "cudaFree input");

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
