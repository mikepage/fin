//! Persistence (one JSON file, written atomically) and the operations on the dataset.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use fin_shared::{
    format_cents_in, months_ending, normalize_iban, normalize_rule, rule_matches, Account, AccountBalance, Contract, Lang, RuleKind, UNSORTED_CATEGORY_ID, BackupInfo, Budget, Category, CategoryKind, Dataset, GroupBudget, ImportFileStat, ImportRecord,
    Transaction, TransactionInput,
};
use serde::{Deserialize, Serialize};

use fin_shared::catalog;
use fin_shared::match_form;

use crate::locale::{self, Locale};
use crate::{backup, camt};

const FORMAT_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct FileFormat {
    version: u32,
    data: Dataset,
}

/// What the data file looked like when last read or written (modified time, size).
type Stamp = Option<(std::time::SystemTime, u64)>;

fn stamp(path: &Path) -> Stamp {
    fs::metadata(path).ok().and_then(|m| Some((m.modified().ok()?, m.len())))
}

pub struct Store {
    path: PathBuf,
    pub data: Dataset,
    stamp: Stamp,
}

impl Store {
    /// Loads the data file, or starts with default categories when there is none yet.
    pub fn load(path: PathBuf) -> Result<Self, String> {
        let stamp = stamp(&path);
        let data = read_data(&path)?;
        Ok(Self { path, data, stamp })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Reads the file again when something else (the CLI, or the app) wrote it since.
    /// Returns whether it did.
    pub fn reload_if_changed(&mut self) -> Result<bool, String> {
        let now = stamp(&self.path);
        if now == self.stamp {
            return Ok(false);
        }
        self.data = read_data(&self.path)?;
        self.stamp = now;
        Ok(true)
    }

    /// Applies `f` to a copy of the data and saves it. Memory only changes if the save succeeds.
    /// The first save of each (UTC) day first keeps a copy of the file as it was.
    pub fn mutate<R>(&mut self, f: impl FnOnce(&mut Dataset) -> Result<R, String>) -> Result<R, String> {
        // Never save over changes written by another process.
        self.reload_if_changed()?;
        let mut next = self.data.clone();
        let r = f(&mut next)?;
        let now = backup::now_secs();
        if !backup::has_daily_for(&self.path, now) {
            backup::create(&self.path, backup::DAILY, now)?;
        }
        self.write(next)?;
        Ok(r)
    }

    fn write(&mut self, data: Dataset) -> Result<(), String> {
        write_atomic(&self.path, &data)?;
        self.data = data;
        self.stamp = stamp(&self.path);
        Ok(())
    }

    pub fn backup(&self, reason: &str) -> Result<Option<BackupInfo>, String> {
        backup::create(&self.path, reason, backup::now_secs())
    }

    pub fn list_backups(&self) -> Result<Vec<BackupInfo>, String> {
        backup::list(&self.path)
    }

    /// Replaces the data with a backup from the list, after backing up the current state.
    pub fn restore_backup(&mut self, name: &str) -> Result<(), String> {
        let bytes = backup::read(&self.path, name)?;
        self.restore_bytes(&bytes)
    }

    /// Replaces the data with the contents of a backup file (JSON as saved by Fin).
    pub fn restore_bytes(&mut self, bytes: &[u8]) -> Result<(), String> {
        let mut data = parse_file(bytes)?;
        ensure_system_categories(&mut data);
        self.backup(backup::BEFORE_RESTORE)?;
        self.write(data)
    }

    /// The current data in the file format, for exporting a copy.
    pub fn export_json(&self) -> Result<Vec<u8>, String> {
        serde_json::to_vec_pretty(&FileFormat { version: FORMAT_VERSION, data: self.data.clone() })
            .map_err(|e| e.to_string())
    }
}

fn read_data(path: &Path) -> Result<Dataset, String> {
    let mut data = match fs::read(path) {
        Ok(bytes) => parse_file(&bytes).map_err(|e| format!("Data file unreadable: {e}"))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Dataset::default(),
        Err(e) => return Err(format!("Cannot read {}: {e}", path.display())),
    };
    ensure_system_categories(&mut data);
    Ok(data)
}

fn parse_file(bytes: &[u8]) -> Result<Dataset, String> {
    let file: FileFormat = serde_json::from_slice(bytes).map_err(|e| format!("Not a valid Fin file: {e}"))?;
    if file.version != FORMAT_VERSION {
        return Err(format!("Unknown file version {}", file.version));
    }
    Ok(file.data)
}

fn write_atomic(path: &Path, data: &Dataset) -> Result<(), String> {
    let err = |e: std::io::Error| format!("Save failed: {e}");
    let dir = path.parent().ok_or("Invalid path")?;
    fs::create_dir_all(dir).map_err(err)?;
    let json = serde_json::to_vec_pretty(&FileFormat { version: FORMAT_VERSION, data: data.clone() })
        .map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    let mut f = File::create(&tmp).map_err(err)?;
    f.write_all(&json).map_err(err)?;
    f.sync_all().map_err(err)?;
    drop(f);
    fs::rename(&tmp, path).map_err(err)?;
    // Persist the rename itself.
    File::open(dir).and_then(|d| d.sync_all()).map_err(err)?;
    Ok(())
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Adds the locale's default rules a file hasn't had yet (those newer than its version),
/// so a default the user removed doesn't come back. A pattern that already lives in some
/// category (the user's own choice) is left where it is.
fn ensure_default_rules(ds: &mut Dataset, l: &Locale) {
    let have = ds.default_rules_version;
    let latest = l.rules_version();
    if have >= latest {
        return;
    }
    for r in l.rules.iter().filter(|r| r.since > have) {
        let (id, kind) = (r.category_id(), r.kind());
        for p in &r.patterns {
            let form = match_form(p);
            if ds.categories.iter().any(|c| c.rules(kind).iter().any(|x| match_form(x) == form)) {
                continue;
            }
            if let Some(c) = ds.categories.iter_mut().find(|c| c.id == id) {
                c.rules_mut(kind).push(p.clone());
            }
        }
    }
    ds.default_rules_version = latest;
    // New defaults also sort what is still waiting in "To categorise".
    categorize_unsorted(ds);
}

/// Runs the rules over every transaction still to be categorised; returns how many got a
/// category. Categorised transactions are left alone.
pub fn categorize_unsorted(ds: &mut Dataset) -> usize {
    let found: Vec<(usize, String)> = ds
        .transactions
        .iter()
        .enumerate()
        .filter(|(_, t)| t.is_unsorted())
        .filter_map(|(i, t)| {
            let c = ds.category_by_rules(&t.description, t.counterparty_iban.as_deref(), t.amount_cents)?;
            (c.id != UNSORTED_CATEGORY_ID).then(|| (i, c.id.clone()))
        })
        .collect();
    for (i, id) in &found {
        ds.transactions[*i].category_id = Some(id.clone());
    }
    found.len()
}

/// Makes the standard categories in a data file match the catalog and the mapping of
/// the file's language: every one exists and is marked system, with the catalog's kind
/// and the locale's name and group (so renames and a change of language reach existing
/// files). Missing ones are added at the end of their group. The user's own categories
/// in a standard group move along with its name. A disabled category that still holds
/// transactions is switched back on, and new default rules are added (always the nl-NL
/// ones: they match Dutch bank descriptions).
fn ensure_system_categories(ds: &mut Dataset) {
    let l = locale::names(ds.language);
    for c in ds.categories.iter_mut() {
        if c.disabled && ds.transactions.iter().any(|t| t.category_id.as_deref() == Some(c.id.as_str())) {
            c.disabled = false;
        }
    }
    for &(id, group_key, kind) in catalog::CATALOG {
        let (name, group) = (l.category_name(id), l.group_name(group_key));
        if let Some(c) = ds.categories.iter_mut().find(|c| c.id == id) {
            c.system = true;
            c.kind = kind;
            c.name = name.to_string();
            c.group = group.to_string();
            continue;
        }
        let cat = Category {
            id: id.to_string(),
            name: name.to_string(),
            group: group.to_string(),
            kind,
            system: true,
            ..Default::default()
        };
        match ds.categories.iter().rposition(|c| c.group == group) {
            Some(i) => ds.categories.insert(i + 1, cat),
            None => ds.categories.push(cat),
        }
    }
    follow_group_names(ds, l);
    ensure_default_rules(ds, locale::nl_nl());
}

/// The user's own categories in a standard group get that group's name in the current
/// language (the Dutch "Huishouden" becomes "Household"), so they stay with the
/// standard categories.
fn follow_group_names(ds: &mut Dataset, l: &Locale) {
    let rename = |group: &mut String| {
        if let Some(key) = Lang::ALL.into_iter().find_map(|lang| locale::names(lang).group_key(group)) {
            *group = l.group_name(key).to_string();
        }
    };
    for c in ds.categories.iter_mut().filter(|c| !c.system) {
        rename(&mut c.group);
    }
    // Group budgets go by the group's name, so they follow too.
    for g in ds.group_budgets.iter_mut() {
        rename(&mut g.group);
    }
}

/// Switches the app's language: the standard category and group names follow.
pub fn set_language(ds: &mut Dataset, lang: Lang) -> Result<(), String> {
    ds.language = lang;
    ensure_system_categories(ds);
    Ok(())
}

#[cfg(test)]
fn default_dataset() -> Dataset {
    let mut ds = Dataset::default();
    ensure_system_categories(&mut ds);
    ds
}

fn clean_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Name cannot be empty".into());
    }
    Ok(name.to_string())
}

fn clean_iban(iban: Option<String>) -> Option<String> {
    // Any whitespace: IBANs copied from a bank site often carry non-breaking spaces.
    iban.map(|i| i.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_uppercase()).filter(|i| !i.is_empty())
}

fn valid_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let (Ok(m), Ok(d)) = (s[5..7].parse::<u32>(), s[8..10].parse::<u32>()) else { return false };
    s[..4].bytes().all(|c| c.is_ascii_digit()) && (1..=12).contains(&m) && (1..=31).contains(&d)
}

pub fn add_account(ds: &mut Dataset, name: &str, iban: Option<String>) -> Result<(), String> {
    ds.accounts.push(Account { id: new_id(), name: clean_name(name)?, iban: clean_iban(iban), balances: Vec::new() });
    Ok(())
}

/// Imports a bank's account balances CSV: per line `DD-MM-YYYY,IBAN,name,currency,amount`
/// (amount with a decimal point), the balance at the end of that day. Each goes to the
/// account with that IBAN, replacing one on the same date. Returns how many were stored.
/// Nothing is stored when a line is wrong or an IBAN unknown.
pub fn import_balances(ds: &mut Dataset, text: &str) -> Result<usize, String> {
    let mut found = Vec::new();
    for (n, line) in text.lines().enumerate().filter(|(_, l)| !l.trim().is_empty()) {
        let cols: Vec<&str> = line.split(',').map(str::trim).collect();
        let bad = || format!("Line {}: not a balance (date,IBAN,name,currency,amount)", n + 1);
        if cols.len() < 5 {
            return Err(bad());
        }
        let date = match cols[0].split('-').collect::<Vec<_>>()[..] {
            [d, m, y] if y.len() == 4 => format!("{y}-{m:0>2}-{d:0>2}"),
            _ => return Err(bad()),
        };
        if !valid_date(&date) {
            return Err(bad());
        }
        let iban = normalize_iban(cols[1]).ok_or_else(bad)?;
        let amount = cols[cols.len() - 1];
        let cents = amount
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
            .map(|v| (v * 100.0).round() as i64)
            .ok_or_else(|| format!("Line {}: invalid amount {}", n + 1, amount))?;
        let id = ds
            .accounts
            .iter()
            .find(|a| a.iban.as_deref() == Some(iban.as_str()))
            .map(|a| a.id.clone())
            .ok_or_else(|| format!("No account with IBAN {iban}; add it first"))?;
        found.push((id, AccountBalance { date, cents }));
    }
    let count = found.len();
    for (id, b) in found {
        let a = ds.accounts.iter_mut().find(|a| a.id == id).expect("found above");
        a.balances.retain(|x| x.date != b.date);
        a.balances.push(b);
        a.balances.sort_by(|x, y| x.date.cmp(&y.date));
    }
    Ok(count)
}

