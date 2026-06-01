; -1 << 15 with nsw: shifted-out bits all = sign bit => NOT poison, = 0x8000 = -32768
define i16 @a() { %s = shl nsw i16 -1, 15  ret i16 %s }
; 1 << 15 with nsw: shifting a 1 into the sign position, top bit changes => poison
define i16 @b() { %s = shl nsw i16 1, 15   ret i16 %s }
; 3 << 15 with nsw: shifted-out bits include a 1 that disagrees with sign => poison
define i16 @c() { %s = shl nsw i16 3, 15   ret i16 %s }
