//! The inbound codec (architecture §5, inbound; spec §11, §22): a payload, decoded against the
//! pool its schema came from and lowered under the mapping model and a root constant, to ground
//! facts — `Symbol`s for the library seam and, rendered from them, a `.lp` fact module for the
//! CLI seam (R6), identical in content (§11) and ground by construction (P10). [`Codec`] is built once per schema and shreds
//! any number of payloads; [`Facts`] is one payload's result; [`PayloadFormat`] and [`Root`] are
//! the surface's value types. Beneath the surface: the decode engine's adapter (`engine`, the one
//! place in the codec that names prost-reflect), the textproto pre-parse depth guard (`guard`,
//! bounding a text payload's nesting ahead of the engine's unbounded text parser), the §6 scalar
//! policy (`scalar`), and the managed-stack walk with its referent index (`walk`). No engine type
//! crosses the surface: a caller names a root *type* by proto name, and the descriptor it
//! resolves to stays inside.

pub(crate) mod engine;
pub(crate) mod guard;
pub(crate) mod scalar;
pub(crate) mod walk;

use std::path::Path;

use themelios_program::prelude::*;
use themelios_program::render::render as render_ast;

use crate::descriptor::model::Schema;
use crate::descriptor::{self, RetainedPool};
use crate::diagnostics::{Diagnostic, DiagnosticKind, Diagnostics, Locus};
use crate::policy::{self, Mapping};
use crate::terms;
use walk::Index;

/// The wire form a payload arrives in. A format joins this enum together with the decode
/// that lowers it, so every variant the surface admits is one the codec shreds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PayloadFormat {
    /// The protobuf binary wire format (a `.binpb` payload).
    Binary,
    /// The protobuf text format (a `.txtpb` payload): UTF-8 text, its message nesting bounded
    /// before the engine's parser sees it and parsed on a thread keryx sizes for that bound — one
    /// thread spawn per payload, the one cost the binary form does not pay (spec §26).
    Textproto,
}

/// The root constant of one payload (spec §4.1 item 6): the only extrinsic identity in the
/// system — a caller-supplied constant, or a fresh `r{n}` minted per invocation — from
/// which every occupant term and fact beneath it is derived.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Root(Name);

impl Root {
    /// A root over a caller-supplied constant — the library seam's choice of root identity
    /// (spec §4.1 item 6).
    #[must_use]
    pub fn named(name: Name) -> Root {
        Root(name)
    }

    /// The fresh root constant `r{n}` — `r0`, `r1`, … one per invocation, the CLI's choice
    /// (spec §4.1 item 6). `r` followed by decimal digits always lexes as an identifier, so
    /// the `expect` is discharged (§6).
    #[must_use]
    pub fn fresh(n: usize) -> Root {
        Root(Name::new(format!("r{n}")).expect("keryx vocabulary is a valid identifier"))
    }

    /// The root as a ground constant term — the occupant every top-level fact of the
    /// payload is over — built through `terms::apply` like every applied term keryx
    /// builds, so it is the collapsed leaf `terms::atom_symbol` relies on.
    pub(crate) fn term(&self) -> Term {
        terms::apply(self.0.clone(), Vec::new())
    }
}

/// The reusable inbound codec: one schema's descriptor pool, its mapping, and the referent index
/// over that mapping, built once — the descriptor set is decoded once per schema, never per
/// payload — and shredding any number of payloads against them. The pool is the very one the
/// schema was walked from, so a payload is decoded against exactly the descriptors its mapping
/// describes, and the two cannot disagree. Cost: construction is the descriptor door's cost plus
/// the policy's; [`shred`](Codec::shred) is linear in the payload's size times its nesting depth
/// — every fact carries its full path term (spec §4.1) — with the depth bounded by the uniform
/// ceiling, and the decoded tree held once, borrowed by the walk.
pub struct Codec {
    pool: RetainedPool,
    mapping: Mapping,
    index: Index,
}