/// Sets an account's bank balance at the end of `date` (replacing one on that date);
/// `None` removes it.
pub fn set_account_balance(ds: &mut Dataset, id: &str, date: &str, cents: Option<i64>) -> Result<(), String> {
    if !valid_date(date) {
        return Err("Invalid date".into());
    }
    let a = ds.accounts.iter_mut().find(|a| a.id == id).ok_or("Account not found")?;
    a.balances.retain(|b| b.date != date);
    if let Some(cents) = cents {
        a.balances.push(AccountBalance { date: date.to_string(), cents });
        a.balances.sort_by(|x, y| x.date.cmp(&y.date));
    }
    Ok(())
}

/// Adds a contract (empty id) or replaces the one with its id.
pub fn save_contract(ds: &mut Dataset, mut c: Contract) -> Result<String, String> {
    c.name = clean_name(&c.name)?;
    c.supplier = c.supplier.trim().to_string();
    for d in [&c.start, &c.end] {
        if !d.is_empty() && !valid_date(d) {
            return Err("Invalid date".into());
        }
    }
    // Net metering ended on 1 January 2027: a contract running past it has none.
    let netting_allowed = c.netting_allowed();
    if let Some(e) = c.energy.as_mut() {
        e.netting &= netting_allowed && e.utility == fin_shared::Utility::Electricity;
    }
    if let Some(e) = &c.energy {
        let rates = [
            e.supply_normal, e.supply_low, e.energy_tax, e.feed_in, e.feed_in_cost_month, e.feed_in_cost_kwh,
            e.fixed_month, e.network_year, e.tax_reduction_year, e.vat,
        ];
        if rates.iter().any(|r| !r.is_finite()) {
            return Err("Invalid tariff".into());
        }
    }
    for p in &c.usage {
        let kwh = [p.used_normal, p.used_low, p.returned_normal, p.returned_low];
        if !valid_date(&p.start) || !valid_date(&p.end) || p.end < p.start || kwh.iter().any(|k| !k.is_finite() || *k < 0.0) {
            return Err("Invalid usage period".into());
        }
    }
    c.usage.sort_by(|a, b| a.start.cmp(&b.start));
    if c.settlements.iter().any(|s| !valid_date(&s.date)) || (!c.bill_due.is_empty() && !valid_date(&c.bill_due)) {
        return Err("Invalid date".into());
    }
    c.settlements.sort_by(|a, b| a.date.cmp(&b.date));
    if c.id.is_empty() {
        c.id = new_id();
        let id = c.id.clone();
        ds.contracts.push(c);
        return Ok(id);
    }
    let slot = ds.contracts.iter_mut().find(|x| x.id == c.id).ok_or("Contract not found")?;
    let id = c.id.clone();
    *slot = c;
    Ok(id)
}

pub fn delete_contract(ds: &mut Dataset, id: &str) -> Result<(), String> {
    let before = ds.contracts.len();
    ds.contracts.retain(|c| c.id != id);
    if ds.contracts.len() == before {
        return Err("Contract not found".into());
    }
    Ok(())
}

pub fn update_account(ds: &mut Dataset, account: Account) -> Result<(), String> {
    let a = ds.accounts.iter_mut().find(|a| a.id == account.id).ok_or("Account not found")?;
    a.name = clean_name(&account.name)?;
    a.iban = clean_iban(account.iban);
    Ok(())
}

pub fn delete_account(ds: &mut Dataset, id: &str) -> Result<(), String> {
    if ds.transactions.iter().any(|t| t.account_id == id) {
        return Err("Account still has transactions".into());
    }
    let before = ds.accounts.len();
    ds.accounts.retain(|a| a.id != id);
    if ds.accounts.len() == before {
        return Err("Account not found".into());
    }
    Ok(())
}

/// New categories go at the end of their group, so the list stays grouped.
pub fn add_category(ds: &mut Dataset, name: &str, group: &str, kind: CategoryKind) -> Result<(), String> {
    if !CategoryKind::SELECTABLE.contains(&kind) {
        return Err("Choose a kind: income, extra income, fixed costs, variable or investment".into());
    }
    let group = group.trim().to_string();
    check_group_kind(ds, None, &group, kind)?;
    let cat = Category { id: new_id(), name: clean_name(name)?, group, kind, ..Default::default() };
    match ds.categories.iter().rposition(|c| c.group == cat.group) {
        Some(i) => ds.categories.insert(i + 1, cat),
        None => ds.categories.push(cat),
    }
    Ok(())
}

/// System categories keep their name, group and kind; only custom categories can change
/// kind, between income, fixed and variable (the transfer kind belongs to "Internal
/// transfers" alone). A category can only be disabled when no transaction uses it.
/// Rules are managed with add/remove_category_rule, not here. Moving to another group
/// or kind (or switching a custom category back on) has to respect the group's kind:
/// see check_group_kind.
pub fn update_category(ds: &mut Dataset, category: Category) -> Result<(), String> {
    let i = ds.categories.iter().position(|c| c.id == category.id).ok_or("Category not found")?;
    let before = ds.categories[i].clone();
    apply_category_update(ds, i, category)?;
    let c = &ds.categories[i];
    let moved = c.group != before.group || c.kind != before.kind || (before.disabled && !c.disabled && !c.system);
    if moved && !c.disabled {
        let (id, group, kind) = (c.id.clone(), c.group.clone(), c.kind);
        if let Err(e) = check_group_kind(ds, Some(&id), &group, kind) {
            ds.categories[i] = before;
            return Err(e);
        }
    }
    Ok(())
}

/// A group holds fixed or variable costs, never both: fixed costs are budgeted per
/// category, variable costs per group. Other kinds (income, extra income, investments)
/// can sit alongside either. Only switched-on categories count; `except` is the
/// category being changed. Categories without a group aren't a group.
fn check_group_kind(ds: &Dataset, except: Option<&str>, group: &str, kind: CategoryKind) -> Result<(), String> {
    if group.is_empty() {
        return Ok(());
    }
    let holds = |k: CategoryKind| {
        ds.categories.iter().any(|c| Some(c.id.as_str()) != except && !c.disabled && c.group == group && c.kind == k)
    };
    match kind {
        CategoryKind::Fixed if holds(CategoryKind::Variable) => {
            Err(format!("{group} holds variable costs: a fixed cost needs a group of fixed costs"))
        }
        CategoryKind::Variable if holds(CategoryKind::Fixed) => {
            Err(format!("{group} holds fixed costs: a variable cost needs a group of variable costs"))
        }
        _ => Ok(()),
    }
}

fn apply_category_update(ds: &mut Dataset, i: usize, category: Category) -> Result<(), String> {
    let in_use = ds.transactions.iter().filter(|t| t.category_id.as_deref() == Some(category.id.as_str())).count();
    let c = &mut ds.categories[i];
    if !c.system {
        c.name = clean_name(&category.name)?;
        c.group = category.group.trim().to_string();
    }
    if c.kind != category.kind {
        if c.system {
            return Err("A standard category's kind is fixed".into());
        }
        if !CategoryKind::SELECTABLE.contains(&category.kind) {
            return Err("Choose a kind: income, extra income, fixed costs, variable or investment".into());
        }
        c.kind = category.kind;
    }
    if category.disabled && !c.disabled && in_use > 0 {
        return Err(if in_use == 1 {
            format!("{} still has 1 transaction. Move it to another category first, then you can disable the category.", c.name)
        } else {
            format!("{} still has {in_use} transactions. Move them to another category first, then you can disable the category.", c.name)
        });
    }
    c.disabled = category.disabled;
    Ok(())
}

/// Deleting a category leaves its transactions uncategorised and drops its budgets.
/// System categories can only be disabled.
pub fn delete_category(ds: &mut Dataset, id: &str) -> Result<(), String> {
    match ds.categories.iter().find(|c| c.id == id) {
        None => return Err("Category not found".into()),
        Some(c) if c.system => {
            return Err("Standard categories cannot be deleted, only disabled".into())
        }
        Some(_) => {}
    }
    ds.categories.retain(|c| c.id != id);
    for t in ds.transactions.iter_mut().filter(|t| t.category_id.as_deref() == Some(id)) {
        t.category_id = None;
    }
    ds.budgets.retain(|b| b.category_id != id);
    Ok(())
}

fn valid_month(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 7
        && b[4] == b'-'
        && s[..4].bytes().all(|c| c.is_ascii_digit())
        && s[5..].bytes().all(|c| c.is_ascii_digit())
        && s[5..].parse::<u32>().is_ok_and(|m| (1..=12).contains(&m))
}

/// `YYYY-MM` of the month after a valid `month`; December rolls over to January.
fn next_month(month: &str) -> Result<String, String> {
    let (y, m): (u32, u32) = (month[..4].parse().unwrap_or(0), month[5..].parse().unwrap_or(0));
    let (y, m) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    if y > 9999 {
        return Err("Invalid month".into());
    }
    Ok(format!("{y:04}-{m:02}"))
}

/// The budgeted result of a year: the income budgets minus the fixed and investment
/// budgets of the switched-on categories and the variable group budgets. A variable
/// category's own budget left in a file doesn't count.
pub fn budget_year_result(ds: &Dataset, year: &str) -> i64 {
    let categories: i64 = ds
        .budgets
        .iter()
        .filter(|b| b.month.starts_with(year))
        .filter_map(|b| {
            ds.category(Some(&b.category_id))
                .filter(|c| ds.own_budget(c, &b.month).is_some())
                .map(|c| (c.kind, b.amount_cents))
        })
        .map(|(kind, a)| match kind {
            CategoryKind::Income => a,
            k if k.is_expense() => -a,
            _ => 0,
        })
        .sum();
    let groups: i64 = ds
        .group_budgets
        .iter()
        .filter(|g| g.month.starts_with(year) && ds.group_budget_counts(g))
        .map(|g| g.amount_cents)
        .sum();
    categories - groups
}

/// Keeps a year's budget within its budgeted income (years without any income budget
/// aren't checked). `old` and `new` are what the change counts for in the year result,
/// `sign` +1 for income and -1 for spending: a change that would make the budgeted
/// year result negative (or more negative) is capped to what still fits. Lowering
/// spending or raising income is always allowed. Errors when nothing fits at all.
fn cap_to_year(ds: &Dataset, month: &str, sign: i64, old: i64, new: i64) -> Result<i64, String> {
    // Only a year with budgeted income has something to stay within.
    let year = &month[..4];
    let has_income = ds.budgets.iter().any(|b| {
        b.month.starts_with(year) && ds.category(Some(&b.category_id)).is_some_and(|c| c.kind == CategoryKind::Income && !c.disabled)
    });
    if !has_income {
        return Ok(new);
    }
    let before = budget_year_result(ds, year);
    let after = before + sign * (new - old);
    if after >= 0 || after >= before {
        return Ok(new);
    }
    // What still fits: the room left this year (none when the year is already short).
    let room = before.max(0);
    let capped = if sign < 0 { old + room } else { old - room };
    if capped == old {
        // English notation; the app shows the amount in its own (see i18n::error).
        return Err(format!(
            "Does not fit in the {} budget: the year result would be € {}. Lower another budget first.",
            &month[..4],
            format_cents_in(after, Lang::En)
        ));
    }
    Ok(capped)
}

/// Sets or replaces an income or fixed category's budget. `None` or 0 removes it. The
/// year result has to fit. Variable categories are budgeted per group
/// (set_group_budget); removing a budget of theirs left in a file is still allowed.
pub fn set_budget(ds: &mut Dataset, category_id: &str, month: &str, amount_cents: Option<i64>) -> Result<(), String> {
    if !valid_month(month) {
        return Err("Invalid month".into());
    }
    let cat = ds.categories.iter().find(|c| c.id == category_id).ok_or("Category not found")?;
    let amount = amount_cents.filter(|&a| a != 0);
    if amount.is_some_and(|a| a < 0) {
        return Err("A budget cannot be negative".into());
    }
    if amount.is_some() && cat.kind == CategoryKind::Variable {
        return Err(format!("Variable categories are budgeted per group: set the budget on {}", cat.group));
    }
    let old = ds.own_budget(cat, month).unwrap_or(0);
    let amount = match cat.kind {
        _ if cat.disabled => amount,
        CategoryKind::Variable => None,
        CategoryKind::Income => Some(cap_to_year(ds, month, 1, old, amount.unwrap_or(0))?).filter(|a| *a != 0),
        k if k.is_expense() => Some(cap_to_year(ds, month, -1, old, amount.unwrap_or(0))?).filter(|a| *a != 0),
        _ => amount,
    };
    let existing = ds.budgets.iter().position(|b| b.category_id == category_id && b.month == month);
    match (existing, amount) {
        (Some(i), Some(a)) => ds.budgets[i].amount_cents = a,
        (Some(i), None) => {
            ds.budgets.remove(i);
        }
        (None, Some(a)) => ds.budgets.push(Budget {
            category_id: category_id.to_string(),
            month: month.to_string(),
            amount_cents: a,
        }),
        (None, None) => {}
    }
    Ok(())
}

