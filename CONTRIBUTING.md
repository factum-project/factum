# Contributing to Factum

Thank you for your interest in contributing to Factum. As an early-stage alpha project, we are actively seeking collaborators who can help us find architectural issues while the architecture is still malleable.

## DCO Sign-Off

All contributions must be signed off with **Developer Certificate of Origin** (DCO). This is a lightweight alternative to CLA — you simply add `-s` to your git commit:

```bash
git commit -s -m "your commit message"
```

This attests that you have the right to submit the work under the project's license. See [DCO details](https://developercertificate.org/).

## PR Process

1. **Open an issue first** for any non-trivial change (new feature, API change, architectural shift). For bug fixes or documentation improvements, a PR directly is fine.
2. **Branch from `main`** and name your branch descriptively (`fix-parser-depth-limit`, `add-wikidata-converter`, etc.).
3. **Tests are mandatory** for new functionality. If you add a feature, add tests. If you fix a bug, add a regression test.
4. **Run `cargo test` and `cargo clippy`** before submitting. Zero warnings is the target (we're not there yet, but new code shouldn't add warnings).
5. **Keep PRs small.** One logical change per PR makes review faster and merge safer.

## Testing Requirements

| Change type | What you need |
|-------------|---------------|
| Bug fix | Regression test proving the fix |
| New morpheme | Pre-M2: add to `seed_morphemes()` in `crates/factum-core/src/morphemes.rs` + conformance vector. M2+: use `morphemes.toml` + `build.rs` codegen. |
| New parser feature | Conformance JSON vector in `spec/conformance/` + Rust test |
| New verifier | Unit test with pass/fail/inconclusive cases |
| Performance change | Before/after benchmark in `factum-bench` |

## Conformance Vectors

Parser test cases are maintained as language-agnostic JSON vectors in `spec/conformance/`. This allows future Python/TypeScript implementations to validate against the same test suite.

To add a new vector:
1. Create a JSON file in `spec/conformance/` following the existing format
2. Include: input source, expected parse result (or expected error), description
3. Run `cargo test -p factum-core conformance` to verify the Rust implementation passes

## Morpheme Proposals

New morphemes (content predicates) are the primary vocabulary extension mechanism. To propose a new morpheme:

1. Open an issue with the **morpheme proposal** template
2. Include: name, kind (Entity/Relation/Quantifier/Modal/Temporal), signature, documentation, and at least 3 example usages
3. Morphemes start as `Draft` status, move to `Review` after community discussion, and become `Adopted` after maintainer approval

## Fuzzing

If you find a parser crash (stack overflow, panic, etc.):
1. **Do not open a public issue** — see [SECURITY.md](SECURITY.md)
2. Minimize the crash case with `cargo +nightly fuzz run fuzz_parser -- -minimize_crash=1`
3. Report privately

## Code Style

- Follow `rustfmt` defaults
- Document public APIs with `///` doc comments
- Use `thiserror` for error types
- Prefer `Arc<T>` over `Rc<T>` (we need `Send + Sync`)

## Areas Needing Help

See [good first issues](https://github.com/factum-project/factum/labels/good%20first%20issue) and [ROADMAP.md](ROADMAP.md) for areas where contributions are most valuable.
