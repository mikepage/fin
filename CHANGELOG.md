# Changelog

All notable changes to Fin are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [2.0.0] - 2026-10-01

The first release of this repository. Fin is personal finance for macOS: one local
JSON file, no account, no network.

### Added

- **Import.** CAMT.053 bank statements (camt.053.001.02 through .08), several files
  at once, with progress; importing again skips what is already there. Each
  statement goes to the account with its IBAN. Bank balances per account, checked
  against the transactions in between.
- **Categories and rules.** Standard categories in Dutch or English, plus your own.
  Rules by IBAN, by whole words in the description (case and punctuation don't
  matter) or by words for money in or out only; the most specific rule wins.
  Default rules for common Dutch counterparties. Split a transaction over
  categories, or let it count in another month.
- **Overview.** Per month: income, fixed costs and variable spending against the
  budget, the result against the budgeted result, and a comparison with another
  month. A group with a group budget is one line that opens to its categories and
  the rest they share.
- **Budget.**
  - Plan: budgets per category and month, and per group one budget for its
    variable (or fixed) categories together; own budgets inside count within it.
    A year with budgeted income has to fit: an increase is capped or refused.
    Fill in from an average, copy a month to the next or last year to this one.
  - Left to spend (the first tab): per variable budget this month's budget against
    what has been spent, where that should be by today, and what is left, per day
    too; earlier and later months with ‹ ›.
  - Year result: the year at budget and at the current pace, and the result per
    month against the budget with its cumulative total.
  - Year forecast: fixed income, fixed costs, variable and the result, budgeted,
    actual so far and expected; then extra income, investments and the net. A
    category budgeted in some months of the year plans nothing in the others.
- **Reports.** Expenses and income per month for a year, by category and channel
  (in store or online), with the average.
- **Contracts.** Energy contracts with tariffs, usage per period, net metering and
  year-end bills, and a monthly overview of cost against the monthly payment.
- **Backups.** Daily, before an import or restore, and by hand; restore from the
  list or a file.
- **fin-cli.** The same data from the command line, as JSON: transactions, rules,
  budgets and group budgets, the overview, forecast, contracts, import and backups.
  A Claude Code skill in `.claude/skills/fin` explains how to use it.

### Notes

- The app is not signed. On first launch, right-click Fin and choose Open, or run
  `xattr -dr com.apple.quarantine /Applications/Fin.app`.

[2.0.0]: https://github.com/mikepage/fin/releases/tag/v2.0.0
