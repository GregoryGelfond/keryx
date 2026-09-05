//! The §6 scalar policy (architecture §5, inbound): one decoded scalar [`Datum`] lowered, under
//! its field's [`ScalarTreatment`], to the themelios [`Term`] a payload fact carries — or refused
//! with the [`Diagnostic`] §6 names, at the field's path. Pure and total: a function of its
//! arguments alone, every term built through [`crate::terms`] (so each is the collapsed ground
//! leaf `terms::atom_symbol` rests on), every refusal a value. The walk applies it to each scalar a
//! payload carries — a singular field's value, a sequence's element, a map's value, and a map's
//! *key* alike (spec §7.2: keys map per §6) — and lowers enum values itself, against the referent
//! enum's mapping (§7.4).
//!
//! The one refusal §6 did not name, closed here: a `string` carrying a control character other
//! than `\n` has no spelling in themelios's clingo dialect (its string escapes are `\"`, `\\`, and
//! `\n` alone), so it is refused *before* the value is built into a fact (`UnrepresentableText`),
//! and the two delivery forms (spec §11 — the symbols and the `.lp` text) carry identical content
//! while a rendering `Unspellable` stays a genuine can't-happen. NUL, which §6 names outright,
//! keeps its own kind (`InteriorNul`).

use std::fmt::{self, Write as _};

use themelios_program::prelude::*;

use crate::codec::engine::Datum;
use crate::descriptor::model::Scalar;
use crate::diagnostics::{Diagnostic, DiagnosticKind, Locus};
use crate::policy::model::ScalarTreatment;
use crate::terms;

/// Lower one scalar value to its term under its field's treatment, or refuse it at `at` — the
/// field's path as the walk composes it, the locus every refusal names.
///
/// | treatment | datum | term | refusal |
/// |---|---|---|---|
/// | `Native` | `I32` | the integer | — |
/// | `Native` | `U32` | the integer, when it fits `i32` | `ValueOutOfRange` above `i32::MAX` |
/// | `DecimalString` | `I64`, `U64` | the decimal string | — |
/// | `Bool` | `Bool` | the constant `true` / `false` | — |
/// | `Text` | `Str` | the string | `InteriorNul` on a NUL; `UnrepresentableText` on any other control character but `\n` |
/// | `HexString` | `Bytes` | the lowercase-hex string | — |
/// | `NeedsAnnotation` | `F64` | — | `UnannotatedFloat`, whatever the value |
///
/// The pairing of `treatment` and `datum` is fixed by `scalar`, the field's proto kind, from both
/// sides: the decode gives a datum the kind of its field (`engine::datum`, `engine::zero`), and
/// the policy gives a field the §6 default treatment of its kind
/// (`policy::names::scalar_treatment`) — the walk hands both here from the one
/// `ValueMapping::Scalar { kind, treatment }`. So a pairing outside the table is a keryx error,
/// never foreign input, and is discharged as one: an `unreachable` over the enumerated
/// treatments and datum kinds, checked rather than wildcarded on either axis, so a walk that
/// mis-paired a field fails loudly instead of lowering a value as another kind, and a treatment
/// or datum kind added later fails to compile here. The annotation overrides of Increment 5
/// (`(keryx.numeric)`, `(keryx.scale)`, `(keryx.opaque)`) change a field's treatment and so widen
/// the table; the discharge is restated then. `scalar` also names the proto type in a refusal's
/// detail — `uint32` against `fixed32`, `float` against `double` — which the treatment alone
/// cannot.
///
/// `NeedsAnnotation` reads no value: §6 makes the *field* the error, not the value, so an
/// unannotated `float`/`double` field is refused whenever the walk reaches it, its materialised
/// zero included (§5), and the detail carries §6's two-choice fix-it.
///
/// # Errors
///
/// The refusals of the table, each one `Diagnostic` at `at` — one value, one field; the walk
/// collects across fields.
pub(crate) fn lower(
    scalar: Scalar,
    treatment: ScalarTreatment,
    datum: &Datum<'_>,
    at: &str,
) -> Result<Term, Diagnostic> {
    match (treatment, datum) {
        (ScalarTreatment::Native, Datum::I32(value)) => Ok(terms::int(*value)),
        (ScalarTreatment::Native, Datum::U32(value)) => i32::try_from(*value)
            .map(terms::int)
            .map_err(|_| out_of_range(scalar, *value, at)),
        (ScalarTreatment::DecimalString, Datum::I64(value)) => Ok(decimal(value)),
        (ScalarTreatment::DecimalString, Datum::U64(value)) => Ok(decimal(value)),
        (ScalarTreatment::Bool, Datum::Bool(value)) => {
            Ok(terms::constant(if *value { "true" } else { "false" }))
        }
        (ScalarTreatment::Text, Datum::Str(value)) => text(value, at),
        (ScalarTreatment::HexString, Datum::Bytes(value)) => Ok(terms::text(&hex(value))),
        (ScalarTreatment::NeedsAnnotation, Datum::F64(_)) => Err(unannotated(scalar, at)),
        (
            ScalarTreatment::Native
            | ScalarTreatment::DecimalString
            | ScalarTreatment::NeedsAnnotation
            | ScalarTreatment::Bool
            | ScalarTreatment::Text
            | ScalarTreatment::HexString,
            Datum::I32(_)
            | Datum::I64(_)
            | Datum::U32(_)
            | Datum::U64(_)
            | Datum::F64(_)
            | Datum::Bool(_)
            | Datum::Str(_)
            | Datum::Bytes(_)
            | Datum::Enum(_),
        ) => unreachable!(
            "the datum of a field of kind `{}` is of that kind, which its {treatment:?} treatment pairs with alone; a mis-paired lowering is a keryx error",
            scalar.as_str()
        ),
    }
}

