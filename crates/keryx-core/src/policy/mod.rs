//! Stage 1 — the mapping policy (architecture §3, R3; spec §21.3): computed in **Rust**
//! (keryx invokes no solver, R4), a pure, deterministic, unique function from the
//! de-sugared [`Schema`] to the [`Mapping`] — name assignment and qualification, presence
//! classification, treatment selection, and reserved-word escapes. The optional ASP
//! co-artifact and its cross-check (spec §21.3) wait for the estate's own
//! elenctic-on-themelios (below the D1 solve boundary); `explain` renders the `Mapping`
//! directly for inspection meanwhile. Submodules: `model` (the mapping model), `names`
//! (un-collided assignment), `qualify` (the injectivity optimization + escapes).
//!
//! [`Schema`]: crate::descriptor::model::Schema
//! [`Mapping`]: model::Mapping

pub mod model;

pub use model::{
    EmitForm, EnumMapping, EnumValueMapping, FieldMapping, Mapping, ScalarTreatment, SortMapping,
    Totality, Unit, ValueMapping, ViewKind,
};
