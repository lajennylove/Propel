# Propel: Working Agreements

Propel is a Blade-inspired templating system for Rocket.rs. Templates compile to typed Rust views. This file records the team's agreements for working on Propel. Add to it as decisions are made.

The full design lives in the vault note `Projects/Rocket/propel-plan.md`. This file records agreements only. Design detail belongs in the plan.

---

## 1. Repository layout

Two separate git repositories, one inside the other:

```
Propel/                       # Rocket clone (git repo, remote: origin = rwf2/Rocket)
├── core/ contrib/ ...        # Rocket's crates: upstream code, unchanged
├── .gitignore                # ignores /propel/
└── propel/                   # Propel repo (own git history, own remote)
    ├── Cargo.toml            # its own workspace
    ├── README.md             # this file
    ├── propel_parser/
    ├── propel_compiler/
    ├── propel_macros/
    └── propel_rocket/
```

- The root repo is Rocket's. Restore its GitHub history and remote, and keep it clean of Propel code.
- Everything under `propel/` belongs to the Propel repo. Run Propel commands from inside `propel/`.
- Rocket's crates are **upstream code**. Don't edit them for Propel work. If Propel needs a change in Rocket, raise it as an upstream issue or PR.
- Propel's crates depend on Rocket by git revision (`3a54d079`), so this repo builds on its own. When Rocket 0.6 ships on crates.io, switch to that version.
- **User views are not in this folder.** Application templates live in the user's Rocket project, in the regular Rocket layout, and depend on the Propel crates from there.

## 2. Remotes

- Rocket's remote is `origin` and points at `rwf2/Rocket`. Don't push to it.
- Propel's repo has no remote yet. The owner adds one with `git remote add origin <url>`.

## 3. Architecture decisions (settled)

- **Compile-time views.** Templates compile to Rust. Structure is checked at `cargo check`. There is no runtime template lookup and no `serde_json::Value` context.
- **The handler builds the context.** Each view has a typed context struct. The handler is responsible for filling every field. A missing field is a compile error in the handler, not a runtime "undefined variable".
- **Shared data goes through request guards.** Anything a Laravel view composer would provide (current user, nav, flash messages) comes from a Rocket request guard or fairing.
- **Escaping is on by default.** `{{ }}` escapes. Raw output needs an explicit `{!! !!}`. Don't add raw output for user-controlled data.
- **Empty states are explicit.** A loop or optional value that can be empty has to say what it renders when it is empty (`@forelse` with `@empty`).

## 4. Development loop (settled)

- **Rebuild on save** is the default. Run `bacon` or `cargo watch -x run`. Keep templates in their own crate so a template edit rebuilds only that crate and the app.
- **Debug-only interpreter** is planned. In debug builds, Propel will parse templates at request time for reload without a rebuild. It must match the compiled output exactly. Shared test cases run against both paths, so the two can't drift.
- **Release builds use compiled views only.** The interpreter is never used in release.

## 5. Quality bar

- Match Rocket's style: edition 2021, `rust-version = "1.75"`, `[lints] workspace = true`, and `.rustfmt.toml` / `.editorconfig` from the repo root.
- Every crate has at least one test. Template behavior is tested with golden files: input template, typed context, expected HTML.
- Verify before saying something works. Run `cargo check`, `cargo test`, and `cargo clippy` for the affected crates, and report the actual output.
- Performance is the reason Propel exists on Rocket. Changes to the render path should come with a benchmark or a note explaining why they don't affect it.

## 6. Commits and history

- Don't commit or push unless the owner asks.
- Commit messages end with the attribution line required by the owner's tooling.

## 7. Open decisions

Tracked in the vault plan under "Gaps & Open Decisions". Settle them here once decided:

- [ ] Parser: Pest or Nom (needed before Task 2)
- [ ] Blade fidelity: match Blade's syntax exactly, or keep a custom grammar
- [ ] `@forelse` empty-state syntax
- [ ] Escaping rules per type
- [ ] Crate placement: top-level (as now) or `contrib/propel/`
- [ ] Template directory convention and file extension (`.propel` proposed)