/// Sets or replaces the budget of a group's variable categories together: variable
/// costs are budgeted per group, fixed costs and income per category. `None` or 0
/// removes it (also one of another kind left in a file). The year result has to fit as
/// with a category budget.
pub fn set_group_budget(ds: &mut Dataset, group: &str, kind: CategoryKind, month: &str, amount_cents: Option<i64>) -> Result<(), String> {
    if !valid_month(month) {
        return Err("Invalid month".into());
    }
    let amount = amount_cents.filter(|&a| a != 0);
    if amount.is_some_and(|a| a < 0) {
        return Err("A budget cannot be negative".into());
    }
    if amount.is_some() {
        if kind != CategoryKind::Variable {
            return Err("Group budgets are for variable costs".into());
        }
        if !ds.categories.iter().any(|c| !c.disabled && c.group == group && c.kind == kind) {
            return Err(format!("{group} has no categories of that kind"));
        }
    }
    let old = ds.group_budget_for(group, kind, month).unwrap_or(0);
    let new = cap_to_year(ds, month, -1, old, amount.unwrap_or(0))?;
    let amount = Some(new).filter(|a| *a != 0);
    let existing = ds.group_budgets.iter().position(|b| b.group == group && b.kind == kind && b.month == month);
    match (existing, amount) {
        (Some(i), Some(a)) => ds.group_budgets[i].amount_cents = a,
        (Some(i), None) => {
            ds.group_budgets.remove(i);
        }
        (None, Some(a)) => {
            ds.group_budgets.push(GroupBudget { group: group.to_string(), kind, month: month.to_string(), amount_cents: a })
        }
        (None, None) => {}
    }
    Ok(())
}

/// Copies every budget of `year - 1` into the same month of `year`, but only where
/// that category (or group) has no budget yet in that month. Variable categories'
/// own budgets left in a file and group budgets of another kind aren't copied: those
/// don't count. Returns how many were copied.
pub fn copy_budgets_from_previous_year(ds: &mut Dataset, year: i32) -> Result<usize, String> {
    if !(1001..=9999).contains(&year) {
        return Err("Invalid year".into());
    }
    let prefix = format!("{:04}-", year - 1);
    let to_copy: Vec<Budget> = ds
        .budgets
        .iter()
        .filter(|b| b.month.starts_with(&prefix))
        .filter(|b| ds.category(Some(&b.category_id)).is_some_and(|c| c.kind != CategoryKind::Variable))
        .map(|b| Budget { month: format!("{year:04}{}", &b.month[4..]), ..b.clone() })
        .filter(|b| ds.budget_for(&b.category_id, &b.month).is_none())
        .collect();
    let groups: Vec<GroupBudget> = ds
        .group_budgets
        .iter()
        .filter(|g| g.month.starts_with(&prefix) && g.kind == CategoryKind::Variable)
        .map(|g| GroupBudget { month: format!("{year:04}{}", &g.month[4..]), ..g.clone() })
        .filter(|g| !ds.group_budgets.iter().any(|x| x.group == g.group && x.kind == g.kind && x.month == g.month))
        .collect();
    let copied = to_copy.len() + groups.len();
    ds.budgets.extend(to_copy);
    ds.group_budgets.extend(groups);
    Ok(copied)
}

/// Sets every month of `year` to each category's average over the `months_back` months
/// ending with `last_month` (see Dataset::average_per_month); variable categories are
/// budgeted per group, so each group of them gets the sum of their averages. With
/// `overwrite` false only months without a budget are filled. Returns how many budgets
/// were set.
pub fn budgets_from_average(
    ds: &mut Dataset,
    year: i32,
    last_month: &str,
    months_back: u32,
    overwrite: bool,
) -> Result<usize, String> {
    if !(1001..=9999).contains(&year) {
        return Err("Invalid year".into());
    }
    if !valid_month(last_month) || !(1..=24).contains(&months_back) {
        return Err("Invalid period".into());
    }
    let mut averages = ds.average_per_month(&months_ending(last_month, months_back as usize));
    // The variable categories' averages, summed per group.
    let mut groups: Vec<(String, i64)> = Vec::new();
    averages.retain(|(id, cents)| match ds.category(Some(id)) {
        Some(c) if c.kind == CategoryKind::Variable => {
            match groups.iter_mut().find(|(g, _)| *g == c.group) {
                Some((_, sum)) => *sum += cents,
                None => groups.push((c.group.clone(), *cents)),
            }
            false
        }
        _ => true,
    });
    let mut set = 0;
    for (group, cents) in groups {
        for m in 1..=12 {
            let month = format!("{year:04}-{m:02}");
            let kind = CategoryKind::Variable;
            match ds.group_budgets.iter_mut().find(|g| g.group == group && g.kind == kind && g.month == month) {
                Some(g) if overwrite && g.amount_cents != cents => {
                    g.amount_cents = cents;
                    set += 1;
                }
                Some(_) => {}
                None => {
                    ds.group_budgets.push(GroupBudget { group: group.clone(), kind, month, amount_cents: cents });
                    set += 1;
                }
            }
        }
    }
    for (category_id, cents) in averages {
        for m in 1..=12 {
            let month = format!("{year:04}-{m:02}");
            match ds.budgets.iter_mut().find(|b| b.category_id == category_id && b.month == month) {
                Some(b) if overwrite && b.amount_cents != cents => {
                    b.amount_cents = cents;
                    set += 1;
                }
                Some(_) => {}
                None => {
                    ds.budgets.push(Budget { category_id: category_id.clone(), month, amount_cents: cents });
                    set += 1;
                }
            }
        }
    }
    Ok(set)
}

/// Copies all budgets of `month` into the following month, overwriting the values
/// there for those categories and groups. Budgets of others in the next month stay.
pub fn copy_budget_month_to_next(ds: &mut Dataset, month: &str) -> Result<(), String> {
    if !valid_month(month) {
        return Err("Invalid month".into());
    }
    let next = next_month(month)?;
    let groups: Vec<(String, CategoryKind, i64)> = ds
        .group_budgets
        .iter()
        .filter(|g| g.month == month && ds.group_budget_counts(g))
        .map(|g| (g.group.clone(), g.kind, g.amount_cents))
        .collect();
    // The categories' own budgets (not a variable one's left in a file), income first
    // so the spending after it fits in the year.
    let mut source: Vec<(String, i64, bool)> = ds
        .budgets
        .iter()
        .filter(|b| b.month == month)
        .filter_map(|b| {
            let c = ds.category(Some(&b.category_id)).filter(|c| c.kind != CategoryKind::Variable)?;
            Some((b.category_id.clone(), b.amount_cents, c.kind == CategoryKind::Income))
        })
        .collect();
    source.sort_by_key(|(_, _, income)| !income);
    for (category_id, amount, _) in source {
        set_budget(ds, &category_id, &next, Some(amount))?;
    }
    for (group, kind, amount) in groups {
        set_group_budget(ds, &group, kind, &next, Some(amount))?;
    }
    Ok(())
}

pub fn save_transaction(ds: &mut Dataset, input: TransactionInput) -> Result<(), String> {
    if !valid_date(&input.date) {
        return Err("Invalid date".into());
    }
    if !ds.accounts.iter().any(|a| a.id == input.account_id) {
        return Err("Account not found".into());
    }
    let category_id = input.category_id.filter(|c| !c.is_empty());
    if let Some(c) = &category_id {
        let cat = ds.categories.iter().find(|x| &x.id == c).ok_or("Category not found")?;
        // Disabled categories hold no transactions.
        if cat.disabled {
            return Err(format!("Category {} is disabled", cat.name));
        }
    }
    let description = input.description.trim().to_string();
    match input.id {
        Some(id) => {
            let t = ds.transactions.iter_mut().find(|t| t.id == id).ok_or("Transaction not found")?;
            t.date = input.date;
            t.description = description;
            t.amount_cents = input.amount_cents;
            t.category_id = category_id;
            t.account_id = input.account_id;
        }
        None => ds.transactions.push(Transaction {
            id: new_id(),
            date: input.date,
            description,
            amount_cents: input.amount_cents,
            category_id,
            account_id: input.account_id,
            ..Default::default()
        }),
    }
    sort_transactions(ds);
    Ok(())
}

/// The category a rule may point to: existing, switched on and not "To categorise".
fn rule_target<'a>(ds: &'a Dataset, category_id: &str) -> Result<&'a Category, String> {
    let cat = ds.categories.iter().find(|c| c.id == category_id).ok_or("Category not found")?;
    if cat.disabled {
        return Err(format!("Category {} is disabled", cat.name));
    }
    if cat.id == UNSORTED_CATEGORY_ID {
        return Err(format!("A rule to \"{}\" makes no sense", cat.name));
    }
    Ok(cat)
}

/// Adds a rule to a category (moving it away from another category if needed) and puts
/// every matching transaction that is still to be categorised in the category.
/// Returns how many transactions it filled in.
pub fn add_category_rule(ds: &mut Dataset, category_id: &str, kind: RuleKind, value: &str) -> Result<usize, String> {
    let Some(value) = normalize_rule(kind, value) else {
        return Err(match kind {
            RuleKind::Iban => "Invalid IBAN".into(),
            RuleKind::Text | RuleKind::TextIn | RuleKind::TextOut => "A text rule needs at least 3 characters".into(),
        });
    };
    rule_target(ds, category_id)?;
    for c in ds.categories.iter_mut() {
        let is_target = c.id == category_id;
        let list = c.rules_mut(kind);
        if is_target {
            if !list.contains(&value) {
                list.push(value.clone());
            }
        } else {
            list.retain(|v| *v != value);
        }
    }
    let mut applied = 0;
    for t in ds.transactions.iter_mut() {
        if t.is_unsorted() && rule_matches(kind, &value, t) {
            t.category_id = Some(category_id.to_string());
            applied += 1;
        }
    }
    Ok(applied)
}

/// Puts every transaction the rule matches dated on or after `from` (`YYYY-MM-DD`) in
/// the category, also ones that had another category. Returns how many changed.
pub fn apply_category_rule(
    ds: &mut Dataset,
    category_id: &str,
    kind: RuleKind,
    value: &str,
    from: &str,
) -> Result<usize, String> {
    let value = normalize_rule(kind, value).ok_or("Invalid rule")?;
    if !valid_date(from) {
        return Err("Invalid date".into());
    }
    rule_target(ds, category_id)?;
    let mut changed = 0;
    for t in ds.transactions.iter_mut() {
        if rule_matches(kind, &value, t) && t.date.as_str() >= from && t.category_id.as_deref() != Some(category_id) {
            t.category_id = Some(category_id.to_string());
            changed += 1;
        }
    }
    Ok(changed)
}

/// Whether a rule is one of the locale's defaults for that category (not one the user
/// added, and not a default the user moved to another category).
pub fn is_default_rule(category_id: &str, kind: RuleKind, value: &str) -> bool {
    let form = match_form(value);
    locale::nl_nl()
        .rules
        .iter()
        .any(|r| r.category_id() == category_id && r.kind() == kind && r.patterns.iter().any(|p| match_form(p) == form))
}

/// Every rule, category by category in list order; defaults first within a category.
pub fn list_rules(ds: &Dataset) -> Vec<fin_shared::RuleInfo> {
    let mut out = Vec::new();
    for c in &ds.categories {
        let start = out.len();
        for kind in RuleKind::ALL {
            for v in c.rules(kind) {
                let default = is_default_rule(&c.id, kind, v);
                out.push(fin_shared::RuleInfo { category_id: c.id.clone(), kind, value: v.clone(), default });
            }
        }
        out[start..].sort_by_key(|r| !r.default);
    }
    out
}

/// Removes a personal rule. Default rules stay: override one with a personal rule instead.
pub fn remove_category_rule(ds: &mut Dataset, category_id: &str, kind: RuleKind, value: &str) -> Result<(), String> {
    if is_default_rule(category_id, kind, value) {
        return Err("A default rule cannot be removed; add your own rule to override it".into());
    }
    let c = ds.categories.iter_mut().find(|c| c.id == category_id).ok_or("Category not found")?;
    let list = c.rules_mut(kind);
    let before = list.len();
    list.retain(|v| v != value);
    if list.len() == before {
        return Err("Rule not found".into());
    }
    Ok(())
}

