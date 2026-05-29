define float @snan_round_trip(float %x) {
  %ext = fpext float %x to double
  %trunc = fptrunc double %ext to float
  ret float %trunc
}