impl Codec {
    /// A codec over a serialized `FileDescriptorSet` — the descriptor door (spec §20): the set
    /// ingested and retained, its schema mapped (§21.3), and the mapping indexed for the walk.
    ///
    /// # Errors
    ///
    /// The descriptor door's diagnostics (`descriptor::ingest`, and `MalformedDescriptor` for a
    /// malformed map entry anywhere in the pool — a payload may be decoded against any message it
    /// declares), then the policy's (`policy::map`), then `UnmappableName` should the mapping's
    /// closed world not hold (a keryx error, checked rather than assumed).
    pub fn new(descriptor_set: &[u8]) -> Result<Codec, Diagnostics> {
        let (schema, pool) = descriptor::ingest_retaining(descriptor_set)?;
        Codec::over(&schema, pool)
    }

    /// A codec over `.proto` source — the source door (spec §20): the files compiled against the
    /// include roots (and keryx's own option registry) to a descriptor set, then as [`new`].
    ///
    /// # Errors
    ///
    /// The source door's diagnostics (`descriptor::compile`), then as [`new`].
    ///
    /// [`new`]: Codec::new
    pub fn from_source(
        files: &[impl AsRef<Path>],
        includes: &[impl AsRef<Path>],
    ) -> Result<Codec, Diagnostics> {
        let (schema, pool) = descriptor::source::compile_retaining(files, includes)?;
        Codec::over(&schema, pool)
    }

    /// The codec over an ingested schema and the pool it was walked from: the policy's mapping,
    /// indexed.
    fn over(schema: &Schema, pool: RetainedPool) -> Result<Codec, Diagnostics> {
        let mapping = policy::map(schema)?;
        let index = Index::build(&mapping)?;
        Ok(Codec {
            pool,
            mapping,
            index,
        })
    }

    /// The mapping model the codec shreds under (spec §21.3) — the vocabulary its facts are over.
    #[must_use]
    pub fn mapping(&self) -> &Mapping {
        &self.mapping
    }

    /// Shred one payload (spec §11): decode it, as an instance of the message `root_type` names,
    /// from the wire form `format`, and lower the tree to the facts hanging from `root`.
    /// `root_type` is a fully-qualified proto path or a short name exactly one message bears; it
    /// is resolved here, once — the library is the one type-resolution site. Every fact or every
    /// diagnosis, never a partial shred beside a diagnosis (§6).
    ///
    /// # Errors
    ///
    /// `UnknownRootType` for a name resolving to no message, or to more than one (the payload is
    /// then never decoded); `UndecodablePayload` for bytes that do not decode as that message
    /// (over-deep binary included; a textproto payload that is not UTF-8, or does not parse);
    /// `DependencyFault` for a contained engine fault; the walk's diagnoses — the §6 refusals at
    /// their fields' paths (`ValueOutOfRange`, `InteriorNul`, `UnrepresentableText`,
    /// `UnannotatedFloat`) and `UnknownEnumValue` (§7.4); and `PayloadTooDeep` past the uniform
    /// nesting ceiling (§8, §26) — the walk's refusal, or for textproto the pre-parse guard's,
    /// ahead of the engine's parser.
    pub fn shred(
        &self,
        root_type: &str,
        payload: &[u8],
        format: PayloadFormat,
        root: &Root,
    ) -> Result<Facts, Diagnostics> {
        let sort = self.index.root(&self.mapping, root_type)?;
        let proto = sort.in_mapping(&self.mapping).proto();
        // A sort of the mapping is a message of the pool: the mapping is walked from the schema
        // this very pool was ingested into (`over`), so the lookup cannot miss — the one-pool
        // invariant the walk's own can't-happens rest on. Discharged loud, as they are: a miss is
        // a keryx error, never a diagnosis under the caller's `UnknownRootType`, which would
        // blame the caller's argument for a bug in keryx.
        let Some(descriptor) = self.pool.message_by_name(proto.as_str()) else {
            unreachable!(
                "`{}` is a sort of the mapping but the descriptor pool declares no such message; the mapping is walked from this pool, so the miss is a keryx error",
                proto.as_str()
            )
        };
        let decoded = match format {
            PayloadFormat::Binary => engine::decode_binary(&descriptor, payload)?,
            PayloadFormat::Textproto => engine::decode_textproto(&descriptor, payload)?,
        };
        walk::shred(
            &self.mapping,
            &self.index,
            root.term(),
            decoded.root(),
            sort,
        )
    }
}

