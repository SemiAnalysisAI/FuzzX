target datalayout = "e-p:4:8"

@g1 = external global ptr
@g2 = external global ptr
@g3 = external global ptr
@g4 = external global ptr
@g5 = external global ptr
@g6 = external global ptr
@g7 = external global ptr
@g8 = external global ptr
@g9 = external global ptr
@g10 = external global ptr
@g11 = external global ptr
@g12 = external global ptr
@g13 = external global ptr
@g14 = external global ptr
@g15 = external global ptr

declare noalias nonnull ptr @alloc(i8) allockind("alloc,uninitialized") allocsize(0)

define i1 @noalias_allocation_multi_compare() {
  %p = call noalias nonnull ptr @alloc(i8 1)
  %l1 = load volatile ptr, ptr @g1, !nonnull !0
  %c1 = icmp eq ptr %p, %l1
  %l2 = load volatile ptr, ptr @g2, !nonnull !0
  %c2 = icmp eq ptr %p, %l2
  %o2 = or i1 %c1, %c2
  %l3 = load volatile ptr, ptr @g3, !nonnull !0
  %c3 = icmp eq ptr %p, %l3
  %o3 = or i1 %o2, %c3
  %l4 = load volatile ptr, ptr @g4, !nonnull !0
  %c4 = icmp eq ptr %p, %l4
  %o4 = or i1 %o3, %c4
  %l5 = load volatile ptr, ptr @g5, !nonnull !0
  %c5 = icmp eq ptr %p, %l5
  %o5 = or i1 %o4, %c5
  %l6 = load volatile ptr, ptr @g6, !nonnull !0
  %c6 = icmp eq ptr %p, %l6
  %o6 = or i1 %o5, %c6
  %l7 = load volatile ptr, ptr @g7, !nonnull !0
  %c7 = icmp eq ptr %p, %l7
  %o7 = or i1 %o6, %c7
  %l8 = load volatile ptr, ptr @g8, !nonnull !0
  %c8 = icmp eq ptr %p, %l8
  %o8 = or i1 %o7, %c8
  %l9 = load volatile ptr, ptr @g9, !nonnull !0
  %c9 = icmp eq ptr %p, %l9
  %o9 = or i1 %o8, %c9
  %l10 = load volatile ptr, ptr @g10, !nonnull !0
  %c10 = icmp eq ptr %p, %l10
  %o10 = or i1 %o9, %c10
  %l11 = load volatile ptr, ptr @g11, !nonnull !0
  %c11 = icmp eq ptr %p, %l11
  %o11 = or i1 %o10, %c11
  %l12 = load volatile ptr, ptr @g12, !nonnull !0
  %c12 = icmp eq ptr %p, %l12
  %o12 = or i1 %o11, %c12
  %l13 = load volatile ptr, ptr @g13, !nonnull !0
  %c13 = icmp eq ptr %p, %l13
  %o13 = or i1 %o12, %c13
  %l14 = load volatile ptr, ptr @g14, !nonnull !0
  %c14 = icmp eq ptr %p, %l14
  %o14 = or i1 %o13, %c14
  %l15 = load volatile ptr, ptr @g15, !nonnull !0
  %c15 = icmp eq ptr %p, %l15
  %o15 = or i1 %o14, %c15
  ret i1 %o15
}

!0 = !{}