/// The decimal-string constant of a 64-bit integer (§6): its decimal text as a string term —
/// opaque to clingo's arithmetic, faithful past its 32-bit integer width.
fn decimal(value: impl fmt::Display) -> Term {
    terms::text(&value.to_string())
}

/// A `string` value as its string term, or the refusal §6 names. The kind is a function of the
/// value's content, not of its first offending character: `InteriorNul` when a NUL occurs
/// anywhere — §6's named refusal takes precedence, so a consumer keyed on it never misses a NUL
/// behind an earlier tab — else `UnrepresentableText` when any other control character but
/// `\n` does, else the term. The admission set is exactly the clingo dialect's spellable set:
/// the dialect's string rule (themelios grammar §4.4) has three escapes — `\"`, `\\`, `\n` —
/// and no other control character has a spelling, so its renderer refuses by the predicate
/// the policy refuses by, `char::is_control` less `\n`. Pinned in this module's tests:
/// character by character across the control range, and past it at the separator, format,
/// boundary, and astral code points where a printer could plausibly refuse and this one does
/// not.
fn text(value: &str, at: &str) -> Result<Term, Diagnostic> {
    if let Some(offset) = value.chars().position(|character| character == '\0') {
        return Err(interior_nul(offset, at));
    }
    if let Some((offset, character)) = value
        .chars()
        .enumerate()
        .find(|(_, character)| *character != '\n' && character.is_control())
    {
        return Err(unrepresentable(offset, character, at));
    }
    Ok(terms::text(value))
}

