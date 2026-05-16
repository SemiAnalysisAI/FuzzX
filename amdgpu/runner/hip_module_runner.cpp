#include <hip/hip_runtime.h>

#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <string>
#include <vector>

namespace {

int usage(const char *argv0) {
    std::fprintf(stderr,
                 "usage: %s <kernel.hsaco> <input.bin> <output.bin> <output_n> [device] [input_n] [kernel]\n",
                 argv0);
    return 2;
}

bool hip_ok(hipError_t err, const char *expr) {
    if (err == hipSuccess) {
        return true;
    }
    std::fprintf(stderr, "%s failed: %s\n", expr, hipGetErrorString(err));
    return false;
}

#define HIP_CHECK(expr)          \
    do {                         \
        if (!hip_ok((expr), #expr)) { \
            return 1;            \
        }                        \
    } while (0)

bool read_u32_file(const std::string &path, std::vector<std::uint32_t> &data, std::size_t n) {
    std::ifstream in(path, std::ios::binary);
    if (!in) {
        std::fprintf(stderr, "failed to open input %s\n", path.c_str());
        return false;
    }
    data.resize(n);
    in.read(reinterpret_cast<char *>(data.data()), static_cast<std::streamsize>(n * sizeof(std::uint32_t)));
    if (in.gcount() != static_cast<std::streamsize>(n * sizeof(std::uint32_t))) {
        std::fprintf(stderr, "input %s is shorter than %zu u32 values\n", path.c_str(), n);
        return false;
    }
    return true;
}

bool write_u32_file(const std::string &path, const std::vector<std::uint32_t> &data) {
    std::ofstream out(path, std::ios::binary);
    if (!out) {
        std::fprintf(stderr, "failed to open output %s\n", path.c_str());
        return false;
    }
    out.write(reinterpret_cast<const char *>(data.data()),
              static_cast<std::streamsize>(data.size() * sizeof(std::uint32_t)));
    return static_cast<bool>(out);
}

} // namespace

int main(int argc, char **argv) {
    if (argc != 5 && argc != 6 && argc != 7 && argc != 8) {
        return usage(argv[0]);
    }

    const char *hsaco_path = argv[1];
    const std::string input_path = argv[2];
    const std::string output_path = argv[3];
    const auto output_n_long = std::strtol(argv[4], nullptr, 10);
    if (output_n_long <= 0) {
        std::fprintf(stderr, "output_n must be positive\n");
        return 2;
    }
    const std::uint32_t output_n = static_cast<std::uint32_t>(output_n_long);
    const int device = argc >= 6 ? std::atoi(argv[5]) : 0;
    const auto input_n_long = argc >= 7 ? std::strtol(argv[6], nullptr, 10) : output_n_long;
    const char *kernel_name = argc == 8 ? argv[7] : "fuzz_kernel";
    if (input_n_long <= 0) {
        std::fprintf(stderr, "input_n must be positive\n");
        return 2;
    }
    const std::uint32_t input_n = static_cast<std::uint32_t>(input_n_long);

    std::vector<std::uint32_t> host_in;
    if (!read_u32_file(input_path, host_in, input_n)) {
        return 1;
    }
    std::vector<std::uint32_t> host_out(output_n, 0);

    std::uint32_t *dev_in = nullptr;
    std::uint32_t *dev_out = nullptr;
    hipModule_t module = nullptr;
    hipFunction_t kernel = nullptr;

    HIP_CHECK(hipSetDevice(device));
    HIP_CHECK(hipMalloc(&dev_in, input_n * sizeof(std::uint32_t)));
    HIP_CHECK(hipMalloc(&dev_out, output_n * sizeof(std::uint32_t)));
    HIP_CHECK(hipMemcpy(dev_in, host_in.data(), input_n * sizeof(std::uint32_t), hipMemcpyHostToDevice));
    HIP_CHECK(hipMemset(dev_out, 0, output_n * sizeof(std::uint32_t)));
    HIP_CHECK(hipModuleLoad(&module, hsaco_path));
    HIP_CHECK(hipModuleGetFunction(&kernel, module, kernel_name));

    void *args[] = {&dev_in, &dev_out, const_cast<std::uint32_t *>(&output_n)};
    constexpr unsigned threads_per_block = 256;
    const unsigned blocks = (output_n + threads_per_block - 1) / threads_per_block;
    HIP_CHECK(hipModuleLaunchKernel(kernel, blocks, 1, 1, threads_per_block, 1, 1, 0, nullptr, args, nullptr));
    HIP_CHECK(hipDeviceSynchronize());
    HIP_CHECK(hipMemcpy(host_out.data(), dev_out, output_n * sizeof(std::uint32_t), hipMemcpyDeviceToHost));

    bool ok = write_u32_file(output_path, host_out);
    if (module) {
        (void)hipModuleUnload(module);
    }
    if (dev_in) {
        (void)hipFree(dev_in);
    }
    if (dev_out) {
        (void)hipFree(dev_out);
    }
    return ok ? 0 : 1;
}
