# Contributing to PlayLibre

Thanks for helping build PlayLibre! This guide covers how to get set up, how
we work, and the rules that keep the project legally clean.

## Ground rules

- Be kind. Everyone here follows the [Code of Conduct](CODE_OF_CONDUCT.md).
- Found a security problem? Please report it privately, as
  [SECURITY.md](SECURITY.md) explains, not in an issue.
- Small pull requests get reviewed fastest. Open an issue first for anything
  large, such as a new subsystem or a change to the scripting API.
- By contributing, you agree your work is dual-licensed under
  MIT OR Apache-2.0, like the rest of the project (see the README).

## Clean-room rules (please read)

PlayLibre's scripting API follows a familiar, widely used style. To keep the
project free of anyone else's copyrights and trade secrets, **every
contribution must be clean-room**:

1. **Only public behaviour.** You may use publicly available documentation,
   tutorials and your own experience of how an API behaves to decide *what*
   PlayLibre should do.
2. **Never decompile, disassemble or reverse-engineer** any proprietary game
   client, engine, server or tool, and never use code, symbols or data that
   came from doing so. Don't submit work based on leaked source code either.
3. **Never copy text.** Write documentation, comments and error messages in
   your own words. Don't paste or closely paraphrase other platforms'
   reference pages, API descriptions or examples.
4. **Never copy assets.** No models, textures, sounds, fonts, icons, logos or
   UI designs from other platforms, even "just as placeholders".
5. **No third-party trademarks in branding.** Don't use other companies'
   product names in PlayLibre's name, logo, taglines, crate names or marketing.
6. **Keep API names in the naming layer.** Script-facing names belong in
   [`crates/playlibre-script/src/names.rs`](crates/playlibre-script/src/names.rs),
   not scattered through the engine, so the API can be renamed in one place.
   Engine code uses its own internal names (`ClassKind`, `Property`).

If you're unsure whether something is allowed, ask in an issue before
writing code. When in doubt, leave it out.

Pull requests may be asked to confirm: *"I wrote this contribution myself,
from public information only, and it follows the clean-room rules."*

## Getting set up

1. Install a recent stable Rust toolchain: `rustup update stable`.
2. Install Bevy's system dependencies (see the README for Arch and Debian/Ubuntu).
3. Build and test:

   ```sh
   cargo test --workspace
   cargo run -p playlibre-proto
   ```

## Before you open a pull request

Run the same checks as CI:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Also:

- Add or update tests for behaviour you change. The instance tree and script
  runtime are fully testable without a GPU; the prototype has headless tests too.
- Keep `unsafe` out. The workspace forbids it; if you think you need it, open
  an issue first.
- Write commit messages in the imperative mood ("Add part transparency"),
  with a short summary line and an optional body explaining why.

## Project structure

- `crates/playlibre-instance`: the engine-agnostic instance tree.
- `crates/playlibre-script`: the Luau runtime and the script-facing API.
- `crates/playlibre-proto`: the Phase 0 Bevy prototype.

## Reporting bugs and security issues

Open a GitHub issue for ordinary bugs, with steps to reproduce. For a
security problem (for example, a script escaping the sandbox), please report
it privately through GitHub's "Report a vulnerability" button on the
repository's Security tab instead of opening a public issue.
