//! The textproto pre-parse depth guard (spec §8, §26; the threat model's property 3, branch (b)):
//! a total lexical scan bounding a textproto payload's message-value nesting **before** the
//! engine's text-format parser sees it. That parser recurses natively on every nested message
//! value and carries no depth limit of its own — prost-reflect 0.16.5
//! `src/dynamic/text_format/parse/mod.rs`, `parse_message_value` (47) → `parse_field` (69) →
//! `parse_field_value` (170) → `parse_value` (273) → `parse_message_value` (316), no counter on
//! the cycle, entered from `DynamicMessage::parse_text_format` (`text_format/mod.rs:40,61`) — so
//! an over-deep payload would overflow the stack and *abort*, which `fault::contain` cannot hold,
//! where the binary decoder's recursion limit refuses cleanly. The guard measures the nesting
//! first and refuses past the uniform ceiling ([`NESTING_CEILING`], the walk's), so the parser
//! only ever sees a payload the walk would admit: for textproto the guard is where the ceiling
//! binds, and the walk's counter stands beneath it as defense-in-depth, as it stands beneath the
//! engine's limit for binary.
//!
//! **A measure, not a parse.** The scan reads lexical structure — brackets, strings, comments —
//! and builds no tree and assigns no meaning: the bounded departure from R5 the source door's
//! scan is (`descriptor::source`), named there and in the threat model. It is that scanner's
//! sibling, not its copy: each is derived from the lexer of the parser it precedes, and the two
//! grammars differ — the text format's one comment is `#` to end of line, and `/` is a token of
//! its `Any` syntax, never a comment marker — so a scanner honouring `//` or `/* */` here would
//! be reading a grammar this parser does not have. Cost: one pass over the payload's bytes,
//! allocation-free; every structural character is ASCII and no byte of a multi-byte UTF-8
//! sequence is, so the validated text is scanned as bytes.

use crate::codec::walk::NESTING_CEILING;
use crate::diagnostics::{Diagnostic, DiagnosticKind, Diagnostics, Locus};

/// Bound a textproto payload's nesting before the engine parses it: `Ok(())` for a payload whose
/// message values nest at most [`NESTING_CEILING`] levels below the root — the uniform ceiling,
/// the walk's own, counted here in message *values* where the walk counts *occupants*: exact for
/// a singular or repeated message field (one opener, one occupant), so such a payload is admitted
/// exactly as deep as its binary form; conservative for a map entry (two openers, the entry's and
/// its value's, per occupant) and an expanded `Any` (an opener for the expansion, and more for
/// whatever it nests, which the walk never enters), which are admitted to a shallower message
/// depth than on the wire, never a deeper one — and `PayloadTooDeep` past it, before
/// `parse_text_format` recurses on a single level. `text` is
/// the payload already validated as UTF-8 by the decode that calls this — a non-UTF-8 payload is
/// that decode's `UndecodablePayload` — so the scan reads bytes no structural character can hide
/// in.
///
/// **The stack margin the parse thread is sized against.** The parser spends between two and
/// five call frames per nesting level, by the field's form (`parse/mod.rs`): four for a singular
/// message field (`parse_message_value` 47 → `parse_field` 90 → `parse_field_value` 218 →
/// `parse_value` 316), five for a list or map element (`parse_field_value` 176/204 →
/// `parse_repeated_value` 253/259 → `parse_value`), two for an `Any` value (`parse_field` 115,
/// directly) — so the deepest admitted payload holds at most `5 × 99` live frames of that cycle,
/// plus the leaf calls. The frames are large, so the margin is measured rather than assumed (this
/// pin, debug and release builds): 99 nested message values parse on a 256 KB thread stack in
/// release (192 KB overflows — some 2.5 KB a level; an 8 MB stack overflows between 3,000 and
/// 4,000 levels) but need 2.5 MB in debug (2 MB overflows — some 25 KB a level; 8 MB overflows
/// between 300 and 400 levels), more than a spawned thread's 2 MB default, the test harness's
/// threads among them. So the decode that calls this leaves the parse to no thread of the
/// caller's: it runs it on a thread it sizes itself, against these figures, at 8 MiB
/// (`engine::TEXTPROTO_PARSE_STACK`, some three times the ceiling's debug need) — closing by
/// construction the residual a sub-standard thread stack would otherwise be, where the source
/// door's guard leaves that residual to the consuming service's process isolation (the threat
/// model's division of labor).
///
/// # Errors
///
/// `PayloadTooDeep` at the whole-payload locus — there is no field path before a parse — naming
/// the depth measured and the ceiling, and nothing of the payload's text.
pub(crate) fn depth(text: &str) -> Result<(), Diagnostics> {
    let measured = deepest(text);
    if measured > NESTING_CEILING {
        return Err(too_deep(measured));
    }
    Ok(())
}

