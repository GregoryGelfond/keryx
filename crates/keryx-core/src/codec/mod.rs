//! The inbound codec (architecture §5, inbound): a decoded payload lowered, under the
//! mapping model and a root constant, to ground facts — `Symbol`s for the library seam,
//! a `.lp` fact module for the CLI seam (R6), ground by construction (P10). This module
//! carries the surface's value types: the [`PayloadFormat`] a payload arrives in and the
//! [`Root`] its facts hang from; the codec proper — the mapping-guided walk and its facts —
//! builds over them.

use themelios_program::prelude::*;

use crate::terms;

/// The wire form a payload arrives in. A format joins this enum together with the decode
/// that lowers it, so every variant the surface admits is one the codec shreds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PayloadFormat {
    /// The protobuf binary wire format (a `.binpb` payload).
    Binary,
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
    // The codec is this term's production caller and lands with it; until then it is
    // exercised only by this module's own tests, so the expectation is stated for the
    // library build alone (an unfulfilled expectation is itself a lint) and retires when
    // the codec walks a payload from its root.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "no production caller until the codec lands")
    )]
    pub(crate) fn term(&self) -> Term {
        terms::apply(self.0.clone(), Vec::new())
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
}
