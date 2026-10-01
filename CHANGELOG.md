# Changelog

All notable changes to Fin are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [2.3.0] - 2026-10-02

### Added

- **Month budget** and **Year budget** on the Budget page. The month budget is
  what comes back every month: fixed income against fixed and variable costs. The
  year budget is the month budgets of January to December plus extra income (holiday
  pay, other income) and investments, in the month they come, with the year's total
  per row.

### Changed

- Extra income and investments can be budgeted. The forecast plans them as budgeted
  instead of only counting what happened; "From average…" still fills the month
  budget only.
- Budgeted extra income counts in the year check, so holiday pay makes room for an
  investment.
- Copying a month to the next copies the month budget only: holiday pay stays in May.
- The Budget report on Reports is against the month budget, also when extra income
  is budgeted.

## [2.2.0] - 2026-10-01

### Added

- **Budget report** on Reports: per complete month, the result against the
  budgeted result as bars, up when better, down when worse. Fixed income only, no
  extra income or investments.
- **Export CSV** on Reports: the report's months as a CSV file in Downloads
  (semicolons and decimal commas, for Dutch spreadsheets).

### Changed

- The Budget page is just the plan: budgets per month, set up for a year. How the
  month goes is on the Overview, how the months compare on Reports.

### Removed

- The Year result and Year forecast tabs, with their tiles, the year table and
  the result per month.

## [2.1.4] - 2026-10-01

### Changed

- The year forecast is a plain table like the others: one font size, no colours
  or badges, the result and the net in bold.
- The Plan grid lists income first, then fixed costs, then variable spending.
  A group budget's row no longer shows a category count (it is in the tooltip).

## [2.1.3] - 2026-10-01

### Fixed

- In an opened group, the compared month's line sits below each category's bar
  instead of over it.

## [2.1.2] - 2026-10-01

### Changed

- Overview lines read like a table: the amount and budget both with €, then one
  status per line in its own column (over budget or on budget) instead of a
  badge with the amount over. The category count on group lines is gone, so the
  names get the room.
- An opened group's categories compare with the chosen month too, with a bit
  more space between them.

## [2.1.1] - 2026-10-01

### Fixed

- Categories stay grouped in catalog order: a category that moved to another
  group (like health insurance to Insurance) no longer leaves that group in two
  places in lists and the Plan grid.
- "From average…" counts budgets, with variable categories per group.

## [2.1.0] - 2026-10-01

### Changed

- Variable spending is budgeted per group only; fixed costs and income per
  category only. A group holds either fixed or variable costs, never both, and a
  new category follows its group's type.
- Health, car and moped/bike insurance move to Insurance; road tax, car
  purchase/lease, debt repayment, bank fees and alimony to a new Finances group.
  Transport, Medical costs and Other expenses are variable only.
- The Plan grid shows one row per variable group; an opened group on the
  Overview lists its categories with what each spent.

### Removed

- The Left to spend tab: the Overview shows the month against the budget.
- Budgets of a variable category's own, and the rest shared within a group.

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

[2.3.0]: https://github.com/mikepage/fin/releases/tag/v2.3.0
[2.2.0]: https://github.com/mikepage/fin/releases/tag/v2.2.0
[2.1.4]: https://github.com/mikepage/fin/releases/tag/v2.1.4
[2.1.3]: https://github.com/mikepage/fin/releases/tag/v2.1.3
[2.1.2]: https://github.com/mikepage/fin/releases/tag/v2.1.2
[2.1.1]: https://github.com/mikepage/fin/releases/tag/v2.1.1
[2.1.0]: https://github.com/mikepage/fin/releases/tag/v2.1.0
[2.0.0]: https://github.com/mikepage/fin/releases/tag/v2.0.0
