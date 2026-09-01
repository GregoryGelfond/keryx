doc("keryx.opt.Sample", "Exercises every AnnotationValue kind and both option targets (§20).").
enum_t("keryx.opt.Signal", "options.proto", open).
enum_value("keryx.opt.Signal", "SIGNAL_HIGH", 2).
enum_value("keryx.opt.Signal", "SIGNAL_LOW", 1).
enum_value("keryx.opt.Signal", "SIGNAL_UNSPECIFIED", 0).
field("keryx.opt.Sample", 1, "tags", string, implicit, repeated).
field("keryx.opt.Sample", 2, "tick", int64, implicit, singular).
field("keryx.opt.Sample", 3, "ratio", float, implicit, singular).
field("keryx.opt.Sample", 4, "priority", int32, implicit, singular).
file("options.proto", "keryx.opt").
message("keryx.opt.Sample", "options.proto").
opt("keryx.opt.Sample", any_types, "a.B").
opt("keryx.opt.Sample", any_types, "c.D").
opt("keryx.opt.Sample", value, true).
opt("keryx.opt.Sample.priority", default, "3").
opt("keryx.opt.Sample.ratio", scale, 3).
opt("keryx.opt.Sample.tags", set, true).
opt("keryx.opt.Sample.tick", numeric, "NATIVE_CHECKED").
opt("keryx.opt.Signal", zero, "ABSENT").
