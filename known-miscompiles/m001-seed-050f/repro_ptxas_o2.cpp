// Standalone ptxas -O2 miscompile reproducer.
//
// This file embeds a 24-line PTX kernel, assembles it twice with ptxas
// (-O0 and -O2), runs both cubins through the CUDA Driver API, and compares
// their output against the scalar PTX trace.  It does not use an input buffer;
// the only kernel parameter is the output pointer.
//
// Build, typical x86 CUDA install:
//   g++ -std=c++17 -O2 repro_ptxas_o2.cpp \
//     -I/usr/local/cuda/include -L/usr/local/cuda/lib64/stubs -lcuda \
//     -o repro_ptxas_o2
//
// Build, CUDA SBSA install like this machine:
//   g++ -std=c++17 -O2 repro_ptxas_o2.cpp \
//     -I/usr/local/cuda/targets/sbsa-linux/include \
//     -L/usr/local/cuda/targets/sbsa-linux/lib/stubs -lcuda \
//     -o repro_ptxas_o2
//
// Run:
//   ./repro_ptxas_o2 [sm_XX]
//
// The program returns 1 when the ptxas bug is reproduced: -O0 matches the
// scalar trace, but -O2 does not.
//
// Optional:
//   PTXAS=/path/to/ptxas ./repro_ptxas_o2 sm_103
//
// Correct scalar behavior for the embedded PTX:
//   The kernel launches two threads and stores one u32 per thread.  Each
//   thread computes out + tid * 4, initializes r0 = 1 as a loop flag, and
//   initializes r1 = 1 as the value that would be wrong to store.
//
//   Thread 0 branches directly to block_5 because tid == 0.  block_5 sees
//   r0 != 0, sets r0 = 0, and branches to block_2.  block_2 sets r1 = 0.  The
//   next block_5 visit sees r0 == 0 and exits, so thread 0 must store 0.
//
//   Thread 1 falls through block_2 before reaching block_5, so it also sets
//   r1 = 0 before any possible exit.  It must store 0 as well.
//
// Observed bug with CUDA 13.0 ptxas V13.0.88 on sm_103 and with CUDA 13.2
// Update 1 ptxas V13.2.78 on sm_103:
//   -O0 output: [0, 0], which matches the scalar trace.
//   -O2 output: [1, 0], which is wrong for tid 0.  This is as if ptxas kept
//   the pre-loop r1 value and lost the required block_2 assignment r1 = 0.
//
// The CUDA 13.2 Update 1 test used ptxas extracted from
// cuda-nvcc-13-2_13.2.78-1_arm64.deb from CUDA Toolkit 13.2.1.

#include <cuda.h>

#include <cerrno>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <iostream>
#include <sstream>
#include <stdexcept>
#include <string>
#include <sys/wait.h>
#include <unistd.h>
#include <vector>

constexpr int N_THREADS = 2;
constexpr int OUTPUT_BYTES = N_THREADS * 4;

static const char* kPtx = R"PTX(
.version 7.0
.target sm_80
.address_size 64
.entry fuzz_kernel(.param .u64 out_ptr) {
    .reg .pred  %p0;
    .reg .b32   %r0, %r1;
    .reg .b64   %rd0;
    ld.param.u64    %rd0, [out_ptr];
    mov.u32         %r0, %tid.x;
    mad.wide.u32    %rd0, %r0, 4, %rd0;
    setp.eq.u32   %p0, %r0, 0;
    mov.u32         %r0, 1;
    mov.u32       %r1, 1;
    @%p0 bra   block_5;
block_2:
    mov.u32       %r1, 0;
block_5:
    setp.eq.u32   %p0, %r0, 0;
    @%p0 bra   done;
    mov.u32         %r0, 0;
    bra             block_2;
done:
    st.global.u32   [%rd0], %r1;
    ret;
}
)PTX";

static void check(CUresult r, const char* op) {
    if (r == CUDA_SUCCESS) {
        return;
    }
    const char* msg = nullptr;
    cuGetErrorString(r, &msg);
    std::ostringstream os;
    os << op << " failed: " << (msg ? msg : "unknown CUDA error");
    throw std::runtime_error(os.str());
}

struct TempDir {
    std::string path;
    explicit TempDir(const char* pattern) {
        std::vector<char> buf(pattern, pattern + std::strlen(pattern) + 1);
        char* p = mkdtemp(buf.data());
        if (!p) {
            throw std::runtime_error(std::string("mkdtemp failed: ") + std::strerror(errno));
        }
        path = p;
    }
    ~TempDir() {
        unlink((path + "/in.ptx").c_str());
        unlink((path + "/out.cubin").c_str());
        rmdir(path.c_str());
    }
};

static void write_text(const std::string& path, const char* text) {
    std::ofstream f(path);
    if (!f) {
        throw std::runtime_error("failed to open " + path);
    }
    f << text;
}

static std::vector<char> read_binary(const std::string& path) {
    std::ifstream f(path, std::ios::binary);
    if (!f) {
        throw std::runtime_error("failed to open " + path);
    }
    return std::vector<char>(std::istreambuf_iterator<char>(f),
                             std::istreambuf_iterator<char>());
}

