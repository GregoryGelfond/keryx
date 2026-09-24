# The fault boundary

keryx depends on foreign code — the descriptor engine, the `.proto` compiler, the JSON deserializer, the ASP layer. It trusts each on the input it *admits*, but not on arbitrary input. The fault boundary is what keeps a dependency's misbehavior on an adversarial input path from becoming a crash in the tool that embeds keryx.

## Faults are contained as values

A refusal keryx *foresees* is a [diagnostic](diagnostics.md). A fault keryx does *not* foresee — an unexpected panic from a dependency on a foreign-input path — is **contained**: caught at the door it crossed and returned as a typed dependency fault, not allowed to unwind through the caller. So a tool calling `keryx-core` sees a `Result`, never a process abort, even on input crafted to break a dependency.

This is defense-in-depth. Where a specific dependency misbehavior is known, keryx pre-empts it before the call (an editions descriptor set, for instance, is refused with `UnsupportedEdition` rather than handed to an engine that would panic on it). The containment catches the *un*foreseen residual.

## Cooperating with a panic hook

Because containment uses a caught unwind, a process-wide panic hook would otherwise print a scary backtrace for a fault keryx is handling deliberately. keryx exposes one flag for this:

```rust,ignore
if keryx_core::is_containing() {
    // a fault keryx is turning into a value — stay quiet
    return;
}
```

A consuming tool's panic hook may consult `is_containing()` to stay silent for a fault keryx returns as a value. It is the one public window into the containment machinery; everything else about the boundary is internal.