/// Splits a transaction: each part (amount without sign, category) counts in its own
/// category, the rest stays where it is. An empty list removes the split. The parts
/// take the transaction's sign and together may not exceed its amount.
pub fn set_splits(ds: &mut Dataset, id: &str, parts: Vec<(i64, String)>) -> Result<(), String> {
    let mut splits = Vec::new();
    for (amount, category_id) in parts {
        if amount <= 0 {
            return Err("Each part needs an amount".into());
        }
        let cat = ds.categories.iter().find(|c| c.id == category_id).ok_or("Category not found")?;
        if cat.disabled {
            return Err(format!("Category {} is disabled", cat.name));
        }
        splits.push(fin_shared::Split { amount_cents: amount, category_id });
    }
    let t = ds.transactions.iter_mut().find(|t| t.id == id).ok_or("Transaction not found")?;
    let sign = if t.amount_cents < 0 { -1 } else { 1 };
    if splits.iter().map(|s| s.amount_cents).sum::<i64>() > t.amount_cents.abs() {
        return Err("The parts add up to more than the amount".into());
    }
    for s in splits.iter_mut() {
        s.amount_cents *= sign;
    }
    t.splits = splits;
    Ok(())
}

/// Sets the day a transaction counts on (`None`, or the bank date itself, clears it).
pub fn set_counts_on(ds: &mut Dataset, id: &str, day: Option<String>) -> Result<(), String> {
    let day = day.map(|d| d.trim().to_string()).filter(|d| !d.is_empty());
    if day.as_deref().is_some_and(|d| !valid_date(d)) {
        return Err("Invalid date".into());
    }
    let t = ds.transactions.iter_mut().find(|t| t.id == id).ok_or("Transaction not found")?;
    t.counts_on = day.filter(|d| *d != t.date);
    Ok(())
}

pub fn delete_transaction(ds: &mut Dataset, id: &str) -> Result<(), String> {
    let before = ds.transactions.len();
    ds.transactions.retain(|t| t.id != id);
    if ds.transactions.len() == before {
        return Err("Transaction not found".into());
    }
    Ok(())
}

/// One parsed file of an import.
pub struct ParsedFile {
    pub name: String,
    pub statements: Vec<camt::Statement>,
}

/// Adds parsed statements to an account and logs the import (newest first in
/// `ds.imports`). Entries whose reference already exists on the account are skipped;
/// new ones get a category from the IBAN rules when one matches. `at` is RFC 3339.
pub fn apply_import(
    ds: &mut Dataset,
    account_id: &str,
    files: Vec<ParsedFile>,
    at: &str,
) -> Result<ImportRecord, String> {
    let account = ds.accounts.iter_mut().find(|a| a.id == account_id).ok_or("Account not found")?;
    for st in files.iter().flat_map(|f| &f.statements) {
        match (&account.iban, &st.iban) {
            (Some(mine), Some(theirs)) if mine != theirs => {
                return Err(format!("The statement is for {theirs}, the account has IBAN {mine}"));
            }
            (None, Some(theirs)) => account.iban = Some(theirs.clone()),
            _ => {}
        }
    }
    let mut known: std::collections::HashSet<String> = ds
        .transactions
        .iter()
        .filter(|t| t.account_id == account_id)
        .filter_map(|t| t.import_ref.clone())
        .collect();
    let import_id = new_id();
    let mut record = ImportRecord {
        id: import_id.clone(),
        at: at.to_string(),
        account_id: account_id.to_string(),
        files: Vec::new(),
        imported: 0,
        skipped_duplicates: 0,
        classified: 0,
        undone_at: None,
    };
    for file in files {
        let entries: Vec<camt::Entry> = file.statements.into_iter().flat_map(|s| s.entries).collect();
        let mut stat = ImportFileStat {
            name: file.name,
            entries: entries.len(),
            imported: 0,
            skipped_duplicates: 0,
            date_from: entries.iter().map(|e| e.date.clone()).min(),
            date_to: entries.iter().map(|e| e.date.clone()).max(),
        };
        for entry in entries {
            if !known.insert(entry.reference.clone()) {
                stat.skipped_duplicates += 1;
                continue;
            }
            // Rules first; whatever they don't match lands in "To categorise".
            let by_rule =
                ds.category_by_rules(&entry.description, entry.counterparty_iban.as_deref(), entry.amount_cents)
                    .map(|c| c.id.clone());
            record.classified += usize::from(by_rule.is_some());
            let category_id = Some(by_rule.unwrap_or_else(|| UNSORTED_CATEGORY_ID.to_string()));
            ds.transactions.push(Transaction {
                id: new_id(),
                date: entry.date,
                description: entry.description,
                amount_cents: entry.amount_cents,
                category_id,
                account_id: account_id.to_string(),
                import_ref: Some(entry.reference),
                counterparty_iban: entry.counterparty_iban,
                import_id: Some(import_id.clone()),
                counts_on: None,
                splits: Vec::new(),
            });
            stat.imported += 1;
        }
        record.imported += stat.imported;
        record.skipped_duplicates += stat.skipped_duplicates;
        record.files.push(stat);
    }
    sort_transactions(ds);
    ds.imports.insert(0, record.clone());
    Ok(record)
}

/// Removes the transactions an import created (also ones edited since) and marks the
/// log entry as undone. Returns how many transactions were removed.
pub fn undo_import(ds: &mut Dataset, import_id: &str, at: &str) -> Result<usize, String> {
    let record = ds.imports.iter_mut().find(|r| r.id == import_id).ok_or("Import not found")?;
    if record.undone_at.is_some() {
        return Err("This import has already been undone".into());
    }
    record.undone_at = Some(at.to_string());
    let before = ds.transactions.len();
    ds.transactions.retain(|t| t.import_id.as_deref() != Some(import_id));
    Ok(before - ds.transactions.len())
}

/// Newest first.
fn sort_transactions(ds: &mut Dataset) {
    ds.transactions.sort_by(|a, b| b.date.cmp(&a.date));
}

#[cfg(test)]
mod tests {
    use super::*;
    use fin_shared::catalog::ids;
    use fin_shared::{contains_words, suggest_pattern};

    /// One-file import; returns (imported, skipped, classified).
    fn import(ds: &mut Dataset, acc: &str, st: Vec<camt::Statement>) -> Result<(usize, usize, usize), String> {
        let files = vec![ParsedFile { name: "test.xml".into(), statements: st }];
        let r = apply_import(ds, acc, files, "2026-09-30T12:00:00Z")?;
        Ok((r.imported, r.skipped_duplicates, r.classified))
    }

    #[test]
    fn contracts_save_and_validate() {
        let mut ds = default_dataset();
        let c = Contract { name: " Stroom ".into(), energy: Some(Default::default()), ..Default::default() };
        let id = save_contract(&mut ds, c).unwrap();
        assert_eq!(ds.contracts[0].name, "Stroom");
        let mut c = ds.contracts[0].clone();
        c.usage = vec![
            fin_shared::UsagePeriod { start: "2026-09-30".into(), end: "2026-10-29".into(), ..Default::default() },
            fin_shared::UsagePeriod { start: "2026-08-30".into(), end: "2026-09-29".into(), used_normal: 66.0, ..Default::default() },
        ];
        assert_eq!(save_contract(&mut ds, c.clone()).unwrap(), id);
        assert_eq!(ds.contracts.len(), 1);
        assert_eq!(ds.contracts[0].usage[0].start, "2026-08-30", "periods sorted");
        let mut bad = c.clone();
        bad.usage[0].used_low = -1.0;
        assert!(save_contract(&mut ds, bad).is_err());
        let mut bad = c.clone();
        bad.energy.as_mut().unwrap().energy_tax = f64::NAN;
        assert!(save_contract(&mut ds, bad).is_err());
        // Net metering ends with 2026: a contract running past it loses it.
        let mut c = ds.contracts[0].clone();
        c.energy.as_mut().unwrap().netting = true;
        c.end = "2026-12-31".into();
        save_contract(&mut ds, c.clone()).unwrap();
        assert!(ds.contracts[0].energy.as_ref().unwrap().netting);
        c.end = "2027-06-30".into();
        save_contract(&mut ds, c).unwrap();
        assert!(!ds.contracts[0].energy.as_ref().unwrap().netting);
        delete_contract(&mut ds, &id).unwrap();
        assert!(ds.contracts.is_empty());
    }

    #[test]
    fn balances_start_and_check() {
        let mut ds = default_dataset();
        add_account(&mut ds, "Betaal", Some("NL88RBRB8836776922".into())).unwrap();
        let acc = ds.accounts[0].id.clone();
        // The balances are at the end of their day: 1 January's transaction is in the
        // first, 1 October's in the second.
        for (date, cents) in [("2026-01-01", -2_500), ("2026-01-05", -10_000), ("2026-10-01", 4_121)] {
            save_transaction(&mut ds, TransactionInput {
                id: None,
                date: date.into(),
                description: "x".into(),
                amount_cents: cents,
                category_id: None,
                account_id: acc.clone(),
            })
            .unwrap();
        }
        let csv = "01-01-2026,NL88RBRB8836776922,Plus Betalen,EUR,-696.99\n01-10-2026,NL88RBRB8836776922,Plus Betalen,EUR,-755.78\n";
        assert_eq!(import_balances(&mut ds, csv).unwrap(), 2);
        let checks = ds.balance_checks(&acc);
        assert_eq!(checks[0].computed_cents, -69_699 - 10_000 + 4_121);
        assert_eq!(checks[0].difference(), 0);
        assert_eq!(ds.account_balance(&acc), -75_578, "from the latest balance on");
        // Importing again replaces, an unknown IBAN imports nothing.
        assert_eq!(import_balances(&mut ds, csv).unwrap(), 2);
        assert_eq!(ds.accounts[0].balances.len(), 2);
        assert!(import_balances(&mut ds, "01-01-2026,NL91ABNA0417164300,X,EUR,1.00").is_err());
        assert!(import_balances(&mut ds, "nonsense").is_err());
        // Set by hand: a new date sorts in; None removes it.
        set_account_balance(&mut ds, &acc, "2025-12-31", Some(-59_173)).unwrap();
        assert_eq!(ds.accounts[0].balances[0].date, "2025-12-31");
        set_account_balance(&mut ds, &acc, "2025-12-31", None).unwrap();
        assert_eq!(ds.accounts[0].balances.len(), 2);
        assert!(set_account_balance(&mut ds, &acc, "31-12-2025", Some(1)).is_err());
    }

    #[test]
    fn split_a_transaction() {
        let mut ds = default_dataset();
        add_account(&mut ds, "Betaal", None).unwrap();
        let acc = ds.accounts[0].id.clone();
        save_transaction(&mut ds, TransactionInput {
            id: None,
            date: "2026-05-20".into(),
            description: "Salaris mei".into(),
            amount_cents: 535_551,
            category_id: Some(ids::SALARY.into()),
            account_id: acc,
        })
        .unwrap();
        let id = ds.transactions[0].id.clone();
        set_splits(&mut ds, &id, vec![(202_051, ids::HOLIDAY_PAY.into())]).unwrap();
        assert_eq!(ds.transactions[0].rest_cents(), 333_500);
        let o = ds.budget_overview("2026-05");
        let line = |id: &str| o.income.iter().find(|l| l.category_id.as_deref() == Some(id)).unwrap().actual_cents;
        assert_eq!((line(ids::SALARY), line(ids::HOLIDAY_PAY)), (333_500, 202_051));
        assert!(set_splits(&mut ds, &id, vec![(600_000, ids::HOLIDAY_PAY.into())]).is_err(), "more than the amount");
        set_splits(&mut ds, &id, vec![]).unwrap();
        assert!(ds.transactions[0].splits.is_empty());
    }

