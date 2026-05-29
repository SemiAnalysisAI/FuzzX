#include <stdint.h>
#include <stdio.h>

uint64_t minimumnum_x_qnan(double x);
uint32_t minimumnum_f32_x_qnan(float x);

int main(void) {
    /* sNaN bit patterns (top fraction bit clear, payload non-zero). */
    union { uint64_t u; double d; } sNaN_d = { .u = 0x7FF0000000000001ULL };
    union { uint32_t u; float  f; } sNaN_f = { .u = 0x7F800001U };

    uint64_t r64 = minimumnum_x_qnan(sNaN_d.d);
    uint32_t r32 = minimumnum_f32_x_qnan(sNaN_f.f);

    printf("input  sNaN double : 0x%016llx\n", (unsigned long long)sNaN_d.u);
    printf("result        f64  : 0x%016llx (want: top bits 0x7FF8...; got 0x7FF0... = sNaN)\n",
           (unsigned long long)r64);
    printf("input  sNaN float  : 0x%08x\n", sNaN_f.u);
    printf("result        f32  : 0x%08x  (want: 0x7FC00001; got 0x7F800001 = sNaN)\n", r32);

    int bad = 0;
    /* expected quiet bit (bit 51 in f64, bit 22 in f32) */
    if ((r64 & 0x0008000000000000ULL) == 0) { puts("FAIL f64: sNaN survived"); bad = 1; }
    if ((r32 & 0x00400000U) == 0)            { puts("FAIL f32: sNaN survived"); bad = 1; }
    return bad;
}