/// The lowercase-hex spelling of a `bytes` value (§6), two digits a byte; empty bytes spell as
/// the empty string.
fn hex(bytes: &[u8]) -> String {
    // A slice is at most `isize::MAX` long, so the doubling cannot overflow.
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        // Writing to a `String` cannot fail.
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// A refusal at the field's path.
fn refuse(kind: DiagnosticKind, at: &str, detail: String) -> Diagnostic {
    Diagnostic::new(kind, Locus::at(at), detail)
}

/// `ValueOutOfRange`: a `uint32`/`fixed32` value above `i32::MAX` — refused, never truncated or
/// wrapped — naming the proto type, the value, the bound, and §6's opt-out.
fn out_of_range(scalar: Scalar, value: u32, at: &str) -> Diagnostic {
    refuse(
        DiagnosticKind::ValueOutOfRange,
        at,
        format!(
            "the {} value {value} is above i32::MAX ({}), the widest integer a native clingo term carries; annotate the field `(keryx.numeric) = DECIMAL_STRING` to carry it as a decimal string",
            scalar.as_str(),
            i32::MAX
        ),
    )
}

/// `InteriorNul`: the NUL's character offset — never the value, a payload's text being the
/// adversary's to flood a diagnostic with.
fn interior_nul(offset: usize, at: &str) -> Diagnostic {
    refuse(
        DiagnosticKind::InteriorNul,
        at,
        format!(
            "the string value carries a NUL (`\\0`) at character offset {offset}, where a NUL-terminated boundary downstream would cut it short; §6 refuses the value rather than truncate it"
        ),
    )
}

/// `UnrepresentableText`: the offending character by code point and offset — never the value.
fn unrepresentable(offset: usize, character: char, at: &str) -> Diagnostic {
    refuse(
        DiagnosticKind::UnrepresentableText,
        at,
        format!(
            "the string value carries the control character U+{:04X} at character offset {offset}, which has no spelling in a clingo string (the escapes are `\\\"`, `\\\\`, and `\\n` alone)",
            u32::from(character)
        ),
    )
}

/// `UnannotatedFloat`: the field, not the value, with §6's two-choice fix-it.
fn unannotated(scalar: Scalar, at: &str) -> Diagnostic {
    refuse(
        DiagnosticKind::UnannotatedFloat,
        at,
        format!(
            "a {} field has no default lowering (§6); annotate it `(keryx.scale) = n` to carry a fixed-point integer (the value scaled by 10^n, range-checked) or `(keryx.opaque) = true` to carry a decimal-string constant",
            scalar.as_str()
        ),
    )
}

#[cfg(test)]
mod tests {
    use themelios_program::prelude::*;
    use themelios_program::render::render;

    use super::lower;
    use crate::codec::engine::Datum;
    use crate::descriptor::model::Scalar;
    use crate::diagnostics::{Diagnostic, DiagnosticKind, Locus};
    use crate::policy::model::ScalarTreatment;
    use crate::terms;

    /// The field path every case is lowered at — the locus a refusal must name.
    const AT: &str = "keryx.scalars.Sample.value";

    /// The largest `uint32` a native clingo integer carries: `i32::MAX`, as the wire's `u32`.
    fn top() -> u32 {
        u32::try_from(i32::MAX).expect("i32::MAX is a u32")
    }

    fn lowered(scalar: Scalar, treatment: ScalarTreatment, datum: &Datum<'_>) -> Term {
        lower(scalar, treatment, datum, AT).expect("admitted")
    }

    fn refused(scalar: Scalar, treatment: ScalarTreatment, datum: &Datum<'_>) -> Diagnostic {
        lower(scalar, treatment, datum, AT).expect_err("refused")
    }

    /// One fact over a term, rendered under the clingo dialect — the `.lp` spelling of a term.
    fn spelled(term: Term) -> Result<String, Unspellable> {
        render(
            &Program::of([terms::fact("p", vec![term])]),
            Dialect::Clingo,
        )
    }

    #[test]
    fn the_section_6_table_lowers_each_admitted_value_to_its_term() {
        use ScalarTreatment::{Bool, DecimalString, HexString, Native, Text};
        let table = [
            (Scalar::Int32, Native, Datum::I32(-7), terms::int(-7)),
            (
                Scalar::Sint32,
                Native,
                Datum::I32(i32::MIN),
                terms::int(i32::MIN),
            ),
            (Scalar::Uint32, Native, Datum::U32(0), terms::int(0)),
            (
                Scalar::Fixed32,
                Native,
                Datum::U32(top()),
                terms::int(i32::MAX),
            ),
            (
                Scalar::Int64,
                DecimalString,
                Datum::I64(-9_007_199_254_740_993),
                terms::text("-9007199254740993"),
            ),
            (
                Scalar::Sfixed64,
                DecimalString,
                Datum::I64(i64::MIN),
                terms::text("-9223372036854775808"),
            ),
            (
                Scalar::Uint64,
                DecimalString,
                Datum::U64(u64::MAX),
                terms::text("18446744073709551615"),
            ),
            (
                Scalar::Fixed64,
                DecimalString,
                Datum::U64(0),
                terms::text("0"),
            ),
            (
                Scalar::Bool,
                Bool,
                Datum::Bool(true),
                terms::constant("true"),
            ),
            (
                Scalar::Bool,
                Bool,
                Datum::Bool(false),
                terms::constant("false"),
            ),
            (
                Scalar::String,
                Text,
                Datum::Str("line one\nline \"two\" \\ é 字"),
                terms::text("line one\nline \"two\" \\ é 字"),
            ),
            (Scalar::String, Text, Datum::Str(""), terms::text("")),
            (
                Scalar::Bytes,
                HexString,
                Datum::Bytes(&[0x00, 0xde, 0xad, 0xbe, 0xef, 0x0f]),
                terms::text("00deadbeef0f"),
            ),
            (Scalar::Bytes, HexString, Datum::Bytes(&[]), terms::text("")),
        ];
        for (scalar, treatment, datum, expected) in table {
            let term = lowered(scalar, treatment, &datum);
            assert_eq!(term, expected, "{scalar:?} {treatment:?} {datum:?}");
            // Every lowered term is a collapsed ground leaf — the premise `terms::atom_symbol`'s
            // discharge rests on, which holds because the policy builds only through `terms`.
            assert!(
                matches!(term, Term::Symbolic(_)),
                "{scalar:?} {treatment:?} {datum:?} lowered to an uncollapsed term"
            );
        }
    }

    #[test]
    fn the_section_6_table_refuses_each_named_case_at_the_field() {
        use DiagnosticKind::{InteriorNul, UnannotatedFloat, UnrepresentableText, ValueOutOfRange};
        use ScalarTreatment::{Native, NeedsAnnotation, Text};
        let table = [
            (
                Scalar::Uint32,
                Native,
                Datum::U32(top() + 1),
                ValueOutOfRange,
            ),
            (
                Scalar::Fixed32,
                Native,
                Datum::U32(u32::MAX),
                ValueOutOfRange,
            ),
            (Scalar::String, Text, Datum::Str("a\0b"), InteriorNul),
            (
                Scalar::String,
                Text,
                Datum::Str("a\tb"),
                UnrepresentableText,
            ),
            // A carriage return, an escape sequence (a terminal-injection vector), DEL, and a C1
            // control (NEL): every control but `\n`, whichever block it comes from.
            (
                Scalar::String,
                Text,
                Datum::Str("\r\n"),
                UnrepresentableText,
            ),
            (
                Scalar::String,
                Text,
                Datum::Str("\u{1b}[31m"),
                UnrepresentableText,
            ),
            (
                Scalar::String,
                Text,
                Datum::Str("\u{7f}"),
                UnrepresentableText,
            ),
            (
                Scalar::String,
                Text,
                Datum::Str("\u{85}"),
                UnrepresentableText,
            ),
            (
                Scalar::Float,
                NeedsAnnotation,
                Datum::F64(1.5),
                UnannotatedFloat,
            ),
            // The materialised zero of an unset IMPLICIT field (§5): the field is the error.
            (
                Scalar::Double,
                NeedsAnnotation,
                Datum::F64(0.0),
                UnannotatedFloat,
            ),
        ];
        for (scalar, treatment, datum, kind) in table {
            let diagnostic = refused(scalar, treatment, &datum);
            assert_eq!(
                diagnostic.kind(),
                kind,
                "{scalar:?} {treatment:?} {datum:?}"
            );
            assert_eq!(
                diagnostic.locus(),
                &Locus::at(AT),
                "{scalar:?} {treatment:?} {datum:?}"
            );
        }
    }

    #[test]
    fn a_refusal_names_the_proto_type_and_its_fix_it_and_never_echoes_the_value() {
        // The range refusal names the proto type (the treatment alone cannot tell `uint32` from
        // `fixed32`), the value, the bound by its name and its width, and §6's opt-out.
        let range = refused(
            Scalar::Fixed32,
            ScalarTreatment::Native,
            &Datum::U32(u32::MAX),
        );
        let detail = range.detail();
        assert!(detail.contains("fixed32"), "{detail}");
        assert!(detail.contains("4294967295"), "{detail}");
        assert!(detail.contains("i32::MAX"), "{detail}");
        assert!(detail.contains("2147483647"), "{detail}");
        assert!(
            detail.contains("(keryx.numeric) = DECIMAL_STRING"),
            "{detail}"
        );

        // The float refusal names the proto type and carries the two-choice fix-it (§6).
        for (scalar, name) in [(Scalar::Float, "float"), (Scalar::Double, "double")] {
            let float = refused(scalar, ScalarTreatment::NeedsAnnotation, &Datum::F64(0.0));
            let detail = float.detail();
            assert!(detail.contains(name), "{detail}");
            assert!(detail.contains("(keryx.scale) = n"), "{detail}");
            assert!(detail.contains("(keryx.opaque) = true"), "{detail}");
        }

        // A text refusal locates the character by offset and names it by code point — never by
        // echoing the value, which is the payload's to flood a diagnostic with.
        let marker = "never-echoed";
        let tab = refused(
            Scalar::String,
            ScalarTreatment::Text,
            &Datum::Str(&format!("{marker}\tafter")),
        );
        let detail = tab.detail();
        assert!(!detail.contains(marker), "{detail}");
        assert!(detail.contains("U+0009"), "{detail}");
        assert!(detail.contains("offset 12"), "{detail}");
        let nul = refused(
            Scalar::String,
            ScalarTreatment::Text,
            &Datum::Str(&format!("é{marker}\0")),
        );
        let detail = nul.detail();
        assert!(!detail.contains(marker), "{detail}");
        assert!(detail.contains("offset 13"), "{detail}");
    }

    #[test]
    fn a_nul_anywhere_is_an_interior_nul_ahead_of_any_other_control() {
        // The kind is a function of the value's content, not of its first offending character:
        // §6's named refusal takes precedence, so a consumer keyed on `InteriorNul` never misses a
        // NUL behind an earlier tab.
        let behind_a_tab = refused(Scalar::String, ScalarTreatment::Text, &Datum::Str("\t\0"));
        assert_eq!(behind_a_tab.kind(), DiagnosticKind::InteriorNul);
        let alone = refused(Scalar::String, ScalarTreatment::Text, &Datum::Str("\0"));
        assert_eq!(alone.kind(), DiagnosticKind::InteriorNul);
    }

    /// Whether the policy admits the one-character string `character`, and whether the clingo
    /// dialect spells it — the two sides of the parity the admission set claims.
    fn admitted_and_spelled(character: char) -> (Result<Term, Diagnostic>, bool) {
        let value = character.to_string();
        let admitted = lower(
            Scalar::String,
            ScalarTreatment::Text,
            &Datum::Str(&value),
            AT,
        );
        let spells = spelled(terms::text(&value)).is_ok();
        (admitted, spells)
    }

    #[test]
    fn the_text_admission_set_is_the_clingo_dialect_s_spellable_set() {
        // Every C0 control, printable ASCII (the escaped `"` and `\` included), DEL, every C1
        // control, and the first character past them, each as a one-character string: the policy
        // admits it exactly when the clingo dialect spells it, so a string the policy admits never
        // meets a rendering `Unspellable` — the claim behind refusing here rather than at render,
        // pinned character by character across the whole control range (Unicode's `Cc` ends at
        // U+009F).
        for code in 0..=0xA0 {
            let character = char::from_u32(code).expect("a scalar value below the surrogates");
            let (admitted, spelled) = admitted_and_spelled(character);
            assert_eq!(
                admitted.is_ok(),
                spelled,
                "U+{code:04X}: the policy and the dialect disagree ({admitted:?})"
            );
            if let Err(diagnostic) = admitted {
                let expected = if character == '\0' {
                    DiagnosticKind::InteriorNul
                } else {
                    DiagnosticKind::UnrepresentableText
                };
                assert_eq!(diagnostic.kind(), expected, "U+{code:04X}");
            }
        }

        // Past the control range, the parity rests on the dialect's rule (themelios grammar
        // §4.4: three escapes, and no other *control* character has a spelling — the same
        // `char::is_control` the policy refuses by), and is pinned where a printer could
        // plausibly refuse and this one does not: the soft hyphen, the zero-width space, the
        // line and paragraph separators, the byte-order mark (Unicode's `Cf`/`Zl`/`Zp`, not
        // `Cc`); a private-use character; the replacement character and a noncharacter; the
        // scalars bounding the surrogate gap; and astral-plane scalars up to the last one.
        // Each is admitted by the policy *and* spelled by the dialect, rendered through
        // themelios rather than assumed.
        for code in [
            0x00AD,
            0x200B,
            0x2028,
            0x2029,
            0xD7FF,
            0xE000,
            0xFEFF,
            0xFFFD,
            0xFFFF,
            0x1F600,
            u32::from(char::MAX),
        ] {
            let character = char::from_u32(code).expect("a scalar value");
            assert!(
                !character.is_control(),
                "U+{code:04X} is not a control character"
            );
            let (admitted, spelled) = admitted_and_spelled(character);
            assert!(
                admitted.is_ok(),
                "U+{code:04X}: the policy refuses a non-control character ({admitted:?})"
            );
            assert!(
                spelled,
                "U+{code:04X}: the dialect refuses a character the policy admits"
            );
        }
    }

    #[test]
    fn the_lowered_terms_spell_as_section_6_reads() {
        // The §6 table as `.lp` text: a native integer bare, a 64-bit value and a hex string
        // quoted, the booleans as constants, a string under the dialect's three escapes.
        let arguments = vec![
            lowered(Scalar::Int32, ScalarTreatment::Native, &Datum::I32(-7)),
            lowered(Scalar::Uint32, ScalarTreatment::Native, &Datum::U32(top())),
            lowered(
                Scalar::Int64,
                ScalarTreatment::DecimalString,
                &Datum::I64(-9_007_199_254_740_993),
            ),
            lowered(Scalar::Bool, ScalarTreatment::Bool, &Datum::Bool(true)),
            lowered(
                Scalar::String,
                ScalarTreatment::Text,
                &Datum::Str("a\nb \"c\" \\"),
            ),
            lowered(
                Scalar::Bytes,
                ScalarTreatment::HexString,
                &Datum::Bytes(&[0xde, 0xad]),
            ),
        ];
        let program = Program::of([terms::fact("sample", arguments)]);
        assert_eq!(
            render(&program, Dialect::Clingo).expect("every lowered term spells"),
            "sample(-7, 2147483647, \"-9007199254740993\", true, \"a\\nb \\\"c\\\" \\\\\", \"dead\").\n"
        );
    }

    #[test]
    #[should_panic(expected = "a mis-paired lowering is a keryx error")]
    fn a_pairing_outside_the_table_is_a_keryx_error_not_a_refusal() {
        // The decode fixes a datum's kind to its field's, so a `Native` field never carries a
        // string: the discharge is checked, not wildcarded — loud, never a value lowered as
        // another kind and never a refusal that misnames a keryx bug as the payload's fault.
        let _ = lower(Scalar::Int32, ScalarTreatment::Native, &Datum::Str("x"), AT);
    }
}