/// The deepest message-value nesting `text` reaches: the maximum, over the scan, of `{`/`<`
/// openers less `}`/`>` closers outside string literals and comments — the levels the text-format
/// parser would recurse to, or more. The scanner's states mirror prost-reflect 0.16.5's text-format
/// lexer (`src/dynamic/text_format/parse/lex.rs`) wherever that lexer continues:
///
/// - **Comments.** `#` to end of line, and only that (`lex.rs:10`, the lexer's one skip pattern
///   beside whitespace, `lex.rs:9`): the scanner drops everything from a `#` to the newline. `/`
///   is the `ForwardSlash` token of the `Any` field name (`lex.rs:50`), so `//` and `/* */` are
///   token sequences, not comments, and a bracket after them counts as the parser would see it.
/// - **Strings.** A literal opens at `'` or `"` (`lex.rs:26`) and runs to the *same* quote
///   (`lex.rs:197,202`); the other quote is content (`lex.rs:205`), as are `#` and every bracket
///   (`lex.rs:137`). An escape is `\` and its form (`lex.rs:141-150`) — `\"`, `\'`, and `\\`
///   among them — so the scanner skips the byte after a `\`: an escaped quote keeps the literal
///   open, and the quote after an escaped backslash ends it. Adjacent literals concatenate in the
///   parser (`mod.rs:514`) but are separate tokens, each delimited on its own.
/// - **Brackets.** `{`, `}`, `<`, `>`, `[`, `]` are single-byte tokens (`lex.rs:32-43`). The
///   parser recurses on `{ }` and `< >` alone (`mod.rs:51-55`); `[ ]` delimits a list
///   (`mod.rs:242`) or an extension or `Any` field name (`mod.rs:143`) and opens no recursion —
///   a list's message elements open their own `{ }`/`< >` (`mod.rs:253,259`) — so the scanner
///   counts the first two pairs and passes over the third. A closer with nothing to close is the
///   parser's to refuse and leaves the count at zero.
///
/// Where the lexer instead *stops* — a bare newline or a NUL inside a literal, an invalid escape,
/// the end of input inside a literal (`lex.rs:137,210-228`, each an error the parser propagates
/// and reads nothing past, `mod.rs:600`) — the scanner's reading of the rest can only exceed the
/// parser's, which read none of it. It leaves a literal at a newline unconditionally, a `\`
/// before the newline notwithstanding, so a one-line string prefix can never hide the nesting
/// after it.
///
/// **Dominance** (`message-nesting depth ≤ scanner depth`, for every input): every
/// `parse_message_value` frame is opened by consuming exactly one `{` or `<` token
/// (`mod.rs:51-55`, the only site that consumes them) and closed by consuming its matching
/// terminator (`mod.rs:60-62`, the only site that consumes a `}` or `>`); the lexer emits those
/// tokens exactly at the `{`/`<`/`}`/`>` bytes outside literals and comments, where the scanner
/// counts them. So at every token the parser reaches, the scanner's count is the parser's live
/// message depth — exactly, for a payload the parser accepts — and past a lexer error it can only
/// be higher. No counted bracket is ever skipped as literal or comment content the lexer would
/// tokenize. Against the *walk* the count is a bound, not always the same number: a singular or
/// repeated message field is one message value and one occupant, but a map entry is two message
/// values (`m { key: … value { … } }`) for the one occupant the walk counts, and an expanded
/// `Any` (`[type.googleapis.com/pkg.Msg] { … }`) is a message value the walk never enters, with
/// whatever it nests (`Any` is opaque to the walk, spec §10). So the guard never admits deeper
/// than the walk would — the safe direction — and binds those two forms conservatively earlier: a
/// map-of-message chain is admitted to 49 levels here where its wire form is admitted to 99 — a
/// settled, documented consequence of bounding the parser lexically (a bracket scan cannot tell an
/// entry's opener from an occupant's); the threat model's property 3 and spec §26 record it.
fn deepest(text: &str) -> usize {
    /// Where the scan stands: in ordinary text, in a `#` comment, or in a string literal opened
    /// by the quote it holds.
    enum State {
        Text,
        Comment,
        Literal(u8),
    }

    let bytes = text.as_bytes();
    let mut state = State::Text;
    let mut depth: usize = 0;
    let mut max: usize = 0;
    let mut at = 0;
    while at < bytes.len() {
        let byte = bytes[at];
        match state {
            State::Text => match byte {
                b'#' => state = State::Comment,
                b'"' | b'\'' => state = State::Literal(byte),
                b'{' | b'<' => {
                    depth += 1;
                    max = max.max(depth);
                }
                b'}' | b'>' => depth = depth.saturating_sub(1),
                _ => {}
            },
            State::Comment => {
                if byte == b'\n' {
                    state = State::Text;
                }
            }
            State::Literal(quote) => match byte {
                // A newline ends the literal unconditionally: the lexer refuses it there.
                b'\n' => state = State::Text,
                // The escaped byte is content — an escaped quote among them — never a newline.
                b'\\' if bytes.get(at + 1) != Some(&b'\n') => at += 1,
                _ if byte == quote => state = State::Text,
                _ => {}
            },
        }
        at += 1;
    }
    max
}

