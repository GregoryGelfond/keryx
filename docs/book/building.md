# Building the documentation

The book is built with mdBook 0.5.4, pinned so the rendered output matches what the checks build.

Install the exact version:

```sh
cargo install --locked --version '=0.5.4' mdbook
```

Build the book — output lands under the git-ignored `target/book/`:

```sh
mdbook build
```

Or run it through the project's check entry point, which also compiles and tests the book's embedded examples:

```sh
scripts/check.sh book
```

The Markdown sources live under [`docs/book/`](.); `book.toml` at the repository root configures the build. The design-of-record documents (`docs/specification.md`, `docs/design/`) are kept outside the book and linked from [About this book](index.md).
