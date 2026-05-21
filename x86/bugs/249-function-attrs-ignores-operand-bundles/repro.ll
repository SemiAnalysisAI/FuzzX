declare void @leaf() nofree nosync nounwind willreturn

define void @caller() {
  call void @leaf() [ "side_effects"() ]
  ret void
}
