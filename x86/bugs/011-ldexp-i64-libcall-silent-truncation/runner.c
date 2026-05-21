#include <stdio.h>
#include <stdint.h>

double t(double a, int64_t e);

int main(void) {
    /* e = 0x80000004 = 2147483652 — a huge positive exponent. */
    int64_t e = (int64_t)2147483652LL;
    double  r = t(1.0, e);
    printf("ldexp(1.0, %lld) -> %.17g (expected +Inf; libcall reads only low 32 bits)\n",
           (long long)e, r);
    /* The low 32 bits of 0x80000004 are interpreted as int = INT_MIN+4
       (very negative), so ldexp returns 0.0. */
    if (r == 0.0) { puts("FAIL — i64 exponent silently truncated to 32 bits"); return 1; }
    puts("OK");
    return 0;
}