/// `PayloadTooDeep` at the whole-payload locus for a payload nesting `measured` levels: the depth
/// and the ceiling as numbers and nothing of the text — the detail is composed from the measure
/// alone, so no byte of the payload reaches a reader of the diagnosis.
fn too_deep(measured: usize) -> Diagnostics {
    Diagnostic::new(
        DiagnosticKind::PayloadTooDeep,
        Locus::whole(),
        format!(
            "the textproto payload nests message values {measured} levels below the root, past keryx's payload nesting ceiling of {NESTING_CEILING}"
        ),
    )
    .into()
}

#[cfg(test)]
mod tests {
    use super::{deepest, depth};
    use crate::codec::walk::NESTING_CEILING;
    use crate::diagnostics::DiagnosticKind;

    const BRACES: (char, char) = ('{', '}');
    const ANGLES: (char, char) = ('<', '>');

    /// A textproto nesting the message-typed field `field` `levels` deep through the bracket
    /// pair `open`/`close`, with `innermost` as the deepest message's body:
    /// `field open field open … innermost close close`.
    fn nested(field: &str, (open, close): (char, char), levels: usize, innermost: &str) -> String {
        let mut text = String::new();
        for _ in 0..levels {
            text.push_str(field);
            text.push(' ');
            text.push(open);
            text.push(' ');
        }
        text.push_str(innermost);
        for _ in 0..levels {
            text.push(' ');
            text.push(close);
        }
        text
    }

    /// The one refusal `depth` returns for `text`, at the whole-payload locus: its kind and detail.
    fn refusal(text: &str) -> (DiagnosticKind, String) {
        let diagnostics = depth(text).expect_err("refused");
        assert_eq!(diagnostics.len(), 1, "one diagnosis: {diagnostics}");
        let diagnostic = diagnostics.iter().next().expect("one diagnostic");
        assert!(
            diagnostic.locus().is_whole(),
            "the whole-payload locus: {diagnostic}"
        );
        (diagnostic.kind(), diagnostic.detail().to_owned())
    }

    #[test]
    fn depth_99_is_admitted_and_depth_100_is_refused_through_either_bracket_pair() {
        // The uniform ceiling met from the textproto side, from the one constant: 99 nested
        // message values pass the guard, 100 are `PayloadTooDeep` — the boundary the walk pins
        // for binary (`codec::walk`, `tests/codec_depth.rs`) — through `{ }` and `< >` alike,
        // and through the two mixed, each opener being one message value.
        assert_eq!(NESTING_CEILING, 99);
        for pair in [BRACES, ANGLES] {
            let admitted = nested("f", pair, 99, "");
            assert_eq!(deepest(&admitted), 99);
            depth(&admitted).expect("the deepest admitted payload passes the guard");
            let refused = nested("secret_field", pair, 100, "");
            assert_eq!(deepest(&refused), 100);
            let (kind, detail) = refusal(&refused);
            assert_eq!(kind, DiagnosticKind::PayloadTooDeep);
            assert!(
                detail.contains("100") && detail.contains("99"),
                "the detail names the depth and the ceiling: {detail}"
            );
            assert!(
                !detail.contains("secret_field"),
                "the detail echoes nothing of the payload: {detail}"
            );
        }
        let mut mixed = String::new();
        let mut closers = Vec::new();
        for level in 0..100 {
            let (open, close) = if level % 2 == 0 { BRACES } else { ANGLES };
            mixed.push_str("f ");
            mixed.push(open);
            mixed.push(' ');
            closers.push(close);
        }
        while let Some(close) = closers.pop() {
            mixed.push(close);
            mixed.push(' ');
        }
        assert_eq!(deepest(&mixed), 100);
        assert_eq!(refusal(&mixed).0, DiagnosticKind::PayloadTooDeep);
    }

