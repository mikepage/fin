# Fin

Personal finance for macOS. Tauri v2 with a Rust Leptos frontend. Local only: all data is one JSON file in the app data dir (`~/Library/Application Support/com.mikepage.fin/fin.json`), written atomically.

- CAMT.053 import (camt.053.001.02 through .08), multiple files at once, with progress; re-importing skips duplicates
- Categories with rules (IBAN, whole-word text, money in or out), splits
- Monthly overview against the budget; budgets per category and per group, with a year check against the budgeted income
- A month budget (fixed income, fixed and variable costs) and a year budget (that plus extra income such as holiday pay, and investments)
- Year result, left to spend and the year forecast; reports of expenses and income per month
- Energy contracts with usage, net metering and year-end bills
- Dutch (default) or English, set in Settings and stored in the data file

See CHANGELOG.md for what each part does.

## Languages

Texts in the app are English in the source and translated through `t!("…")` / `tn!(n, "one", "other")` (`src/i18n.rs`); the Dutch catalog is `src/i18n/nl.rs`, compiled in. The backend returns English messages, which the CLI prints as they are and the app translates (`i18n::error`, with `{}` for the parts that vary). `cargo test` fails when a string or backend message has no Dutch translation, or when the catalog has an entry nothing uses.

Standard category and group names come from `src-tauri/locales/nl-NL.toml` and `en.toml` (same keys, the ids in `shared/src/catalog.rs`). The data file's `language` decides which names it holds; on load and on a switch the backend renames them. Default rules live in `nl-NL.toml` only: they match Dutch bank descriptions, whatever the language.

## CLI

`fin-cli` reads and changes the same data file and prints JSON, so Claude Code (or a script) can query spending, categorise, add rules, set budgets and import statements. The skill in `.claude/skills/fin` tells Claude Code how to use it.

```bash
cargo install --path src-tauri --bin fin-cli
fin-cli help
fin-cli transactions --unsorted
```

Writes go through the same store as the app: daily backups, and neither side saves over the other's changes. The app reloads when its window gets focus.

## Layout

- `src/`: Leptos frontend (wasm, built by Trunk); translations in `i18n.rs` and `i18n/nl.rs`
- `src-tauri/`: backend: storage (`store.rs`), CAMT.053 parser (`camt.rs`), commands (`lib.rs`), CLI (`cli.rs`, `bin/fin-cli.rs`)
- `shared/`: types and pure logic (monthly overview, amount parsing) used by both

## Develop

```bash
cargo install tauri-cli --version ^2.0.0 --locked
cargo install trunk --locked
rustup target add wasm32-unknown-unknown
cargo tauri dev
```

`cargo test` runs the backend, shared and frontend tests, including an IPC test that calls the real commands through Tauri's mock runtime and the translation checks.