    #[test]
    fn language_renames_standard_categories() {
        let mut ds = default_dataset();
        add_category(&mut ds, "Hond", "Huishouden", CategoryKind::Variable).unwrap();
        add_category(&mut ds, "Hobby", "Eigen groep", CategoryKind::Variable).unwrap();
        let rules_before = list_rules(&ds);
        set_language(&mut ds, Lang::En).unwrap();
        assert_eq!(ds.language, Lang::En);
        let food = ds.category(Some(ids::GROCERIES)).unwrap();
        assert_eq!((food.name.as_str(), food.group.as_str()), ("Groceries", "Household"));
        let group = |ds: &Dataset, name: &str| ds.categories.iter().find(|c| c.name == name).unwrap().group.clone();
        assert_eq!(group(&ds, "Hond"), "Household", "own categories follow a standard group");
        assert_eq!(group(&ds, "Hobby"), "Eigen groep", "own groups stay");
        assert_eq!(list_rules(&ds), rules_before, "the rules stay Dutch");
        // Loading again keeps English; switching back restores the Dutch names.
        ensure_system_categories(&mut ds);
        assert_eq!(ds.category(Some(ids::GROCERIES)).unwrap().name, "Groceries");
        set_language(&mut ds, Lang::Nl).unwrap();
        assert_eq!(ds.category(Some(ids::GROCERIES)).unwrap().name, "Boodschappen");
        assert_eq!(group(&ds, "Hond"), "Huishouden");
    }

    #[test]
    fn default_rules_are_locked() {
        let mut ds = default_dataset();
        add_category_rule(&mut ds, ids::GROCERIES, RuleKind::Text, "elmas market").unwrap();
        let rules = list_rules(&ds);
        let find = |v: &str| rules.iter().find(|r| r.value == v).unwrap().clone();
        assert!(find("jumbo").default);
        assert!(!find("elmas market").default);
        assert!(remove_category_rule(&mut ds, ids::GROCERIES, RuleKind::Text, "jumbo").is_err());
        remove_category_rule(&mut ds, ids::GROCERIES, RuleKind::Text, "elmas market").unwrap();
        // A default moved to another category by the user is theirs there.
        add_category_rule(&mut ds, ids::SHOPPING, RuleKind::Text, "jumbo").unwrap();
        assert!(!list_rules(&ds).iter().find(|r| r.value == "jumbo").unwrap().default);
    }

    #[test]
    fn account_iban_loses_all_whitespace() {
        let mut ds = default_dataset();
        add_account(&mut ds, "Betaal", Some("nl91 abna\u{a0}0417 1643\t00".into())).unwrap();
        assert_eq!(ds.accounts[0].iban.as_deref(), Some("NL91ABNA0417164300"));
        let mut a = ds.accounts[0].clone();
        a.iban = Some(" \u{a0} ".into());
        update_account(&mut ds, a).unwrap();
        assert_eq!(ds.accounts[0].iban, None);
    }

    #[test]
    fn import_log_per_file_and_undo() {
        let mut ds = default_dataset();
        add_account(&mut ds, "Betaal", None).unwrap();
        let acc = ds.accounts[0].id.clone();
        let files = vec![
            ParsedFile { name: "sep.xml".into(), statements: camt::parse(camt::SAMPLE_V02).unwrap() },
            ParsedFile { name: "sep-again.xml".into(), statements: camt::parse(camt::SAMPLE_V02).unwrap() },
        ];
        let r = apply_import(&mut ds, &acc, files, "2026-09-30T12:00:00Z").unwrap();
        assert_eq!((r.imported, r.skipped_duplicates), (2, 2));
        assert_eq!(r.files[0].entries, 2);
        assert_eq!((r.files[0].imported, r.files[1].skipped_duplicates), (2, 2));
        assert_eq!(r.files[0].date_from.as_deref(), Some("2026-09-02"));
        assert_eq!(r.files[0].date_to.as_deref(), Some("2026-09-25"));
        assert_eq!(ds.imports.len(), 1);

        // A manual transaction survives undo; the imported ones go, even after edits.
        save_transaction(&mut ds, TransactionInput {
            id: None,
            date: "2026-09-03".into(),
            description: "Koffie".into(),
            amount_cents: -300,
            category_id: None,
            account_id: acc.clone(),
        })
        .unwrap();
        assert_eq!(undo_import(&mut ds, &r.id, "2026-09-30T13:00:00Z").unwrap(), 2);
        assert_eq!(ds.transactions.len(), 1);
        assert_eq!(ds.imports[0].undone_at.as_deref(), Some("2026-09-30T13:00:00Z"));
        assert!(undo_import(&mut ds, &r.id, "x").is_err(), "only once");

        // After undo the same file imports again (Albert Heijn via the default rule).
        assert_eq!(import(&mut ds, &acc, camt::parse(camt::SAMPLE_V02).unwrap()).unwrap(), (2, 0, 1));
        assert_eq!(ds.imports.len(), 2);
    }

    #[test]
    fn backups_daily_and_restore() {
        let path = temp_path("backups");
        let mut store = Store::load(path.clone()).unwrap();
        store.mutate(|ds| add_account(ds, "Eerste", None)).unwrap();
        assert!(store.list_backups().unwrap().is_empty(), "no file before the first save, so nothing to back up");
        store.mutate(|ds| add_account(ds, "Tweede", None)).unwrap();
        store.mutate(|ds| add_account(ds, "Derde", None)).unwrap();
        let list = store.list_backups().unwrap();
        assert_eq!(list.len(), 1, "one daily backup per day");
        assert_eq!(list[0].reason, backup::DAILY);

        // The daily backup holds the state before the second save: one account.
        store.restore_backup(&list[0].name).unwrap();
        assert_eq!(store.data.accounts.len(), 1);
        assert_eq!(Store::load(path.clone()).unwrap().data.accounts.len(), 1, "restore is saved");
        let list = store.list_backups().unwrap();
        assert!(list.iter().any(|b| b.reason == backup::BEFORE_RESTORE), "state before restore is kept");

        // Restoring from an exported file, and refusing junk.
        let exported = store.export_json().unwrap();
        store.mutate(|ds| add_account(ds, "Vierde", None)).unwrap();
        store.restore_bytes(&exported).unwrap();
        assert_eq!(store.data.accounts.len(), 1);
        assert!(store.restore_bytes(b"{\"hello\":1}").is_err());
        assert!(store.restore_backup("../fin.json").is_err());
        assert_eq!(store.data.accounts.len(), 1);
    }