    #[test]
    fn a_well_formed_payload_measures_its_message_values_exactly() {
        assert_eq!(deepest(""), 0);
        assert_eq!(deepest("sensor: \"s-101\" temp_c: 44"), 0);
        assert_eq!(
            deepest(
                "readings { sensor: \"s-101\" temp_c: 44 } readings { sensor: \"s-107\" temp_c: 21 }"
            ),
            1
        );
        assert_eq!(deepest("a { b < c { } > }"), 3);
        assert_eq!(deepest("a { } b { } c { d { } }"), 2);
        // A field name takes its message value with or without a colon (`mod.rs:72-78`).
        assert_eq!(deepest("a: { b: < > }"), 2);
        depth("a { b < c { } > }").expect("a shallow payload passes");
    }

    #[test]
    fn a_bracket_in_a_comment_does_not_count_and_a_quote_in_a_comment_opens_no_string() {
        // `#` to end of line is the text format's one comment (`lex.rs:10`): a bracket inside it
        // is never a token, so it adds and closes nothing — the guard admits a payload at the
        // ceiling whose deepest body carries a commented-out bracket ...
        assert_eq!(deepest("# { { { {\na { }"), 1);
        assert_eq!(deepest("a { } # < < <"), 1);
        assert_eq!(deepest("a {\n  # }\n  b { }\n}"), 2);
        depth(&nested("f", BRACES, 99, "# {\n")).expect("a comment's bracket does not count");
        // ... while a quote inside a comment is comment text, opening no string that would
        // swallow the brackets on the next line.
        assert_eq!(deepest("# it's\na { b { } }"), 2);
        assert_eq!(deepest("# \"\na { }"), 1);
        // `//` and `/* */` are not comments in this grammar — `/` is a token of the `Any` field
        // name (`lex.rs:50`) — so a bracket after them counts, as the parser would see it.
        assert_eq!(deepest("// {\na { }"), 2);
        assert_eq!(deepest("/* { */ a { }"), 2);
    }

    #[test]
    fn a_bracket_in_a_string_does_not_count() {
        // A literal runs from its quote to the same quote (`lex.rs:26,197,202`); whatever it
        // holds is content, never a token — either bracket pair, the other quote, and `#`.
        assert_eq!(deepest("s: \"{{{{\""), 0);
        assert_eq!(deepest("s: '<<<<'"), 0);
        assert_eq!(deepest("s: \"it's {\" a { }"), 1);
        assert_eq!(deepest("s: 'say \"{\"' a { }"), 1);
        assert_eq!(deepest("s: \"# {\" a { }"), 1);
        assert_eq!(deepest("a { s: \"}\" b { } }"), 2);
        depth(&nested("f", BRACES, 99, "s: \"{ {\"")).expect("a string's brackets do not count");
    }

    #[test]
    fn an_escaped_quote_keeps_a_string_open_and_an_escaped_backslash_does_not() {
        // The escapes of `lex.rs:141-150`: `\"` and `\'` are content, so the literal stays open
        // across them and the brackets inside stay content ...
        assert_eq!(deepest("s: \"\\\"{{{\""), 0);
        assert_eq!(deepest("s: '\\'<<<'"), 0);
        assert_eq!(deepest("s: \"\\x22{\" a { }"), 1);
        depth(&nested("f", BRACES, 99, "s: \"\\\"{\""))
            .expect("an escaped quote keeps the string closed over its bracket");
        // ... while `\\` is one escaped backslash, so the quote after it ends the literal: a
        // scanner reading `\\"` as an escaped quote would stay inside and miss every bracket
        // after it — the under-count dominance forbids.
        assert_eq!(deepest("s: \"\\\\\" a { b { } }"), 2);
        assert_eq!(deepest("s: '\\\\' a < >"), 1);
        assert_eq!(
            refusal(&nested("f", BRACES, 99, "s: \"\\\\\" g { }")).0,
            DiagnosticKind::PayloadTooDeep
        );
    }