static std::vector<char> compile_ptx(const std::string& ptxas,
                                     const std::string& arch,
                                     const char* opt) {
    TempDir dir("/tmp/ptxas_o2_repro.XXXXXX");
    const std::string ptx_path = dir.path + "/in.ptx";
    const std::string cubin_path = dir.path + "/out.cubin";
    write_text(ptx_path, kPtx);

    pid_t pid = fork();
    if (pid < 0) {
        throw std::runtime_error(std::string("fork failed: ") + std::strerror(errno));
    }
    if (pid == 0) {
        std::string arch_arg = "-arch=" + arch;
        execlp(ptxas.c_str(), ptxas.c_str(), arch_arg.c_str(), opt,
               "-o", cubin_path.c_str(), ptx_path.c_str(), static_cast<char*>(nullptr));
        std::perror("execlp ptxas");
        _exit(127);
    }

    int status = 0;
    if (waitpid(pid, &status, 0) < 0) {
        throw std::runtime_error(std::string("waitpid failed: ") + std::strerror(errno));
    }
    if (!WIFEXITED(status) || WEXITSTATUS(status) != 0) {
        std::ostringstream os;
        os << "ptxas " << opt << " failed with status " << status;
        throw std::runtime_error(os.str());
    }
    return read_binary(cubin_path);
}

static void create_context(CUcontext* ctx, CUdevice dev) {
#if CUDA_VERSION >= 13000
    check(cuCtxCreate(ctx, nullptr, 0, dev), "cuCtxCreate");
#else
    check(cuCtxCreate(ctx, 0, dev), "cuCtxCreate");
#endif
}

static std::vector<uint32_t> run_kernel(const std::vector<char>& cubin) {
    CUmodule module = nullptr;
    CUfunction fn = nullptr;
    CUdeviceptr d_out = 0;
    std::vector<uint32_t> out(N_THREADS, 0);

    check(cuModuleLoadData(&module, cubin.data()), "cuModuleLoadData");
    check(cuModuleGetFunction(&fn, module, "fuzz_kernel"), "cuModuleGetFunction");
    check(cuMemAlloc(&d_out, OUTPUT_BYTES), "cuMemAlloc output");
    check(cuMemsetD8(d_out, 0xa5, OUTPUT_BYTES), "cuMemsetD8 output");

    void* params[] = { &d_out };
    check(cuLaunchKernel(fn, 1, 1, 1, N_THREADS, 1, 1, 0, nullptr, params, nullptr),
          "cuLaunchKernel");
    check(cuCtxSynchronize(), "cuCtxSynchronize");
    check(cuMemcpyDtoH(out.data(), d_out, OUTPUT_BYTES), "cuMemcpyDtoH output");

    cuMemFree(d_out);
    cuModuleUnload(module);
    return out;
}

static int report(const char* label, const std::vector<uint32_t>& out) {
    int mismatches = 0;
    std::cout << "=== " << label << " ===\n";
    for (uint32_t tid = 0; tid < N_THREADS; ++tid) {
        uint32_t got = out[tid];
        constexpr uint32_t expect = 0;
        bool ok = got == expect;
        mismatches += ok ? 0 : 1;
        std::printf("tid %u: out got 0x%08x expected 0x%08x%s\n",
                    tid, got, expect, ok ? "" : "  MISMATCH");
    }
    std::cout << label << " mismatches: " << mismatches << "\n\n";
    return mismatches;
}

int main(int argc, char** argv) {
    try {
        const char* env_ptxas = std::getenv("PTXAS");
        std::string ptxas = env_ptxas ? env_ptxas : "/usr/local/cuda/bin/ptxas";
        if (access(ptxas.c_str(), X_OK) != 0) {
            ptxas = "ptxas";
        }
        std::string arch = (argc >= 2) ? argv[1] : "sm_103";

        std::cout << "Using ptxas: " << ptxas << "\n";
        std::cout << "Using arch:  " << arch << "\n\n";

        auto cubin_o0 = compile_ptx(ptxas, arch, "-O0");
        auto cubin_o2 = compile_ptx(ptxas, arch, "-O2");

        check(cuInit(0), "cuInit");
        CUdevice dev = 0;
        CUcontext ctx = nullptr;
        check(cuDeviceGet(&dev, 0), "cuDeviceGet");
        create_context(&ctx, dev);

        auto out_o0 = run_kernel(cubin_o0);
        auto out_o2 = run_kernel(cubin_o2);

        int bad_o0 = report("-O0", out_o0);
        int bad_o2 = report("-O2", out_o2);

        cuCtxDestroy(ctx);

        if (bad_o0 == 0 && bad_o2 != 0) {
            std::cout << "REPRODUCED: -O0 matches the scalar PTX trace, but -O2 is wrong.\n";
            return 1;
        }
        if (bad_o0 != 0) {
            std::cout << "Unexpected: -O0 did not match the scalar PTX trace.\n";
            return 2;
        }
        std::cout << "Not reproduced: -O2 matched the scalar PTX trace on this setup.\n";
        return 0;
    } catch (const std::exception& e) {
        std::cerr << "error: " << e.what() << "\n";
        return 2;
    }
}
