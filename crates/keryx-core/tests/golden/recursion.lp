doc("keryx.rec.A", "A mutual A <-> B containment cycle.").
doc("keryx.rec.Tree", "Self-recursive through a repeated field of its own type.").
field("keryx.rec.A", 1, "b", msg("keryx.rec.B"), explicit, singular).
field("keryx.rec.B", 1, "a", msg("keryx.rec.A"), explicit, singular).
field("keryx.rec.Tree", 1, "label", string, implicit, singular).
field("keryx.rec.Tree", 2, "children", msg("keryx.rec.Tree"), implicit, repeated).
file("recursion.proto", "keryx.rec").
message("keryx.rec.A", "recursion.proto").
message("keryx.rec.B", "recursion.proto").
message("keryx.rec.Tree", "recursion.proto").
recursive("keryx.rec.A").
recursive("keryx.rec.B").
recursive("keryx.rec.Tree").
