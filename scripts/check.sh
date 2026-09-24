#!/bin/sh
# keryx's single check entry point. Each mode is a slice of the gate keryx holds
# every change to; `full` runs them all. Run from the repository root. Requires a
# stable toolchain with clippy and rustfmt (rust-toolchain.toml pins it); `ground`
# additionally requires clingo on PATH, and `book` requires mdBook 0.5.4.
#
#   scripts/check.sh portable    fmt, clippy (-D warnings), test, doc (-D warnings)
#   scripts/check.sh coverage    line coverage, floor 85 (cargo-llvm-cov)
#   scripts/check.sh ground      the clingo grounding gate (scripts/ground.sh)
#   scripts/check.sh book        build the mdBook manual
#   scripts/check.sh full        all of the above, in order
set -eu

usage() {
	echo "usage: scripts/check.sh <portable|coverage|ground|book|full>" >&2
	exit 2
}

portable() {
	cargo fmt --all --check
	cargo clippy --workspace --all-targets --locked -- -D warnings
	cargo test --workspace --locked
	RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
}

coverage() {
	cargo llvm-cov --workspace --locked --fail-under-lines 85
}

ground() {
	bash scripts/ground.sh
}

book() {
	want="mdbook v0.5.4"
	have="$(mdbook --version 2>/dev/null || true)"
	if [ "$have" != "$want" ]; then
		echo "book mode needs $want (found: ${have:-none})" >&2
		echo "install: cargo install --locked --version '=0.5.4' mdbook" >&2
		exit 3
	fi
	mdbook build
}

[ $# -eq 1 ] || usage
case "$1" in
	portable) portable ;;
	coverage) coverage ;;
	ground) ground ;;
	book) book ;;
	full)
		portable
		coverage
		ground
		book
		;;
	*) usage ;;
esac
