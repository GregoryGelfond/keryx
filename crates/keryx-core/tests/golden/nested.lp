enum_t("keryx.nest.Outer.Kind", "nested.proto", open).
enum_value("keryx.nest.Outer.Kind", "KIND_A", 1).
enum_value("keryx.nest.Outer.Kind", "KIND_UNSPECIFIED", 0).
field("keryx.nest.Outer", 1, "inner", msg("keryx.nest.Outer.Inner"), explicit, singular).
field("keryx.nest.Outer", 2, "kind", enum("keryx.nest.Outer.Kind"), implicit, singular).
field("keryx.nest.Outer.Inner", 1, "label", string, implicit, singular).
file("nested.proto", "keryx.nest").
message("keryx.nest.Outer", "nested.proto").
message("keryx.nest.Outer.Inner", "nested.proto").
nested("keryx.nest.Outer.Inner", "keryx.nest.Outer").
nested("keryx.nest.Outer.Kind", "keryx.nest.Outer").