/// The facts of one shredded payload (spec §11), held once: the ground [`Symbol`]s a consuming
/// tool feeds its solver directly (the library seam — no text between, R6) are the one model of
/// the payload's facts, each built as one `(predicate, arguments)` at the walk's one emit site.
/// The `.lp` fact module of the CLI seam is a view of that model, rendered from the symbols by
/// [`render`](Facts::render), so the two seams cannot disagree — there is one structure, and
/// nothing beside it to differ from. Cost: the facts' memory, steady-state; the rendering's
/// program and text are a transient of the render alone. Two deterministic orders of the same
/// facts: [`symbols`](Facts::symbols) in `Symbol::Ord`, the rendering in themelios's statement
/// order — a function of the facts' content, whatever order the statements are derived in.
#[derive(Clone, Debug)]
pub struct Facts {
    symbols: Vec<Symbol>,
}

impl Facts {
    /// The facts as ground symbols, in `Symbol::Ord` — the library seam (spec §11): identical
    /// payload, identical symbols.
    #[must_use]
    pub fn symbols(&self) -> &[Symbol] {
        &self.symbols
    }

    /// The facts as a clingo-dialect `.lp` fact module — the CLI seam (spec §11): one fact per
    /// line, in themelios's canonical statement order, each spelled once. A view of
    /// [`symbols`](Facts::symbols): each symbol's statement is derived from it (`terms::fact_of`)
    /// and the program they make rendered canonically.
    ///
    /// # Errors
    ///
    /// `UnrenderableFacts` should themelios be unable to spell a fact — a genuine can't-happen,
    /// the §6 policy having refused every value the dialect cannot spell before it was built into
    /// a fact (`UnrepresentableText`, `InteriorNul`); total rather than a panic, as the descriptor
    /// facts' rendering is.
    pub fn render(&self) -> Result<String, Diagnostics> {
        render_ast(
            &Program::of(self.symbols.iter().map(terms::fact_of)),
            Dialect::Clingo,
        )
        .map_err(|unspellable| {
            Diagnostics::from(Diagnostic::new(
                DiagnosticKind::UnrenderableFacts,
                Locus::whole(),
                format!("{unspellable}"),
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use themelios_program::render::render;

    // The fresh root is the constant `r{n}` (spec §4.1 item 6): the value a caller naming
    // `r0` supplies, the constant term the literal door builds for it, and a bare constant
    // in a fact over it.
    #[test]
    fn a_fresh_root_is_the_constant_r_n() {
        let root = Root::fresh(0);
        assert_eq!(
            root,
            Root::named(Name::new("r0").expect("`r0` is an identifier"))
        );
        assert_eq!(root.term(), terms::constant("r0"));
        assert_eq!(Root::fresh(12).term(), terms::constant("r12"));
        let batch = terms::fact_named(
            Name::new("reading_batch").expect("an identifier"),
            vec![root.term()],
        );
        assert_eq!(
            render(&Program::of([batch]), Dialect::Clingo).expect("renders"),
            "reading_batch(r0).\n"
        );
    }

    // The one-pool invariant, pinned from the other side: a codec assembled over one schema's
    // mapping and another schema's pool breaks it, and the miss at the root's lookup is a keryx
    // error, loud — never reported under the caller's `UnknownRootType`, which would blame the
    // caller's argument for a bug in keryx. No public door assembles such a codec: `new` and
    // `from_source` ingest the pool the mapping is walked from.
    #[test]
    #[should_panic(expected = "declares no such message")]
    fn a_sort_the_pool_does_not_declare_is_a_keryx_error_not_a_usage_error() {
        let (schema, _) =
            descriptor::ingest_retaining(&keryx_test_support::compile_fixture("proto3.proto"))
                .expect("the fixture ingests");
        let (_, other_pool) =
            descriptor::ingest_retaining(&keryx_test_support::compile_fixture("maps.proto"))
                .expect("the fixture ingests");
        let codec = Codec::over(&schema, other_pool).expect("the mapping indexes");
        let _ = codec.shred("Reading", &[], PayloadFormat::Binary, &Root::fresh(0));
    }
}