    #[test]
    fn concatenated_string_literals_are_each_delimited() {
        // Adjacent literals concatenate in the parser (`mod.rs:514`) but are separate tokens in
        // the lexer, each delimited on its own: `""` ends one literal and opens the next rather
        // than escaping a quote, and the brackets after the last one count.
        assert_eq!(deepest("s: \"{\" \"{\" '{'"), 0);
        assert_eq!(deepest("s: \"{\"\"{\""), 0);
        assert_eq!(deepest("s: \"\" \"\" a { }"), 1);
        assert_eq!(deepest("s: \"x\"\"{\" a { }"), 1);
        assert_eq!(deepest("s: \"{\" '{' a { b < > }"), 2);
        depth(&nested("f", BRACES, 99, "s: \"{\" \"{\""))
            .expect("concatenated strings' brackets do not count");
        assert_eq!(
            refusal(&nested("f", BRACES, 99, "s: \"{\" \"\" g { }")).0,
            DiagnosticKind::PayloadTooDeep
        );
    }

    #[test]
    fn list_brackets_delimit_but_add_no_depth_while_their_message_elements_do() {
        // `[ ]` is a list (`mod.rs:242`) or an extension or `Any` field name (`mod.rs:143`); the
        // parser recurses on neither — a list's message elements open their own `{ }`/`< >`
        // (`mod.rs:253,259`, through `parse_value` to `parse_message_value`).
        assert_eq!(deepest("a: [1, 2, 3]"), 0);
        assert_eq!(deepest("a: []"), 0);
        assert_eq!(deepest("a: [{ b: 1 }, { b: 2 }]"), 1);
        assert_eq!(deepest("a: [< b: 1 >, { c { } }]"), 2);
        assert_eq!(deepest("[ext.full.name]: 1 [ext.msg] { }"), 1);
        assert_eq!(deepest("[type.googleapis.com/pkg.Msg] { a { } }"), 2);
        assert_eq!(deepest(&"[".repeat(200)), 0);
        depth(&nested("f", BRACES, 99, "l: [1, [2]]")).expect("list brackets add no depth");
        assert_eq!(
            refusal(&nested("f", BRACES, 99, "l: [{ }]")).0,
            DiagnosticKind::PayloadTooDeep
        );
    }

    #[test]
    fn a_newline_ends_the_scanner_s_string_where_the_lexer_refuses_it() {
        // A bare newline inside a literal is a lexer error (`lex.rs:137` excludes it from
        // content and nothing else matches it, `lex.rs:210-222`), so the parser reads nothing
        // past it; the scanner leaves the literal there unconditionally — a `\` before the
        // newline does not hold it open — and counts what follows, so a one-line string prefix
        // can never hide nesting from the guard.
        assert_eq!(deepest("s: \"open\na { b { } }"), 2);
        assert_eq!(deepest("s: \"\\\na { b { } }"), 2);
        assert_eq!(deepest("s: \"\\\r\na { }"), 1);
        assert_eq!(
            refusal(&nested("f", BRACES, 99, "s: \"\n g { }")).0,
            DiagnosticKind::PayloadTooDeep
        );
    }

    #[test]
    fn an_unmatched_closer_never_underflows_and_the_scan_is_total_on_any_text() {
        // A stray closer the parser would refuse leaves the count at zero, so the nesting after
        // it still counts; and the scan reads any text to its end — a trailing `\`, an
        // unterminated literal or comment, a NUL, non-ASCII content — and returns.
        assert_eq!(deepest("} } > a { b { } }"), 2);
        assert_eq!(deepest("> {"), 1);
        assert_eq!(deepest("s: \"\\"), 0);
        assert_eq!(deepest("\\"), 0);
        assert_eq!(deepest("s: \"unterminated {"), 0);
        assert_eq!(deepest("# unterminated {"), 0);
        assert_eq!(deepest("s: \"\0{\" a { }"), 1);
        assert_eq!(deepest("s: \"héllo {\" a { \u{1F600} b < > }"), 2);
        depth("").expect("an empty payload passes");
        depth("} } }").expect("unmatched closers are the parser's to refuse, not the guard's");
    }
}
