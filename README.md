# Propel: Working Agreements

Propel is a Blade-inspired templating system for Rocket.rs. Templates compile to typed Rust views. This file records the team's agreements for working on Propel. Add to it as decisions are made.

The full design lives in the vault note `Projects/Rocket/propel-plan.md`. This file records agreements only. Design detail belongs in the plan.

---

## 1. Repository layout

Two separate git repositories, one inside the other:

```
Propel/                           # Rocket clone (git repo, remote: origin = rwf2/Rocket)
├── core/ contrib/ ...            # Rocket's crates: upstream code, unchanged
├── examples/
│   └── propel/                   # the Propel test app (a Rocket app, in the Rocket repo)
├── .gitignore                    # ignores /propel/
└── propel/                       # Propel repo (own git history, own remote)
    ├── Cargo.toml                # its own workspace
    ├── README.md                 # this file
    ├── propel_parser/            # Pest grammar, AST, parser
    ├── propel_runtime/           # dev-mode renderer (reads templates from disk)
    ├── propel_compiler/          # release-mode code generation (not built yet)
    ├── propel_macros/            # proc macros (not built yet)
    └── propel_rocket/            # PropelView responder
```

- The root repo is Rocket's. Keep it clean of Propel code. The test app in `examples/propel/` is the exception, because it exists to exercise Propel.
- Everything under `propel/` belongs to the Propel repo. Run Propel commands from inside `propel/`.
- Rocket's crates are **upstream code**. Don't edit them for Propel work. If Propel needs a change in Rocket, raise it as an upstream issue or PR.
- Propel's crates depend on Rocket by git revision (`3a54d079`), so this repo builds on its own. When Rocket 0.6 ships on crates.io, switch to that version.
- The test app patches that git dependency to the local Rocket checkout (`[patch]` in `examples/Cargo.toml`). This keeps a single copy of Rocket in the build.
- **User views are not in this folder.** Application templates live in the user's Rocket project, in the regular Rocket layout, and depend on the Propel crates from there.

## 2. Remotes

- Propel's repo: `origin` is `github.com/lajennylove/propel` (public).
- The Rocket repo has no remote. Its GitHub repo for the test app is set up by the owner.

## 3. Architecture decisions (settled)

- **Compile-time views.** Templates compile to Rust. Structure is checked at `cargo check`. There is no runtime template lookup and no `serde_json::Value` context in release builds.
- **The handler builds the context.** Each view has a typed context struct. The handler is responsible for filling every field. A missing field is a compile error in the handler, not a runtime "undefined variable".
- **Shared data goes through request guards.** Anything a Laravel view composer would provide (current user, nav, flash messages) comes from a Rocket request guard or fairing.
- **Escaping is on by default.** `{{ }}` escapes. Raw output needs an explicit `{!! !!}`. Don't add raw output for user-controlled data.
- **Empty states are explicit.** A loop or optional value that can be empty has to say what it renders when it is empty (`@forelse` with `@empty`).
- **Parser is Pest.** The grammar is `propel_parser/src/propel.pest`. Syntax changes go in the grammar and in its tests.

## 4. Development loop (settled)

Two speeds, both in use:

- **View edits are instant.** In dev builds, `propel_runtime` reads the `.propel` file from disk on every render. Edit a view and refresh. No rebuild.
- **Rust edits rebuild.** Run `bacon` in `examples/propel/`. It rebuilds and restarts on changes to `src/` and `views/`. The config is `examples/propel/bacon.toml`.
- **Release builds will use compiled views only.** The runtime renderer is for development. Until `propel_compiler` is built, release builds also use the runtime renderer. This is a known gap.
- **Shared tests.** Template behavior is tested in `propel_runtime/tests/`. When the compiled path exists, the same golden files must pass against both renderers, so the two can't drift.
- Views are read from `views/` relative to the working directory. Run the app from its own directory.

## 5. Quality bar

- Match Rocket's style: edition 2021, `rust-version = "1.75"`, `[lints] workspace = true`. Formatting uses `propel/.rustfmt.toml`, because the Rocket root config disables formatting.
- Every crate has at least one test. Template behavior is tested with golden files: input template, typed context, expected HTML.
- Verify before saying something works. Run these in `propel/` and report the actual output:
  - `cargo fmt --all -- --check`
  - `cargo clippy --workspace --all-targets` (must be warning-free for Propel crates)
  - `cargo test --workspace`
- Performance is the reason Propel exists on Rocket. Changes to the render path should come with a benchmark or a note explaining why they don't affect it.

## 6. Tooling

These tools are installed on the development machine. Anyone else setting up Propel needs them too.

| Tool | Why | Install |
| --- | --- | --- |
| `cargo` (Homebrew) | Builds the workspaces | `brew install rust` |
| `rustup` + `clippy` | Lints. Homebrew's Rust has no clippy | `brew install rustup`, then `rustup default stable` and `rustup component add clippy rustfmt` |
| `bacon` | Rebuild-on-save for `examples/propel` | `brew install bacon` |

Homebrew's `cargo` is first on the PATH and has no clippy. Run the rustup tools from `~/.cargo/bin`, for example `~/.cargo/bin/cargo clippy`. To make rustup's tools the default, add this to `~/.zshrc`:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
```

## 7. Commits and history

- Don't commit or push unless the owner asks.
- Commit messages end with the attribution line required by the owner's tooling.

## 8. Open decisions

Tracked in the vault plan under "Gaps & Open Decisions". Settle them here once decided:

- [x] Parser: Pest (decided; grammar in `propel_parser/src/propel.pest`)
- [ ] Blade fidelity: match Blade's syntax exactly, or keep a custom grammar. The current syntax is a draft: `@foreach (item in collection)` is not Blade's form.
- [ ] `@forelse` empty-state syntax. Currently `@empty ... @endforelse`, as in Blade.
- [ ] Member access and truthiness of `Option`. Currently `post.title` and `null`/`false`/empty are false.
- [ ] Escaping rules per type. The runtime escapes `& < > " '` and echoes `null` as nothing.
- [ ] Crate placement: top-level (as now) or `contrib/propel/`
- [ ] Template directory convention and file extension (`.propel` used)
- [ ] Whether release builds keep the runtime renderer, or compiled views only
