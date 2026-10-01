---
name: fin
description: Read and change the user's Fin personal finance data (transactions, categories, rules, budgets, imports, backups) with the fin-cli command. Use when the user asks about their spending, income, budgets, uncategorised transactions, or wants to categorise, add rules, set budgets or import bank statements.
---

# Fin CLI

`fin-cli` works on the same data file as the Fin app
(`~/Library/Application Support/com.mikepage.fin/fin.json`). Every command prints
JSON; errors print `{"error": ...}` on stderr with exit code 1. Run `fin-cli help`
for the full list.

If `fin-cli` is not on PATH, run it from the repo:
`cargo run -q -p fin --bin fin-cli -- <args>`.

## Reading

```
fin-cli accounts
fin-cli categories [--all]
fin-cli rules [CATEGORY]
fin-cli transactions --month 2026-09 [--category groceries] [--search albert] [--unsorted] [--limit 0]
fin-cli overview --month 2026-09        # budget vs actual per category; a group budget
                                        # is one line (group_line) with its members
fin-cli forecast --year 2026 [--pace]   # how the year ends at budget (or current pace)
fin-cli report --year 2026 --series expenses|fixed|variable|income|saldo|CATEGORY
fin-cli cashflow --months 12
fin-cli imports
fin-cli backups
```

## Changing

```
fin-cli categorize TX_ID CATEGORY
fin-cli rule add CATEGORY iban|text|in|out VALUE [--apply-from 2026-01-01]
fin-cli rule remove CATEGORY KIND VALUE
fin-cli budget set CATEGORY 2026-10 350      # income and fixed costs; "none" to clear
fin-cli budget group Transport variable 2026-10 115   # variable costs; "none" to clear
fin-cli category add NAME GROUP KIND         # KIND must match the group's fixed/variable
fin-cli category group CATEGORY GROUP
fin-cli budget average 2026 2026-09 6 [--overwrite]
fin-cli import ACCOUNT file.xml...           # CAMT.053
fin-cli undo-import IMPORT_ID
fin-cli backup
fin-cli restore BACKUP_NAME
fin-cli language [nl-NL|en]                  # the app's language; standard names follow
```

CATEGORY can be an id (`sys-groceries`), a catalog key (`groceries`) or a name in
either language (`Boodschappen`, `Groceries`). ACCOUNT can be an id, a name or an IBAN.
Category names in the output follow the app's language; messages are English.

## How to work

- Amounts in JSON are cents (`amount_cents`, negative = money out) with a
  formatted copy (`amount`, Dutch notation).
- "To categorise" (`sys-unsorted`) means still to be categorised; find these with
  `transactions --unsorted`.
- Prefer a rule over one-off categorising when a payee recurs. Text rules match
  whole words, case- and punctuation-insensitive; `in`/`out` rules only match money
  in or out. Show the user what a rule will catch (`transactions --search VALUE`)
  before adding it.
- Budgets are per month: set a year with a loop over the 12 months. Income and
  fixed costs are budgeted per category (`budget set`), variable costs only per
  group (`budget group GROUP variable`, GROUP by name in either language or key
  like `transport`); `budget set` on a variable category is refused. A group holds
  fixed or variable categories, never both (income, extra income and investments
  can sit alongside), so `category add`/`category group` refuse a mix. A year with
  income budgets can't go below zero (an increase is capped, `"capped": true`, or
  refused). Lower budgets before raising others. `budget average` sets each
  income/fixed category and each variable group (the sum of its categories'
  averages); `--overwrite` touches all of them.
- Ask before changes that are hard to undo: `restore`, `undo-import`, rules with
  `--apply-from`, or budget `--overwrite`. Writes are backed up daily, and imports
  and restores back up first.
- The app picks up CLI changes when its window gets focus.
