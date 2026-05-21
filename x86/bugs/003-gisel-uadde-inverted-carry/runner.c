#include <stdio.h>
#include <stdint.h>

__int128 add128(__int128 a, __int128 b);

int main(void) {
    /* a = 0x...0000_FFFF_FFFF_FFFF_FFFF, b = 1
       expected: low overflows to 0, high incremented by 1 */
    __int128 a = (__int128)0xFFFFFFFFFFFFFFFFULL;
    __int128 b = 1;
    __int128 r = add128(a, b);
    uint64_t lo = (uint64_t)r;
    uint64_t hi = (uint64_t)(r >> 64);
    printf("add128(0xFFFF_FFFF_FFFF_FFFF, 1) = (hi=0x%016llx lo=0x%016llx)\n",
           (unsigned long long)hi, (unsigned long long)lo);
    printf("expected:                       (hi=0x0000000000000001 lo=0x0000000000000000)\n");
    if (hi == 1 && lo == 0) { puts("OK"); return 0; }
    puts("FAIL — multi-word add carried wrong amount (GISel uadde carry-in bug)");
    return 1;
}
