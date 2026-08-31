//! Proves keryx-core builds against and uses themelios's consolidated
//! program surface exactly as the founding design records: construct a
//! `Program`, render it canonically, and round-trip a `Symbol` in both
//! directions of the codec.

use themelios_program::prelude::*;
// `render` is not in the prelude nor a crate-root re-export; name it directly.
use themelios_program::render::render;

#[test]
fn constructs_and_renders_a_fact() {
    let fact = Rule::fact(Atom::new(
        Name::new("p").expect("a valid identifier"),
        [Term::from(1)],
    ));
    let program = Program::of([WithProvenance::constructed(Statement::Rule(fact))]);
    assert_eq!(
        render(&program, Dialect::Clingo).expect("renders"),
        "p(1).\n"
    );
}

#[test]
fn round_trips_scalars_through_symbol() {
    let number: Symbol = 42_i32.to_symbol();
    assert_eq!(number, Symbol::Number(42));
    assert_eq!(i32::from_symbol(&number), Ok(42));

    let text: Symbol = "s".to_symbol();
    assert_eq!(text, Symbol::String("s".to_owned()));
    assert_eq!(String::from_symbol(&text), Ok("s".to_owned()));
}