    fn temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fin-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir.join("fin.json")
    }

    #[test]
    fn persists_and_reloads() {
        let path = temp_path("persist");
        let mut store = Store::load(path.clone()).unwrap();
        assert!(!store.data.categories.is_empty(), "new store gets default categories");
        store.mutate(|ds| add_account(ds, "Betaalrekening", Some("nl91 abna 0417 1643 00".into()))).unwrap();
        let account_id = store.data.accounts[0].id.clone();
        store
            .mutate(|ds| {
                save_transaction(ds, TransactionInput {
                    id: None,
                    date: "2026-09-10".into(),
                    description: " Koffie ".into(),
                    amount_cents: -350,
                    category_id: None,
                    account_id: account_id.clone(),
                })
            })
            .unwrap();

        let reloaded = Store::load(path.clone()).unwrap();
        assert_eq!(reloaded.data, store.data);
        assert_eq!(reloaded.data.accounts[0].iban.as_deref(), Some("NL91ABNA0417164300"));
        assert_eq!(reloaded.data.transactions[0].description, "Koffie");
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn failed_mutation_leaves_data_untouched() {
        let mut store = Store::load(temp_path("fail")).unwrap();
        let before = store.data.clone();
        let r = store.mutate(|ds| {
            ds.accounts.clear();
            save_transaction(ds, TransactionInput {
                id: None,
                date: "2026-13-01".into(),
                description: "x".into(),
                amount_cents: 1,
                category_id: None,
                account_id: "nope".into(),
            })
        });
        assert!(r.is_err());
        assert_eq!(store.data, before);
    }

    #[test]
    fn delete_rules() {
        let mut ds = default_dataset();
        add_account(&mut ds, "A", None).unwrap();
        let acc = ds.accounts[0].id.clone();
        add_category(&mut ds, "Hond", "Huishouden", CategoryKind::Variable).unwrap();
        let cat = ds.categories.iter().find(|c| c.name == "Hond").unwrap().id.clone();
        save_transaction(&mut ds, TransactionInput {
            id: None,
            date: "2026-09-01".into(),
            description: "x".into(),
            amount_cents: 100,
            category_id: Some(cat.clone()),
            account_id: acc.clone(),
        })
        .unwrap();
        assert!(delete_account(&mut ds, &acc).is_err());
        delete_category(&mut ds, &cat).unwrap();
        assert_eq!(ds.transactions[0].category_id, None);
    }

    #[test]
    fn set_and_remove_budget() {
        let mut ds = default_dataset();
        // Income first: expenses must fit in it (see budget_within_income).
        set_budget(&mut ds, ids::SALARY, "2026-01", Some(1_000_000)).unwrap();
        let cat = ids::RENT_MORTGAGE.to_string();
        let mine = |ds: &Dataset| ds.budgets.iter().filter(|b| b.category_id == cat).count();
        set_budget(&mut ds, &cat, "2026-09", Some(45000)).unwrap();
        set_budget(&mut ds, &cat, "2026-09", Some(50000)).unwrap();
        assert_eq!(mine(&ds), 1, "setting again replaces");
        assert_eq!(ds.budget_for(&cat, "2026-09"), Some(50000));
        assert_eq!(ds.budget_for(&cat, "2026-10"), None);

        set_budget(&mut ds, &cat, "2026-09", Some(0)).unwrap();
        assert_eq!(mine(&ds), 0, "0 removes");
        set_budget(&mut ds, &cat, "2026-09", Some(100)).unwrap();
        set_budget(&mut ds, &cat, "2026-09", None).unwrap();
        assert_eq!(mine(&ds), 0, "None removes");
        set_budget(&mut ds, &cat, "2026-09", None).unwrap();

        for bad in ["2026-13", "2026-00", "2026-9", "26-09", "2026-09-01", "abcd-01", "2026-+9"] {
            assert!(set_budget(&mut ds, &cat, bad, Some(100)).is_err(), "{bad}");
        }
        assert!(set_budget(&mut ds, "nope", "2026-09", Some(100)).is_err());
        assert!(set_budget(&mut ds, &cat, "2026-09", Some(-100)).is_err());
        assert_eq!(mine(&ds), 0);
    }

    #[test]
    fn variable_categories_are_budgeted_per_group() {
        let mut ds = default_dataset();
        let err = set_budget(&mut ds, ids::GROCERIES, "2026-09", Some(100)).unwrap_err();
        assert_eq!(err, "Variable categories are budgeted per group: set the budget on Huishouden");
        // A budget of theirs left in a file doesn't count, and can still be removed.
        ds.budgets.push(Budget { category_id: ids::GROCERIES.into(), month: "2026-09".into(), amount_cents: 100 });
        assert_eq!(budget_year_result(&ds, "2026"), 0);
        set_budget(&mut ds, ids::GROCERIES, "2026-09", Some(0)).unwrap();
        assert!(ds.budgets.is_empty());
    }

    #[test]
    fn budget_within_income() {
        let mut ds = default_dataset();
        set_budget(&mut ds, ids::SALARY, "2026-01", Some(100_000)).unwrap();
        set_budget(&mut ds, ids::RENT_MORTGAGE, "2026-02", Some(60_000)).unwrap();
        // Too much for what is left of the year's income: capped to the room (€ 400).
        set_budget(&mut ds, ids::INTERNET, "2026-03", Some(90_000)).unwrap();
        assert_eq!(ds.budget_for(ids::INTERNET, "2026-03"), Some(40_000));
        assert_eq!(budget_year_result(&ds, "2026"), 0);
        // No room left: more is refused, less is fine.
        assert!(set_budget(&mut ds, ids::INTERNET, "2026-04", Some(100)).is_err());
        assert!(set_group_budget(&mut ds, "Huishouden", CategoryKind::Variable, "2026-04", Some(100)).is_err());
        assert!(set_budget(&mut ds, ids::SALARY, "2026-01", Some(50_000)).is_err(), "income can't drop below the spending");
        set_budget(&mut ds, ids::RENT_MORTGAGE, "2026-02", Some(50_000)).unwrap();
        assert_eq!(budget_year_result(&ds, "2026"), 10_000);
        // Other years are separate; one without budgeted income isn't capped.
        set_budget(&mut ds, ids::RENT_MORTGAGE, "2027-01", Some(100)).unwrap();
    }

    #[test]
    fn group_budgets() {
        use CategoryKind::{Fixed, Variable};
        let mut ds = default_dataset();
        let m = "2026-01";
        set_budget(&mut ds, ids::SALARY, m, Some(100_000)).unwrap();
        set_group_budget(&mut ds, "Vervoer", Variable, m, Some(11_500)).unwrap();
        assert_eq!(budget_year_result(&ds, "2026"), 100_000 - 11_500);
        assert_eq!(ds.budget_overview(m).budget.variable, 11_500);
        // Fixed is separate, per category: road tax isn't in the variable budget.
        set_budget(&mut ds, ids::ROAD_TAX, m, Some(4_000)).unwrap();
        assert_eq!(budget_year_result(&ds, "2026"), 100_000 - 11_500 - 4_000);
        assert_eq!(set_group_budget(&mut ds, "Financiën", Fixed, m, Some(3_000)).unwrap_err(), "Group budgets are for variable costs");
        assert!(set_group_budget(&mut ds, "Inkomsten", CategoryKind::Income, m, Some(3_000)).is_err());
        assert!(set_group_budget(&mut ds, "Nergens", Variable, m, Some(3_000)).is_err());
        // A fixed group budget left in a file counts nowhere and can be removed.
        ds.group_budgets.push(GroupBudget { group: "Financiën".into(), kind: Fixed, month: m.into(), amount_cents: 5_000 });
        assert_eq!(budget_year_result(&ds, "2026"), 100_000 - 11_500 - 4_000);
        set_group_budget(&mut ds, "Financiën", Fixed, m, None).unwrap();
        assert_eq!(ds.group_budgets.len(), 1);
        // The year cap applies to the group budget too.
        assert!(set_group_budget(&mut ds, "Vervoer", Variable, m, Some(200_000)).is_ok());
        assert_eq!(ds.group_budget_for("Vervoer", Variable, m), Some(96_000), "capped to the room");
        assert_eq!(budget_year_result(&ds, "2026"), 0);
        set_group_budget(&mut ds, "Vervoer", Variable, m, None).unwrap();
        assert_eq!(budget_year_result(&ds, "2026"), 100_000 - 4_000);

        // Copying: a month to the next and a year to the next take group budgets along,
        // but not a variable category's budget left in a file.
        set_group_budget(&mut ds, "Vervoer", Variable, m, Some(12_000)).unwrap();
        ds.budgets.push(Budget { category_id: ids::FUEL.into(), month: m.into(), amount_cents: 700 });
        copy_budget_month_to_next(&mut ds, m).unwrap();
        assert_eq!(ds.group_budget_for("Vervoer", Variable, "2026-02"), Some(12_000));
        assert_eq!(ds.budget_for(ids::ROAD_TAX, "2026-02"), Some(4_000));
        assert_eq!(ds.budget_for(ids::FUEL, "2026-02"), None);
        assert!(copy_budgets_from_previous_year(&mut ds, 2027).unwrap() > 0);
        assert_eq!(ds.group_budget_for("Vervoer", Variable, "2027-01"), Some(12_000));
        assert_eq!(ds.budget_for(ids::FUEL, "2027-01"), None);

        // A language switch renames the group budget with its group.
        set_language(&mut ds, Lang::En).unwrap();
        assert_eq!(ds.group_budget_for("Transport", Variable, m), Some(12_000));
        set_language(&mut ds, Lang::Nl).unwrap();
        assert_eq!(ds.group_budget_for("Vervoer", Variable, m), Some(12_000));
    }

    #[test]
    fn groups_hold_fixed_or_variable_costs() {
        use CategoryKind::{Fixed, Investment, Variable};
        let mut ds = default_dataset();
        // Adding: the kind has to match the group's.
        assert_eq!(
            add_category(&mut ds, "Parkeervergunning", "Vervoer", Fixed).unwrap_err(),
            "Vervoer holds variable costs: a fixed cost needs a group of fixed costs"
        );
        assert_eq!(
            add_category(&mut ds, "Boetes", "Financiën", Variable).unwrap_err(),
            "Financiën holds fixed costs: a variable cost needs a group of variable costs"
        );
        add_category(&mut ds, "Fietsen", "Vervoer", Variable).unwrap();
        add_category(&mut ds, "Lening", "Financiën", Fixed).unwrap();
        add_category(&mut ds, "Zonnepanelen", "Vervoer", Investment).unwrap();
        add_category(&mut ds, "Eigen", "Nieuw", Fixed).unwrap();

        // Changing group or kind: the same, and a refused change leaves it as it was.
        let mut bike = ds.categories.iter().find(|c| c.name == "Fietsen").unwrap().clone();
        bike.kind = Fixed;
        assert!(update_category(&mut ds, bike.clone()).unwrap_err().starts_with("Vervoer holds variable costs"));
        assert_eq!(ds.category(Some(&bike.id)).unwrap().kind, Variable, "left as it was");
        bike.kind = Variable;
        bike.group = "Financiën".into();
        assert!(update_category(&mut ds, bike.clone()).unwrap_err().starts_with("Financiën holds fixed costs"));
        assert_eq!(ds.category(Some(&bike.id)).unwrap().group, "Vervoer", "left as it was");
        // A group of its own kind, or one being emptied of the other kind, is fine.
        bike.group = "Huishouden".into();
        update_category(&mut ds, bike).unwrap();
        let mut own = ds.categories.iter().find(|c| c.name == "Eigen").unwrap().clone();
        own.kind = Variable;
        update_category(&mut ds, own).unwrap();
    }

    #[test]
    fn copy_budgets_from_previous_year_does_not_overwrite() {
        let mut ds = default_dataset();
        let (a, b) = (ids::RENT_MORTGAGE.to_string(), ids::INTERNET.to_string());
        for m in 1..=12 {
            set_budget(&mut ds, &a, &format!("2025-{m:02}"), Some(1000 * m)).unwrap();
        }
        set_budget(&mut ds, &b, "2025-07", Some(90000)).unwrap();
        set_budget(&mut ds, &a, "2026-03", Some(1)).unwrap();
        set_budget(&mut ds, &a, "2024-01", Some(777)).unwrap();

        assert_eq!(copy_budgets_from_previous_year(&mut ds, 2026).unwrap(), 12);
        assert_eq!(ds.budget_for(&a, "2026-01"), Some(1000));
        assert_eq!(ds.budget_for(&a, "2026-12"), Some(12000));
        assert_eq!(ds.budget_for(&a, "2026-03"), Some(1), "existing budget kept");
        assert_eq!(ds.budget_for(&b, "2026-07"), Some(90000));
        assert_eq!(ds.budget_for(&b, "2026-08"), None);
        assert_eq!(ds.budget_for(&a, "2025-01"), Some(1000), "source untouched");

        assert_eq!(copy_budgets_from_previous_year(&mut ds, 2026).unwrap(), 0, "second run copies nothing");
        assert_eq!(copy_budgets_from_previous_year(&mut ds, 2030).unwrap(), 0);
        assert!(copy_budgets_from_previous_year(&mut ds, 0).is_err());
    }

    #[test]
    fn copy_budget_month_to_next_overwrites_and_rolls_over_year() {
        let mut ds = default_dataset();
        let (a, b, c) = (ids::RENT_MORTGAGE.to_string(), ids::INTERNET.to_string(), ids::MOBILE.to_string());
        set_budget(&mut ds, &a, "2026-09", Some(100)).unwrap();
        set_budget(&mut ds, &b, "2026-09", Some(200)).unwrap();
        set_budget(&mut ds, &a, "2026-10", Some(5)).unwrap();
        set_budget(&mut ds, &c, "2026-10", Some(300)).unwrap();

        copy_budget_month_to_next(&mut ds, "2026-09").unwrap();
        assert_eq!(ds.budget_for(&a, "2026-10"), Some(100), "overwritten");
        assert_eq!(ds.budget_for(&b, "2026-10"), Some(200));
        assert_eq!(ds.budget_for(&c, "2026-10"), Some(300), "category not in source month stays");
        assert_eq!(ds.budgets.len(), 5);

        set_budget(&mut ds, &a, "2026-12", Some(4200)).unwrap();
        copy_budget_month_to_next(&mut ds, "2026-12").unwrap();
        assert_eq!(ds.budget_for(&a, "2027-01"), Some(4200));
        assert!(copy_budget_month_to_next(&mut ds, "2026-13").is_err());
        assert!(copy_budget_month_to_next(&mut ds, "9999-12").is_err());
    }

    #[test]
    fn budgets_from_average_fill_or_overwrite() {
        let mut ds = default_dataset();
        add_account(&mut ds, "A", None).unwrap();
        let acc = ds.accounts[0].id.clone();
        let food = ids::GROCERIES.to_string();
        let cash = ids::CASH_WITHDRAWALS.to_string();
        let rent = ids::RENT_MORTGAGE.to_string();
        let transfer = ids::INTERNAL_TRANSFERS.to_string();
        for (date, cat, cents) in [
            ("2026-07-03", &food, -30000),
            ("2026-08-03", &food, -60000),
            ("2026-08-05", &cash, -15000),
            ("2026-08-01", &rent, -90000),
            ("2026-08-04", &transfer, -99900),
        ] {
            save_transaction(&mut ds, TransactionInput {
                id: None,
                date: date.into(),
                description: "x".into(),
                amount_cents: cents,
                category_id: Some(cat.clone()),
                account_id: acc.clone(),
            })
            .unwrap();
        }
        set_budget(&mut ds, &rent, "2026-12", Some(1)).unwrap();
        ds.group_budgets.push(GroupBudget { group: "Huishouden".into(), kind: CategoryKind::Variable, month: "2026-12".into(), amount_cents: 1 });
        let household = |ds: &Dataset, m: &str| ds.group_budget_for("Huishouden", CategoryKind::Variable, m);

        // Three months up to August. Rent: 900 / 3 = 300, per category. Groceries
        // (300 + 600) / 3 = 300 and cash 150 / 3 = 50: together 350 for Huishouden.
        assert_eq!(budgets_from_average(&mut ds, 2026, "2026-08", 3, false).unwrap(), 22);
        assert_eq!(ds.budget_for(&rent, "2026-01"), Some(30000));
        assert_eq!(household(&ds, "2026-01"), Some(35000));
        assert_eq!(ds.budget_for(&food, "2026-01"), None, "variable categories get no budget of their own");
        assert_eq!(ds.budget_for(&rent, "2026-12"), Some(1), "existing budget kept");
        assert_eq!(household(&ds, "2026-12"), Some(1), "existing group budget kept");
        assert_eq!(ds.budget_for(&transfer, "2026-01"), None, "transfers get no budget");

        assert_eq!(budgets_from_average(&mut ds, 2026, "2026-08", 3, true).unwrap(), 2);
        assert_eq!(ds.budget_for(&rent, "2026-12"), Some(30000), "overwritten");
        assert_eq!(household(&ds, "2026-12"), Some(35000), "overwritten");
        assert_eq!(budgets_from_average(&mut ds, 2026, "2026-08", 3, true).unwrap(), 0, "nothing left to change");
        assert!(budgets_from_average(&mut ds, 2026, "2026-13", 3, false).is_err());
        assert!(budgets_from_average(&mut ds, 2026, "2026-08", 0, false).is_err());
    }

    #[test]
    fn deleting_category_removes_its_budgets() {
        let mut ds = default_dataset();
        add_category(&mut ds, "Hond", "Huishouden", CategoryKind::Variable).unwrap();
        let a = ds.categories.iter().find(|c| c.name == "Hond").unwrap().id.clone();
        let b = ids::RENT_MORTGAGE.to_string();
        // Budgets of a variable category left in a file.
        for m in ["2026-09", "2026-10"] {
            ds.budgets.push(Budget { category_id: a.clone(), month: m.into(), amount_cents: 100 });
        }
        set_budget(&mut ds, &b, "2026-09", Some(200)).unwrap();
        delete_category(&mut ds, &a).unwrap();
        assert_eq!(ds.budgets.len(), 1);
        assert_eq!(ds.budget_for(&b, "2026-09"), Some(200));
    }

    #[test]
    fn loads_file_without_budgets() {
        let path = temp_path("old-format");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let old = r#"{"version": 1, "data": {
            "accounts": [{"id": "a", "name": "Betaal"}],
            "categories": [{"id": "c", "name": "Boodschappen", "group": "Huishouden", "kind": "Variable"}],
            "transactions": []
        }}"#;
        fs::write(&path, old).unwrap();
        let mut store = Store::load(path.clone()).unwrap();
        assert!(store.data.budgets.is_empty());
        assert_eq!(store.data.categories.len(), catalog::CATALOG.len() + 1, "the catalog is added");

        store.mutate(|ds| set_budget(ds, ids::RENT_MORTGAGE, "2026-09", Some(45000))).unwrap();
        let reloaded = Store::load(path).unwrap();
        assert_eq!(reloaded.data.budget_for(ids::RENT_MORTGAGE, "2026-09"), Some(45000));
    }

    #[test]
    fn import_skips_duplicates_and_checks_iban() {
        let mut ds = default_dataset();
        add_account(&mut ds, "Betaal", None).unwrap();
        let acc = ds.accounts[0].id.clone();

        let st = camt::parse(camt::SAMPLE_V02).unwrap();
        assert_eq!(import(&mut ds, &acc, st.clone()).unwrap(), (2, 0, 1), "Albert Heijn via the default rule");
        assert_eq!(ds.accounts[0].iban.as_deref(), Some("NL91ABNA0417164300"));
        assert_eq!(import(&mut ds, &acc, st.clone()).unwrap(), (0, 2, 0));
        assert_eq!(ds.transactions.len(), 2);
        assert_eq!(ds.monthly_overview("2026-09").income_cents, 300000);

        add_account(&mut ds, "Spaar", Some("NL02RABO0123456789".into())).unwrap();
        let other = ds.accounts[1].id.clone();
        assert!(import(&mut ds, &other, st).is_err());
    }

    #[test]
    fn system_categories_are_added_adopted_and_protected() {
        let ds = default_dataset();
        assert_eq!(ds.categories.len(), catalog::CATALOG.len());
        assert!(ds.categories.iter().all(|c| c.system));
        assert_eq!(ds.categories[0].id, ids::SALARY);
        assert_eq!(ds.categories[0].name, "Salaris/uitkering", "name from the nl-NL mapping");

        // A file with one standard category under an old name and one custom category:
        // the rest of the catalog is added, the name follows the mapping, the custom one stays.
        let mut file = Dataset::default();
        file.categories.push(Category {
            id: ids::HEALTH_INSURANCE.into(),
            name: "Ziektekostenverzekering".into(),
            group: "Medische kosten".into(),
            ..Default::default()
        });
        file.categories.push(Category { id: "u2".into(), name: "Hond".into(), group: "Huishouden".into(), ..Default::default() });
        ensure_system_categories(&mut file);
        ensure_system_categories(&mut file); // idempotent
        assert_eq!(file.categories.len(), catalog::CATALOG.len() + 1);
        let health = file.category(Some(ids::HEALTH_INSURANCE)).unwrap();
        assert!(health.system);
        assert_eq!(health.name, "Zorgverzekering", "renamed through the nl-NL mapping");
        assert!(!file.categories.iter().find(|c| c.id == "u2").unwrap().system);

        // System: no delete, no rename; disabling works and blocks new assignments.
        let mut ds = ds;
        let id = ds.categories[0].id.clone();
        assert!(delete_category(&mut ds, &id).is_err());
        let mut edit = ds.categories[0].clone();
        edit.name = "Anders".into();
        edit.disabled = true;
        update_category(&mut ds, edit).unwrap();
        assert_eq!((ds.categories[0].name.as_str(), ds.categories[0].disabled), ("Salaris/uitkering", true));

        add_account(&mut ds, "A", None).unwrap();
        let acc = ds.accounts[0].id.clone();
        let input = |cat: &str| TransactionInput {
            id: None,
            date: "2026-09-01".into(),
            description: "x".into(),
            amount_cents: 100,
            category_id: Some(cat.into()),
            account_id: acc.clone(),
        };
        assert!(save_transaction(&mut ds, input(&id)).is_err());
        let custom = ds.categories.iter().find(|c| !c.system).map(|c| c.id.clone());
        assert!(custom.is_none(), "default dataset has only system categories");

        // A category in use can't be disabled.
        let food = ids::GROCERIES.to_string();
        save_transaction(&mut ds, input(&food)).unwrap();
        let mut off = ds.categories.iter().find(|c| c.id == food).unwrap().clone();
        off.disabled = true;
        let err = update_category(&mut ds, off).unwrap_err();
        assert!(err.contains("1 transaction."), "{err}");

        // Only "Internal transfers" is a transfer, and that is fixed.
        let transfer = ids::INTERNAL_TRANSFERS.to_string();
        let mut t = ds.categories.iter().find(|c| c.id == transfer).unwrap().clone();
        assert_eq!(t.kind, CategoryKind::Transfer);
        t.kind = CategoryKind::Variable;
        assert!(update_category(&mut ds, t).is_err());
        // Standard categories keep their kind; custom ones can change it (not to transfer).
        for k in [CategoryKind::Transfer, CategoryKind::Fixed] {
            let mut f = ds.categories.iter().find(|c| c.id == food).unwrap().clone();
            f.kind = k;
            assert!(update_category(&mut ds, f).is_err(), "{k:?}");
        }
        assert!(add_category(&mut ds, "Spaar", "Overig", CategoryKind::Transfer).is_err());
        add_category(&mut ds, "Hond", "Huishouden", CategoryKind::Variable).unwrap();
        let mut dog = ds.categories.iter().find(|c| c.name == "Hond").unwrap().clone();
        dog.kind = CategoryKind::Investment;
        update_category(&mut ds, dog.clone()).unwrap();
        dog.kind = CategoryKind::Transfer;
        assert!(update_category(&mut ds, dog).is_err());
    }

    /// Imports a real bank file into a fresh dataset and reports what the default rules
    /// catch. Not part of the normal run (the file stays on the user's machine):
    /// `FIN_CAMT_FILE=/path/to/file.xml cargo test -p fin real_file_report -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_file_report() {
        let path = std::env::var("FIN_CAMT_FILE").expect("set FIN_CAMT_FILE");
        let xml = fs::read_to_string(&path).unwrap();
        let statements = camt::parse(&xml).unwrap();
        let mut ds = default_dataset();
        // Personal setup as a user would do it (see apply_personal_setup).
        apply_personal_setup(&mut ds, &statements).unwrap();
        if ds.accounts.is_empty() {
            add_account(&mut ds, "Test", None).unwrap();
        }
        let acc = ds.accounts[0].id.clone();
        let files = vec![ParsedFile { name: "real.xml".into(), statements }];
        let r = apply_import(&mut ds, &acc, files, "2026-09-30T12:00:00Z").unwrap();
        println!("imported {} · by rules {} · duplicates {}", r.imported, r.classified, r.skipped_duplicates);
        let mut per_cat: Vec<(String, usize)> = Vec::new();
        for t in &ds.transactions {
            let name = ds.category(t.category_id.as_deref()).map(|c| c.name.clone()).unwrap_or_default();
            match per_cat.iter_mut().find(|(n, _)| *n == name) {
                Some((_, c)) => *c += 1,
                None => per_cat.push((name, 1)),
            }
        }
        per_cat.sort_by(|a, b| b.1.cmp(&a.1));
        println!("per category: {per_cat:?}");
        let count = |c| ds.transactions.iter().filter(|t| t.channel() == c).count();
        println!(
            "channel: online {} · in store {} · other {}",
            count(Some(fin_shared::Channel::Online)),
            count(Some(fin_shared::Channel::InStore)),
            count(None)
        );
        for t in ds.transactions.iter().filter(|t| t.channel() == Some(fin_shared::Channel::Online)) {
            println!("  online ← {}", t.description.chars().take(50).collect::<String>());
        }
        // Unsorted, grouped by the suggested text rule.
        let mut open: Vec<(String, usize, bool, i64)> = Vec::new();
        for t in ds.transactions.iter().filter(|t| t.is_unsorted()) {
            let key = suggest_pattern(&t.description);
            match open.iter_mut().find(|(k, ..)| *k == key) {
                Some((_, n, _, sum)) => {
                    *n += 1;
                    *sum += t.amount_cents;
                }
                None => open.push((key, 1, t.counterparty_iban.is_some(), t.amount_cents)),
            }
        }
        open.sort_by(|a, b| b.1.cmp(&a.1));
        for (k, n, iban, sum) in open {
            println!("  open {n:>3}× iban={iban:<5} sum={:>10}  {k}", sum);
        }
        // What each rule caught, to spot wrong matches: category ← description (first part).
        for t in ds.transactions.iter().filter(|t| !t.is_unsorted()) {
            let name = ds.category(t.category_id.as_deref()).map(|c| c.name.clone()).unwrap_or_default();
            let head: String = t.description.chars().take(60).collect();
            println!("  hit  {name:<24} ← {head}");
        }
    }

    /// One-off personal setup on a real data file (stop the app first; a backup is made):
    /// adds the statement's account and own accounts found by description (by the
    /// counterparty IBAN in the bank file) and personal rules.
    /// FIN_DATA_FILE=… FIN_CAMT_FILE=… FIN_OWN_ACCOUNTS="Spaar-op-maat=spaar-op-maat;Gezamenlijke rekening=e/o"
    /// FIN_TEST_RULES="…" cargo test -p fin seed_personal_setup -- --ignored --nocapture
    #[test]
    #[ignore]
    fn seed_personal_setup() {
        let data = PathBuf::from(std::env::var("FIN_DATA_FILE").expect("set FIN_DATA_FILE"));
        let xml = fs::read_to_string(std::env::var("FIN_CAMT_FILE").expect("set FIN_CAMT_FILE")).unwrap();
        let statements = camt::parse(&xml).unwrap();
        let mut store = Store::load(data).unwrap();
        store.mutate(|ds| apply_personal_setup(ds, &statements)).unwrap();
    }

    /// The statement's own account, FIN_OWN_ACCOUNTS (name=text found in a description,
    /// its counterparty IBAN becomes an own account) and FIN_TEST_RULES (category:in|out|text:pattern).
    fn apply_personal_setup(ds: &mut Dataset, statements: &[camt::Statement]) -> Result<(), String> {
        let mut added = Vec::new();
        if let Some(iban) = statements.iter().find_map(|s| s.iban.clone()) {
            if !ds.accounts.iter().any(|a| a.iban.as_deref() == Some(iban.as_str())) {
                add_account(ds, "Betaalrekening", Some(iban))?;
                added.push("Betaalrekening".to_string());
            }
        }
        for pair in std::env::var("FIN_OWN_ACCOUNTS").unwrap_or_default().split(';').filter(|p| !p.is_empty()) {
            let (name, needle) = pair.split_once('=').ok_or("FIN_OWN_ACCOUNTS: name=text")?;
            let iban = statements
                .iter()
                .flat_map(|s| &s.entries)
                .find(|e| contains_words(&e.description, needle) && e.counterparty_iban.is_some())
                .and_then(|e| e.counterparty_iban.clone())
                .ok_or(format!("no counterparty IBAN for \"{needle}\""))?;
            if !ds.accounts.iter().any(|a| a.iban.as_deref() == Some(iban.as_str())) {
                add_account(ds, name, Some(iban))?;
                added.push(name.to_string());
            }
        }
        let mut rules = 0;
        for rule in std::env::var("FIN_TEST_RULES").unwrap_or_default().split(';').filter(|r| !r.is_empty()) {
            let parts: Vec<&str> = rule.splitn(3, ':').collect();
            let kind = match parts[1] {
                "in" => RuleKind::TextIn,
                "out" => RuleKind::TextOut,
                _ => RuleKind::Text,
            };
            add_category_rule(ds, &format!("sys-{}", parts[0]), kind, parts[2])?;
            rules += 1;
        }
        println!("accounts added: {added:?} · rules added: {rules}");
        Ok(())
    }

    #[test]
    fn default_rules_added_once() {
        let mut ds = default_dataset();
        let food = ids::GROCERIES.to_string();
        let care = ids::PERSONAL_CARE.to_string();
        let cat = |ds: &Dataset, id: &str| ds.categories.iter().find(|c| c.id == id).unwrap().clone();
        assert!(cat(&ds, &food).patterns.contains(&"jumbo".to_string()));
        assert!(cat(&ds, &care).patterns.contains(&"kruidvat".to_string()));
        assert_eq!(ds.category_by_rules("KRUIDVAT 7788 ZEIST", None, -100).map(|c| c.id.clone()), Some(care.clone()));
        let medical = ids::MEDICAL_OTHER.to_string();
        assert_eq!(ds.category_by_rules("Apotheek De Linde", None, -100).map(|c| c.id.clone()), Some(medical));
        // "plus" is no default: too ambiguous.
        assert!(ds.category_by_rules("Surplus Nederland", None, -100).is_none());
        // A new file gets every default.
        let defaults: usize = locale::nl_nl().rules.iter().map(|r| r.patterns.len()).sum();
        assert_eq!(list_rules(&ds).iter().filter(|r| r.default).count(), defaults);
        assert_eq!(ds.default_rules_version, locale::nl_nl().rules_version());

        // A file from before 2.0 (rules version 16) gets nothing added.
        let mut v16 = default_dataset();
        v16.categories.iter_mut().for_each(|c| c.patterns.clear());
        v16.default_rules_version = 16;
        ensure_system_categories(&mut v16);
        assert!(v16.categories.iter().all(|c| c.patterns.is_empty()));
        assert_eq!(v16.default_rules_version, 16);

        // A default removed (by an older version, which allowed it) stays removed.
        ds.categories.iter_mut().find(|c| c.id == food).unwrap().patterns.retain(|p| p != "jumbo");
        ensure_system_categories(&mut ds);
        assert!(!cat(&ds, &food).patterns.contains(&"jumbo".to_string()));

        // An older file gets them once, but a pattern the user put elsewhere stays there.
        let mut old = Dataset::default();
        old.categories.push(Category { id: "u".into(), name: "Drogist".into(), group: "Eigen".into(), patterns: vec!["etos".into()], ..Default::default() });
        ensure_system_categories(&mut old);
        assert_eq!(old.default_rules_version, locale::nl_nl().rules_version());
        assert!(!cat(&old, &care).patterns.contains(&"etos".to_string()));
        assert!(cat(&old, &care).patterns.contains(&"kruidvat".to_string()));
    }

    #[test]
    fn subscriptions_and_direction_rules() {
        use RuleKind::{Text, TextIn, TextOut};
        let mut ds = default_dataset();
        let hit = |ds: &Dataset, d: &str, cents| ds.category_by_rules(d, None, cents).map(|c| c.id.clone());
        for d in ["Spotify – P1234", "NPO – Maandelijkse betaling NPO Start Plus", "AMAZON PRIME VIDEO UK IDEAL", "Google Play Apps >Dublin", "TransIP B.V. – factuur", "mijn.host – hosting"] {
            assert_eq!(hit(&ds, d, -500).as_deref(), Some(ids::SUBSCRIPTIONS), "{d}");
        }
        assert_eq!(hit(&ds, "Vinted via Mangopay SA – 12345", -3482).as_deref(), Some(ids::CLOTHES));
        assert_eq!(hit(&ds, "H. de Vries via Tikkie – etentje", -2000).as_deref(), Some(ids::PAYMENT_REQUESTS));
        assert_eq!(hit(&ds, "S. de Putter via Rabo Betaalverzoek", 1500).as_deref(), Some(ids::PAYMENT_REQUESTS));
        assert_eq!(hit(&ds, "IJssalon Fresco via Tikkie", -720).as_deref(), Some(ids::OUTINGS), "the shop wins");
        for d in ["ACTION 1234 >UTRECHT", "HEMA UTRECHT >UTRECHT", "Bol.com – bestelling 998", "Amazon EU SARL – 1", "Marktplaats via Online Payments", "Klarna Bank AB via Stichting Mollie Payments"] {
            assert_eq!(hit(&ds, d, -1000).as_deref(), Some(ids::SHOPPING), "{d}");
        }
        assert_eq!(hit(&ds, "Amazon Prime Video UK iDeal", -299).as_deref(), Some(ids::SUBSCRIPTIONS), "longer rule wins");
        assert_eq!(hit(&ds, "Klarna Bank AB – Zalando bestelling", -5000).as_deref(), Some(ids::CLOTHES), "the shop wins over Klarna");

        for d in ["GAMMA 123 >ZWOLLE", "Praxis Utrecht >UTRECHT", "IKEA Delft >DELFT", "Karwei – bestelling"] {
            assert_eq!(hit(&ds, d, -2500).as_deref(), Some(ids::SHOPPING), "{d}");
        }
        assert_eq!(hit(&ds, "Koninklijke PostNL B.V. – PnlFrank", -440).as_deref(), Some(ids::SHOPPING), "postage is shopping");
        assert!(hit(&ds, "Transaction fee", -100).is_none(), "whole words only");
        // Belastingdienst and DUO by direction; a specific word still wins.
        assert_eq!(hit(&ds, "Belastingdienst – teruggaaf 2025", 75900).as_deref(), Some(ids::TAX_REFUND));
        assert_eq!(hit(&ds, "Belastingdienst – aanslag IB 2025", -12000).as_deref(), Some(ids::TAXES));
        assert_eq!(hit(&ds, "Belastingdienst – VOORSCHOT ZORGTOESLAG", 12300).as_deref(), Some(ids::TAX_ALLOWANCES));
        assert_eq!(hit(&ds, "Belastingdienst – motorrijtuigenbelasting", -9000).as_deref(), Some(ids::ROAD_TAX));
        assert_eq!(hit(&ds, "DUO hoofdrekening – aflossing", -6585).as_deref(), Some(ids::DEBT_REPAYMENT));
        assert_eq!(hit(&ds, "DUO – studiefinanciering", 30000).as_deref(), Some(ids::STUDY_ALLOWANCE));

        // A user's own direction rules, like an employer: salary in, lunch out.
        let salary = ids::SALARY.to_string();
        let food = ids::GROCERIES.to_string();
        add_category_rule(&mut ds, &salary, TextIn, "Acme").unwrap();
        add_category_rule(&mut ds, &food, TextOut, "acme").unwrap();
        assert_eq!(hit(&ds, "Acme B.V. – salaris september", 300000).as_deref(), Some(ids::SALARY));
        assert_eq!(hit(&ds, "ACME B.V. >UTRECHT", -7375).as_deref(), Some(ids::GROCERIES));
        // Same text, other direction, other category: no conflict; same direction moves it.
        add_category_rule(&mut ds, &ids::OUTINGS.to_string(), TextOut, "acme").unwrap();
        assert!(ds.category(Some(ids::GROCERIES)).unwrap().patterns_out.is_empty());
        assert_eq!(ds.category(Some(ids::SALARY)).unwrap().patterns_in, vec!["acme".to_string()]);
        remove_category_rule(&mut ds, &salary, TextIn, "acme").unwrap();
        assert!(remove_category_rule(&mut ds, &salary, Text, "acme").is_err());
    }

    #[test]
    fn insurance_and_mortgage_rules() {
        let ds = default_dataset();
        let ins = ids::INSURANCE.to_string();
        let home = ids::RENT_MORTGAGE.to_string();
        let hit = |d: &str| ds.category_by_rules(d, None, -100).map(|c| c.id.clone());
        assert_eq!(hit("NATIONALE-NEDERLANDEN Schadeverzekering"), Some(ins.clone()), "as banks write it");
        assert_eq!(hit("Nationale Nederlanden premie"), Some(ins.clone()));
        assert_eq!(hit("NN Leven premie okt"), Some(ins.clone()));
        assert_eq!(hit("Centraal Beheer – verzekeringen"), Some(ins.clone()));
        assert_eq!(hit("Samen verzekeren incasso"), Some(ins.clone()));
        let health = ids::HEALTH_INSURANCE.to_string();
        assert_eq!(hit("Zilveren Kruis zorgverzekering"), Some(health.clone()), "health insurance, not Insurance");
        assert_eq!(hit("VGZ Zorgverzekeraar NV – premie"), Some(health.clone()));
        assert_eq!(hit("CZ groep incasso"), Some(health.clone()));
        assert_eq!(hit("a.s.r. zorgverzekering"), Some(health));
        assert_eq!(hit("Autoverzekering Centraal Beheer"), None, "compounds stay out");
        assert_eq!(hit("ING Hypotheken termijn"), Some(home.clone()));
        assert_eq!(hit("Rente hypotheek"), Some(home));

        let internet = ids::INTERNET.to_string();
        let mobile = ids::MOBILE.to_string();
        assert_eq!(hit("KPN B.V. – factuur 123"), Some(internet));
        assert_eq!(hit("KPN Mobiel – factuur 456"), Some(mobile), "longest rule wins");
    }

    #[test]
    fn disabled_category_with_transactions_is_switched_on() {
        // An unknown key ("excluded", from old files) is ignored, not an error.
        let json = r#"{"version":1,"data":{"accounts":[],"transactions":[
            {"id":"t","date":"2026-09-01","description":"x","amount_cents":-1,"category_id":"u2","account_id":"a"}],
          "categories":[
            {"id":"u2","name":"Hond","group":"Huishouden","kind":"Variable","excluded":true,"disabled":true}]}}"#;
        let path = temp_path("disabled");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, json).unwrap();
        let store = Store::load(path.clone()).unwrap();
        let dog = store.data.categories.iter().find(|c| c.id == "u2").unwrap();
        assert_eq!(dog.kind, CategoryKind::Variable);
        assert!(!dog.disabled, "a disabled category with transactions is switched back on");
        let saved = String::from_utf8(store.export_json().unwrap()).unwrap();
        assert!(!saved.contains("excluded"), "unknown keys are not written back");
    }

    #[test]
    fn rules_classify_imports_and_existing_transactions() {
        use RuleKind::{Iban, Text};
        let mut ds = default_dataset();
        // Start without the default rules, to see each rule do its work.
        ds.categories.iter_mut().for_each(|c| c.patterns.clear());
        add_account(&mut ds, "Betaal", None).unwrap();
        let acc = ds.accounts[0].id.clone();
        let st = camt::parse(camt::SAMPLE_V02).unwrap();
        import(&mut ds, &acc, st).unwrap();
        assert!(
            ds.transactions.iter().all(|t| t.category_id.as_deref() == Some(UNSORTED_CATEGORY_ID)),
            "no rule matched, so everything is still to categorise"
        );

        let salary = ids::SALARY.to_string();
        let food = ids::GROCERIES.to_string();
        // A rule added afterwards fills in the matching transaction still to be sorted.
        assert_eq!(add_category_rule(&mut ds, &salary, Iban, "nl20 ingb 0001 2345 67").unwrap(), 1);
        let tx = ds.transactions.iter().find(|t| t.amount_cents > 0).unwrap();
        assert_eq!(tx.category_id.as_deref(), Some(salary.as_str()));
        assert!(add_category_rule(&mut ds, &salary, Iban, "not an iban").is_err());
        assert!(add_category_rule(&mut ds, &salary, Text, "ah").is_err(), "text rules need 3 characters");
        assert!(add_category_rule(&mut ds, UNSORTED_CATEGORY_ID, Text, "albert").is_err());

        // A text rule on the description (card payments, iDEAL).
        assert_eq!(add_category_rule(&mut ds, &food, Text, "  Albert HEIJN ").unwrap(), 1);
        let rules = |ds: &Dataset, id: &str| ds.categories.iter().find(|c| c.id == id).unwrap().clone();
        assert_eq!(rules(&ds, &food).patterns, vec!["albert heijn".to_string()]);

        // A rule lives in one category: adding it elsewhere moves it.
        add_category_rule(&mut ds, &salary, Iban, "NL44INGB0000123456").unwrap();
        add_category_rule(&mut ds, &food, Iban, "NL44INGB0000123456").unwrap();
        assert_eq!(rules(&ds, &salary).ibans, vec!["NL20INGB0001234567".to_string()]);
        assert_eq!(rules(&ds, &food).ibans, vec!["NL44INGB0000123456".to_string()]);

        // New imports are classified on the way in.
        let mut fresh = ds.clone();
        fresh.transactions.clear();
        let st = camt::parse(camt::SAMPLE_V02).unwrap();
        assert_eq!(import(&mut fresh, &acc, st).unwrap(), (2, 0, 2));

        remove_category_rule(&mut ds, &food, Iban, "NL44INGB0000123456").unwrap();
        assert!(remove_category_rule(&mut ds, &food, Iban, "NL44INGB0000123456").is_err());

        // Applying to the year also overrides a category set by hand, but not older ones.
        let shop = ids::OUTINGS.to_string();
        let ah = ds.transactions.iter().position(|t| t.amount_cents < 0).unwrap();
        ds.transactions[ah].category_id = Some(shop.clone());
        let mut old = ds.transactions[ah].clone();
        old.id = "old".into();
        old.date = "2025-12-31".into();
        ds.transactions.push(old);
        assert_eq!(apply_category_rule(&mut ds, &food, Text, "albert heijn", "2026-01-01").unwrap(), 1);
        assert_eq!(ds.transactions[ah].category_id.as_deref(), Some(food.as_str()));
        assert_eq!(ds.transactions.last().unwrap().category_id.as_deref(), Some(shop.as_str()));
        assert_eq!(apply_category_rule(&mut ds, &food, Iban, "NL44INGB0000123456", "2026-01-01").unwrap(), 0);
        assert!(apply_category_rule(&mut ds, &food, Iban, "NL44INGB0000123456", "2026").is_err());
    }
}
