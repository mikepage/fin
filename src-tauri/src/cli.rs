//! `fin-cli`: the same data and operations as the app, from a terminal (or Claude Code).
//! Every command prints JSON on stdout; errors print `{"error": ...}` on stderr and exit 1.
//! Writes go through `Store::mutate`, so they back up daily and never save over changes
//! the app made; the app picks up CLI changes on its next command or window focus.

use std::path::PathBuf;

use fin_shared::{
    format_cents, months_ending, parse_amount, Category, CategoryKind, Dataset, Lang, ReportSeries, RuleKind, Transaction,
    TransactionInput, UNSORTED_CATEGORY_ID,
};
use serde_json::{json, Value};

use crate::store::{self, Store};
use crate::{backup, camt};

const IDENTIFIER: &str = "com.mikepage.fin";

pub const HELP: &str = "\
fin-cli: read and change Fin's data. Output is JSON.

Reading
  path                              data file in use
  accounts                          accounts with balance
  balances                          bank balances per account and whether they add up
  balances import FILE.csv...       a bank's account balances CSV (date,IBAN,name,
                                    currency,amount): balances at the end of that day
  balances remove YYYY-MM-DD        remove that date's balance from every account
  categories [--all]                enabled categories (--all: also disabled)
  rules [CATEGORY]                  rules per category
  transactions [filters]            newest first
      --month YYYY-MM | --from YYYY-MM-DD --to YYYY-MM-DD | --year YYYY
      --account A  --category C  --search TEXT  --amount 12,50  --unsorted
      --limit N (default 200, 0 = all)
  overview [--month YYYY-MM]        budget vs actual (default: this month)
  report [--year YYYY] [--series S] monthly values; S = expenses (default), fixed,
                                    variable, income, saldo, or a category
  cashflow [--months N] [--to YYYY-MM]
  forecast [--year YYYY] [--current YYYY-MM] [--pace]
                                    how the year ends: complete months actual, then budget
                                    (--pace: variable at the average of the last 6 months)
  contracts                         contracts; energy: cost per usage period and, with
                                    net metering, the surplus (returned beyond used)
  energy [--year YYYY]              energy cost per month (estimated from the year before
                                    where usage is missing), monthly payments, balance
  imports                           import log
  backups                           backups, newest first

Changing
  categorize TX_ID CATEGORY         set one transaction's category
  split TX_ID AMOUNT CATEGORY [AMOUNT CATEGORY ...] | split TX_ID none
                                    count parts in other categories (holiday pay in the
                                    salary); the rest stays; none removes the split
  set-date TX_ID YYYY-MM-DD|none    the day it counts on (a reversal in September
                                    for an August payment counts in August)
  recategorize                      run all rules over the unsorted transactions
  rule add CATEGORY KIND VALUE [--apply-from YYYY-MM-DD]
                                    KIND = iban, text, in (money in), out (money out);
                                    fills unsorted matches, --apply-from also overrides
                                    categorised ones from that date
  rule remove CATEGORY KIND VALUE
  category add NAME GROUP KIND      a custom category; KIND = income, irregularincome,
                                    fixed, variable or investment. A group holds fixed
                                    or variable costs, never both
  category group CATEGORY GROUP     move a custom category to another group (of its kind)
  budget set CATEGORY YYYY-MM AMOUNT|none
                                    income and fixed costs, per category (month
                                    budget); extra income and investments (year budget)
  budget group GROUP variable YYYY-MM AMOUNT|none
                                    variable costs, one budget per group for its
                                    variable categories together
  budget average YEAR LAST_MONTH MONTHS [--overwrite]
                                    every month of YEAR from the average: per category,
                                    variable costs per group
  import [ACCOUNT] FILE.xml...      CAMT.053; backs up first. Without ACCOUNT each
                                    statement goes to the account with its IBAN
  contract save FILE.json           add (no id) or replace a contract, as `contracts` shows
  contract usage CONTRACT START END USED_NORMAL USED_LOW RETURNED_NORMAL RETURNED_LOW
                                    a period's meter readings in kWh (replaces that start)
  contract settle CONTRACT DATE AMOUNT|none [NOTE]
                                    a year-end or final bill paid on DATE: back (positive)
                                    or on top (negative); none removes it
  contract remove CONTRACT
  undo-import IMPORT_ID
  backup                            manual backup
  restore BACKUP_NAME               backs up the current state first
  language [nl-NL|en]               the app's language (standard category names follow)

CATEGORY is an id, a catalog key (groceries) or a name (Boodschappen, Groceries).
ACCOUNT is an id, a name or an IBAN. Amounts are cents in JSON, with a
formatted copy. --data FILE (or FIN_DATA_FILE) uses another data file.
";

type R<T> = Result<T, String>;

/// Fin's data file: `--data`, else FIN_DATA_FILE, else the app's data dir.
pub fn data_file(flag: Option<String>) -> R<PathBuf> {
    if let Some(p) = flag.or_else(|| std::env::var("FIN_DATA_FILE").ok()) {
        return Ok(p.into());
    }
    let home = std::env::var_os("HOME").ok_or("HOME is not set")?;
    Ok(PathBuf::from(home).join("Library/Application Support").join(IDENTIFIER).join("fin.json"))
}

/// Runs one command line (without the program name) and returns its JSON output.
pub fn run(args: Vec<String>) -> R<Value> {
    let mut a = Args::parse(args)?;
    if a.pos.is_empty() || a.has("help") || a.pos[0] == "help" {
        return Ok(Value::String(HELP.into()));
    }
    let mut s = Store::load(data_file(a.opt("data"))?)?;
    let cmd = a.pos.remove(0);
    let out = match cmd.as_str() {
        "path" => json!({ "data_file": s.path().display().to_string() }),
        "accounts" => accounts(&s.data),
        "categories" => categories(&s.data, a.has("all")),
        "rules" => rules(&s.data, a.pos.first().map(String::as_str))?,
        "transactions" | "tx" => transactions(&s.data, &a)?,
        "overview" => {
            let month = a.opt("month").unwrap_or_else(this_month);
            check_month(&month)?;
            serde_json::to_value(s.data.budget_overview_grouped(&month)).map_err(|e| e.to_string())?
        }
        "report" => report(&s.data, &a)?,
        "forecast" => {
            let now = this_month();
            let year: i32 = a.opt("year").unwrap_or_else(|| now[..4].to_string()).parse().map_err(|_| "Not a year".to_string())?;
            // The month in progress: earlier months are actual, later ones forecast.
            let current = a.opt("current").unwrap_or(now);
            // --pace: variable spending to come at the average of the last six months.
            let months = if a.has("pace") { s.data.forecast_at_pace(year, &current, &this_month()) } else { s.data.forecast(year, &current) };
            let sum = |f: fn(&fin_shared::KindTotals) -> i64| months.iter().map(|m| f(&m.totals)).sum::<i64>();
            let result = sum(|t| t.saldo());
            json!({
                "year": year,
                "current_month": current,
                "income_cents": sum(|t| t.income),
                "fixed_cents": sum(|t| t.fixed),
                "variable_cents": sum(|t| t.variable),
                "investment_cents": sum(|t| t.investment),
                "result_cents": result,
                "result": money(result),
                "months": months.iter().map(|m| json!({ "month": m.month, "actual": m.actual, "result": money(m.totals.saldo()), "result_cents": m.totals.saldo() })).collect::<Vec<_>>(),
            })
        }
        "cashflow" => cashflow(&s.data, &a)?,
        "imports" => serde_json::to_value(&s.data.imports).map_err(|e| e.to_string())?,
        "backups" => serde_json::to_value(s.list_backups()?).map_err(|e| e.to_string())?,
        "categorize" => {
            let [tx, cat] = a.take::<2>("categorize TX_ID CATEGORY")?;
            let cat = category(&s.data, &cat)?.id.clone();
            s.mutate(|ds| {
                let t = ds.transactions.iter().find(|t| t.id == tx).ok_or("Transaction not found")?;
                let input = TransactionInput {
                    id: Some(t.id.clone()),
                    date: t.date.clone(),
                    description: t.description.clone(),
                    amount_cents: t.amount_cents,
                    category_id: Some(cat),
                    account_id: t.account_id.clone(),
                };
                store::save_transaction(ds, input)
            })?;
            let t = s.data.transactions.iter().find(|t| t.id == tx).ok_or("Transaction not found")?;
            tx_json(&s.data, t)
        }
        "balances" => {
            // `balances import FILE.csv...` stores bank balances; `balances` lists them
            // per account with the check against the transactions.
            if a.pos.first().map(String::as_str) == Some("remove") {
                // `balances remove YYYY-MM-DD`: drop that date's balance from every account.
                let [_, date] = a.take::<2>("balances remove YYYY-MM-DD")?;
                let ids: Vec<String> = s.data.accounts.iter().filter(|x| x.balances.iter().any(|b| b.date == date)).map(|x| x.id.clone()).collect();
                for id in &ids {
                    s.mutate(|ds| store::set_account_balance(ds, id, &date, None))?;
                }
                json!({ "removed": ids.len() })
            } else if a.pos.first().map(String::as_str) == Some("import") {
                let mut stored = 0;
                for path in &a.pos[1..] {
                    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
                    stored += s.mutate(|ds| store::import_balances(ds, &text))?;
                }
                json!({ "stored": stored })
            } else {
                s.data
                    .accounts
                    .iter()
                    .map(|acc| {
                        let checks: Vec<Value> = s.data.balance_checks(&acc.id).iter().map(|c| json!({
                            "date": c.date,
                            "bank": money(c.bank_cents),
                            "computed": money(c.computed_cents),
                            "difference": money(c.difference()),
                            "difference_cents": c.difference(),
                        })).collect();
                        let balance = s.data.account_balance(&acc.id);
                        json!({
                            "account": acc.name,
                            "iban": acc.iban,
                            "balances": acc.balances.iter().map(|b| json!({ "date": b.date, "amount": money(b.cents) })).collect::<Vec<_>>(),
                            "checks": checks,
                            "balance_now": money(balance),
                        })
                    })
                    .collect()
            }
        }
        "contracts" => s.data.contracts.iter().map(contract_json).collect(),
        "energy" => {
            let year: i32 = a.opt("year").unwrap_or_else(|| this_month()[..4].to_string()).parse().map_err(|_| "Not a year".to_string())?;
            fin_shared::contract::energy_months(&s.data.contracts, year)
                .iter()
                .map(|m| json!({
                    "month": m.month,
                    "electricity": money(m.electricity_cents),
                    "electricity_estimated": m.electricity_estimated,
                    "gas": money(m.gas_cents),
                    "gas_estimated": m.gas_estimated,
                    "salderen_settlement": money(m.settlement_cents),
                    "payment": money(m.payment_cents),
                    "bill": money(m.bill_cents),
                    "bill_expected": money(m.bill_expected_cents),
                    "balance": money(m.balance_cents()),
                    "balance_cents": m.balance_cents(),
                }))
                .collect()
        }
        "contract" => match a.pos.first().map(String::as_str) {
            Some("save") => {
                let [_, path] = a.take::<2>("contract save FILE.json")?;
                let text = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
                let c: fin_shared::Contract = serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
                let id = s.mutate(|ds| store::save_contract(ds, c))?;
                contract_json(contract(&s.data, &id)?)
            }
            Some("usage") => {
                let [_, c, start, end, un, ul, rn, rl] =
                    a.take::<8>("contract usage CONTRACT START END USED_NORMAL USED_LOW RETURNED_NORMAL RETURNED_LOW")?;
                let kwh = |v: &str| v.replace(',', ".").parse::<f64>().map_err(|_| format!("Not a number: {v}"));
                let p = fin_shared::UsagePeriod {
                    start,
                    end,
                    used_normal: kwh(&un)?,
                    used_low: kwh(&ul)?,
                    returned_normal: kwh(&rn)?,
                    returned_low: kwh(&rl)?,
                };
                let mut c = contract(&s.data, &c)?.clone();
                c.usage.retain(|x| x.start != p.start);
                c.usage.push(p);
                let id = s.mutate(|ds| store::save_contract(ds, c))?;
                contract_json(contract(&s.data, &id)?)
            }
            Some("settle") => {
                // `contract settle CONTRACT DATE AMOUNT|none [NOTE]`: a year-end or final
                // bill paid on DATE (positive = money back); none removes that date's.
                let usage = "contract settle CONTRACT DATE AMOUNT|none [NOTE]";
                if a.pos.len() < 4 {
                    return Err(usage.into());
                }
                let (c, date, amount) = (a.pos[1].clone(), a.pos[2].clone(), a.pos[3].clone());
                let note = a.pos[4..].join(" ");
                let mut c = contract(&s.data, &c)?.clone();
                c.settlements.retain(|x| x.date != date);
                if amount != "none" {
                    let cents = parse_amount(&amount).ok_or_else(|| format!("Invalid amount: {amount}"))?;
                    c.settlements.push(fin_shared::contract::Settlement { date, cents, note });
                }
                let id = s.mutate(|ds| store::save_contract(ds, c))?;
                contract_json(contract(&s.data, &id)?)
            }
            Some("remove") => {
                let [_, c] = a.take::<2>("contract remove CONTRACT")?;
                let id = contract(&s.data, &c)?.id.clone();
                s.mutate(|ds| store::delete_contract(ds, &id))?;
                json!({ "removed": id })
            }
            _ => return Err("contract save FILE.json | usage ... | remove CONTRACT".into()),
        },
        "split" => {
            let usage = "split TX_ID AMOUNT CATEGORY [AMOUNT CATEGORY ...] | split TX_ID none";
            let mut rest = std::mem::take(&mut a.pos);
            if rest.is_empty() {
                return Err(format!("Usage: fin-cli {usage}"));
            }
            let tx = rest.remove(0);
            let mut parts = Vec::new();
            if rest.first().map(String::as_str) != Some("none") {
                if rest.is_empty() || rest.len() % 2 != 0 {
                    return Err(format!("Usage: fin-cli {usage}"));
                }
                for pair in rest.chunks(2) {
                    let cents = parse_amount(&pair[0]).ok_or(format!("Not an amount: {}", pair[0]))?.abs();
                    parts.push((cents, category(&s.data, &pair[1])?.id.clone()));
                }
            }
            s.mutate(|ds| store::set_splits(ds, &tx, parts))?;
            let t = s.data.transactions.iter().find(|t| t.id == tx).ok_or("Transaction not found")?;
            tx_json(&s.data, t)
        }
        "set-date" => {
            let [tx, day] = a.take::<2>("set-date TX_ID YYYY-MM-DD|none")?;
            let day = Some(day).filter(|d| d != "none");
            s.mutate(|ds| store::set_counts_on(ds, &tx, day))?;
            let t = s.data.transactions.iter().find(|t| t.id == tx).ok_or("Transaction not found")?;
            tx_json(&s.data, t)
        }
        "recategorize" => {
            let sorted = s.mutate(|ds| Ok(store::categorize_unsorted(ds)))?;
            let left = s.data.transactions.iter().filter(|t| t.is_unsorted()).count();
            json!({ "categorized": sorted, "still_unsorted": left })
        }
        "rule" => rule(&mut s, &mut a)?,
        "category" if a.pos.first().map(String::as_str) == Some("group") => {
            // `category group CATEGORY GROUP`: move a custom category to the end of another group.
            let [_, cat, group] = a.take::<3>("category group CATEGORY GROUP")?;
            let mut c = category(&s.data, &cat)?.clone();
            c.group = group;
            let id = c.id.clone();
            s.mutate(|ds| {
                store::update_category(ds, c)?;
                let i = ds.categories.iter().position(|x| x.id == id).ok_or("Category not found")?;
                let moved = ds.categories.remove(i);
                let at = ds.categories.iter().rposition(|x| x.group == moved.group).map_or(ds.categories.len(), |j| j + 1);
                ds.categories.insert(at, moved);
                Ok(())
            })?;
            let c = category(&s.data, &id)?;
            json!({ "id": c.id, "name": c.name, "group": c.group })
        }
        "category" => {
            // `category add NAME GROUP KIND`: a custom category at the end of its group.
            let [sub, name, group, kind] = a.take::<4>("category add NAME GROUP KIND")?;
            if sub != "add" {
                return Err("category add NAME GROUP KIND | category group CATEGORY GROUP".into());
            }
            let kind = CategoryKind::ALL
                .into_iter()
                .find(|k| k.key().eq_ignore_ascii_case(&kind))
                .ok_or("KIND = income, irregularincome, fixed, variable or investment")?;
            s.mutate(|ds| store::add_category(ds, &name, &group, kind))?;
            let c = category(&s.data, &name)?;
            json!({ "id": c.id, "name": c.name, "group": c.group, "kind": c.kind.key() })
        }
        "budget" => budget(&mut s, &mut a)?,
        "import" => import(&mut s, &mut a)?,
        "undo-import" => {
            let [id] = a.take::<1>("undo-import IMPORT_ID")?;
            let at = backup::rfc3339(backup::now_secs());
            let removed = s.mutate(|ds| store::undo_import(ds, &id, &at))?;
            json!({ "removed": removed })
        }
        "backup" => {
            let b = s.backup(backup::MANUAL)?.ok_or("Nothing to back up yet")?;
            serde_json::to_value(b).map_err(|e| e.to_string())?
        }
        "language" => {
            if let Some(tag) = a.pos.first() {
                let lang = Lang::from_tag(tag).ok_or(format!("Language is nl-NL or en, not '{tag}'"))?;
                s.mutate(|ds| store::set_language(ds, lang))?;
            }
            json!({ "language": s.data.language })
        }
        "restore" => {
            let [name] = a.take::<1>("restore BACKUP_NAME")?;
            s.restore_backup(&name)?;
            json!({ "restored": name })
        }
        other => return Err(format!("Unknown command '{other}'. Run fin-cli help.")),
    };
    Ok(out)
}

/// Positional arguments and `--flag [value]` options.
struct Args {
    pos: Vec<String>,
    opts: Vec<(String, Option<String>)>,
}

/// Options that take no value.
const SWITCHES: &[&str] = &["all", "unsorted", "overwrite", "pace", "help"];

impl Args {
    fn parse(args: Vec<String>) -> R<Self> {
        let mut pos = Vec::new();
        let mut opts = Vec::new();
        let mut it = args.into_iter();
        while let Some(arg) = it.next() {
            match arg.strip_prefix("--") {
                Some(name) if SWITCHES.contains(&name) => opts.push((name.to_string(), None)),
                Some(name) => {
                    let v = it.next().ok_or(format!("--{name} needs a value"))?;
                    opts.push((name.to_string(), Some(v)));
                }
                None => pos.push(arg),
            }
        }
        Ok(Self { pos, opts })
    }

    fn opt(&self, name: &str) -> Option<String> {
        self.opts.iter().rev().find(|(n, _)| n == name).and_then(|(_, v)| v.clone())
    }

    fn has(&self, name: &str) -> bool {
        self.opts.iter().any(|(n, _)| n == name)
    }

    /// Exactly N more positional arguments.
    fn take<const N: usize>(&mut self, usage: &str) -> R<[String; N]> {
        let rest = std::mem::take(&mut self.pos);
        rest.try_into().map_err(|_| format!("Usage: fin-cli {usage}"))
    }
}

fn this_month() -> String {
    backup::rfc3339(backup::now_secs())[..7].to_string()
}

fn check_month(m: &str) -> R<()> {
    let ok = m.len() == 7 && m.as_bytes()[4] == b'-' && m[5..].parse::<u32>().is_ok_and(|n| (1..=12).contains(&n));
    ok.then_some(()).ok_or(format!("Not a month (YYYY-MM): {m}"))
}

fn money(cents: i64) -> Value {
    json!(format_cents(cents))
}

/// A contract by id or name (case-insensitive).
fn contract<'a>(ds: &'a Dataset, query: &str) -> R<&'a fin_shared::Contract> {
    ds.contracts
        .iter()
        .find(|c| c.id == query)
        .or_else(|| ds.contracts.iter().find(|c| c.name.eq_ignore_ascii_case(query)))
        .ok_or_else(|| format!("No contract {query}"))
}

/// The contract as stored, plus per usage period the cost with net metering (as the
/// supplier reports it) and, with net metering, the surplus (under the `salderen` key).
fn contract_json(c: &fin_shared::Contract) -> Value {
    let mut v = serde_json::to_value(c).unwrap_or_default();
    if let Some(e) = &c.energy {
        let cost = |p: &fin_shared::PeriodCost| json!({
            "usage": money(p.usage_cents),
            "return": money(p.return_cents),
            "fixed": money(p.fixed_cents),
            "total": money(p.total_cents),
            "total_cents": p.total_cents,
        });
        let periods: Vec<Value> = c.usage.iter().map(|p| json!({
            "start": p.start,
            "end": p.end,
            "used_kwh": p.used(),
            "returned_kwh": p.returned(),
            "cost": cost(&e.period_cost(p)),
        })).collect();
        v["periods"] = json!(periods);
        v["price_normal"] = json!(e.price_normal());
        v["feed_in_value_per_kwh"] = json!(if e.netting { e.price_normal() } else { e.feed_in_net() });
        if e.netting {
            let pos = e.netting_position(&c.usage);
            v["salderen"] = json!({
                "used_kwh": pos.used,
                "returned_kwh": pos.returned,
                "surplus_kwh": pos.surplus,
                "surplus_loss": money(pos.surplus_loss_cents),
            });
        }
    }
    v
}

fn category<'a>(ds: &'a Dataset, query: &str) -> R<&'a Category> {
    let q = query.trim();
    let lower = q.to_lowercase();
    ds.categories
        .iter()
        .find(|c| c.id == q || c.id.strip_prefix("sys-") == Some(q))
        .or_else(|| ds.categories.iter().find(|c| c.name.to_lowercase() == lower))
        // A standard category also by its name in the other language.
        .or_else(|| {
            ds.categories.iter().filter(|c| c.system).find(|c| {
                Lang::ALL.iter().any(|l| crate::locale::names(*l).category_name(&c.id).to_lowercase() == lower)
            })
        })
        .ok_or_else(|| format!("Category not found: {q}"))
}

fn account_id(ds: &Dataset, query: &str) -> R<String> {
    let q = query.trim();
    let lower = q.to_lowercase();
    let iban = fin_shared::normalize_iban(q);
    ds.accounts
        .iter()
        .find(|a| a.id == q || a.name.to_lowercase() == lower || (iban.is_some() && a.iban == iban))
        .map(|a| a.id.clone())
        .ok_or_else(|| format!("Account not found: {q}"))
}

fn rule_kind(s: &str) -> R<RuleKind> {
    match s.to_lowercase().as_str() {
        "iban" => Ok(RuleKind::Iban),
        "text" => Ok(RuleKind::Text),
        "in" | "text-in" => Ok(RuleKind::TextIn),
        "out" | "text-out" => Ok(RuleKind::TextOut),
        _ => Err(format!("Rule kind is iban, text, in or out, not '{s}'")),
    }
}

fn kind_key(k: RuleKind) -> &'static str {
    match k {
        RuleKind::Iban => "iban",
        RuleKind::Text => "text",
        RuleKind::TextIn => "in",
        RuleKind::TextOut => "out",
    }
}

fn accounts(ds: &Dataset) -> Value {
    ds.accounts
        .iter()
        .map(|a| {
            let balance = ds.account_balance(&a.id);
            json!({ "id": a.id, "name": a.name, "iban": a.iban, "balance_cents": balance, "balance": money(balance) })
        })
        .collect()
}

fn cat_json(c: &Category) -> Value {
    json!({
        "id": c.id,
        "name": c.name,
        "group": c.group,
        "kind": c.kind.key(),
        "system": c.system,
        "disabled": c.disabled,
        "rules": c.rule_count(),
    })
}

fn categories(ds: &Dataset, all: bool) -> Value {
    ds.categories.iter().filter(|c| all || !c.disabled).map(cat_json).collect()
}

fn rules(ds: &Dataset, only: Option<&str>) -> R<Value> {
    let only = only.map(|q| category(ds, q)).transpose()?.map(|c| c.id.clone());
    Ok(ds
        .categories
        .iter()
        .filter(|c| only.as_ref().is_none_or(|id| &c.id == id))
        .filter(|c| c.rule_count() > 0 || only.is_some())
        .map(|c| {
            // Personal rules (removable) and the locale's defaults (locked), per kind.
            let (mut rules, mut defaults) = (serde_json::Map::new(), serde_json::Map::new());
            for k in RuleKind::ALL {
                let (d, p): (Vec<&String>, Vec<&String>) =
                    c.rules(k).iter().partition(|v| store::is_default_rule(&c.id, k, v));
                if !p.is_empty() {
                    rules.insert(kind_key(k).into(), json!(p));
                }
                if !d.is_empty() {
                    defaults.insert(kind_key(k).into(), json!(d));
                }
            }
            json!({ "id": c.id, "name": c.name, "rules": rules, "default_rules": defaults })
        })
        .collect())
}

fn tx_json(ds: &Dataset, t: &Transaction) -> Value {
    let account = ds.accounts.iter().find(|a| a.id == t.account_id).map(|a| a.name.as_str());
    let cat = ds.category(t.category_id.as_deref());
    json!({
        "id": t.id,
        "date": t.date,
        "counts_on": t.counts_on,
        "splits": t.splits.iter().map(|s| json!({
            "amount": money(s.amount_cents),
            "category": ds.category(Some(&s.category_id)).map(|c| c.name.as_str()),
        })).collect::<Vec<_>>(),
        "description": t.description,
        "amount_cents": t.amount_cents,
        "amount": money(t.amount_cents),
        "account": account,
        "category_id": t.category_id,
        "category": cat.map(|c| c.name.as_str()),
        "counterparty_iban": t.counterparty_iban,
        "channel": t.channel().map(|c| c.label()),
        "import_id": t.import_id,
    })
}

fn transactions(ds: &Dataset, a: &Args) -> R<Value> {
    let (from, to) = match (a.opt("month"), a.opt("year"), a.opt("from"), a.opt("to")) {
        (Some(m), ..) => {
            check_month(&m)?;
            (format!("{m}-01"), format!("{m}-31"))
        }
        (None, Some(y), ..) => (format!("{y}-01-01"), format!("{y}-12-31")),
        (None, None, from, to) => (from.unwrap_or_default(), to.unwrap_or_else(|| "9999".into())),
    };
    let account = a.opt("account").map(|q| account_id(ds, &q)).transpose()?;
    let cat = a.opt("category").map(|q| category(ds, &q).map(|c| c.id.clone())).transpose()?;
    let search = a.opt("search").map(|s| s.to_lowercase());
    let amount = match a.opt("amount") {
        Some(s) => Some(parse_amount(&s).ok_or(format!("Not an amount: {s}"))?.abs()),
        None => None,
    };
    let limit: usize = a.opt("limit").map_or(Ok(200), |l| l.parse().map_err(|_| format!("Not a number: {l}")))?;
    let unsorted = a.has("unsorted");

    let matching: Vec<&Transaction> = ds
        .transactions
        .iter()
        .filter(|t| t.day() >= from.as_str() && t.day() <= to.as_str())
        .filter(|t| account.as_ref().is_none_or(|id| &t.account_id == id))
        .filter(|t| cat.as_ref().is_none_or(|id| t.category_id.as_ref() == Some(id) || t.splits.iter().any(|s| &s.category_id == id)))
        .filter(|t| !unsorted || t.is_unsorted())
        .filter(|t| amount.is_none_or(|w| t.amount_cents.abs() == w))
        .filter(|t| {
            search.as_ref().is_none_or(|q| {
                t.description.to_lowercase().contains(q)
                    || t.counterparty_iban.as_ref().is_some_and(|i| i.to_lowercase().contains(q))
            })
        })
        .collect();
    let total: i64 = matching.iter().map(|t| t.amount_cents).sum();
    let shown: Vec<Value> =
        matching.iter().take(if limit == 0 { usize::MAX } else { limit }).map(|t| tx_json(ds, t)).collect();
    Ok(json!({
        "count": matching.len(),
        "shown": shown.len(),
        "total_cents": total,
        "total": money(total),
        "transactions": shown,
    }))
}

fn report(ds: &Dataset, a: &Args) -> R<Value> {
    let year = a.opt("year").unwrap_or_else(|| this_month()[..4].to_string());
    year.parse::<i32>().map_err(|_| format!("Not a year: {year}"))?;
    let name = a.opt("series").unwrap_or_else(|| "expenses".into());
    let series = match name.as_str() {
        "expenses" => ReportSeries::ExpensesTotal,
        "fixed" => ReportSeries::ExpensesFixed,
        "variable" => ReportSeries::ExpensesVariable,
        "income" => ReportSeries::IncomeTotal,
        "saldo" => ReportSeries::Saldo,
        other => ReportSeries::Category(category(ds, other)?.id.clone()),
    };
    let months: Vec<String> = (1..=12).map(|m| format!("{year}-{m:02}")).collect();
    let values = ds.report_series(&months, &series);
    let total: i64 = values.iter().sum();
    let rows: Vec<Value> = months
        .iter()
        .zip(&values)
        .map(|(m, v)| json!({ "month": m, "cents": v, "amount": money(*v) }))
        .collect();
    Ok(json!({ "year": year, "series": name, "total_cents": total, "total": money(total), "months": rows }))
}

fn cashflow(ds: &Dataset, a: &Args) -> R<Value> {
    let to = a.opt("to").unwrap_or_else(this_month);
    check_month(&to)?;
    let n: usize = a.opt("months").map_or(Ok(12), |n| n.parse().map_err(|_| format!("Not a number: {n}")))?;
    let rows: Vec<Value> = ds
        .cash_flow(&months_ending(&to, n))
        .iter()
        .map(|f| {
            json!({
                "month": f.month,
                "income_cents": f.income_cents,
                "expense_cents": f.expense_cents,
                "net_cents": f.net_cents(),
                "net": money(f.net_cents()),
            })
        })
        .collect();
    Ok(Value::Array(rows))
}

fn rule(s: &mut Store, a: &mut Args) -> R<Value> {
    let usage = "rule add|remove CATEGORY KIND VALUE [--apply-from YYYY-MM-DD]";
    let [action, cat, kind, value] = a.take::<4>(usage)?;
    let cat = category(&s.data, &cat)?.id.clone();
    let kind = rule_kind(&kind)?;
    match action.as_str() {
        "add" => {
            let filled = s.mutate(|ds| store::add_category_rule(ds, &cat, kind, &value))?;
            let overridden = match a.opt("apply-from") {
                Some(from) => s.mutate(|ds| store::apply_category_rule(ds, &cat, kind, &value, &from))?,
                None => 0,
            };
            Ok(json!({ "category_id": cat, "kind": kind_key(kind), "value": value, "filled": filled, "applied_from": overridden }))
        }
        "remove" => {
            s.mutate(|ds| store::remove_category_rule(ds, &cat, kind, &value))?;
            Ok(json!({ "removed": { "category_id": cat, "kind": kind_key(kind), "value": value } }))
        }
        _ => Err(format!("Usage: fin-cli {usage}")),
    }
}

fn budget(s: &mut Store, a: &mut Args) -> R<Value> {
    match a.pos.first().map(String::as_str) {
        Some("set") => {
            let [_, cat, month, amount] = a.take::<4>("budget set CATEGORY YYYY-MM AMOUNT|none")?;
            check_month(&month)?;
            let cat = category(&s.data, &cat)?;
            if cat.kind == CategoryKind::Transfer || cat.id == UNSORTED_CATEGORY_ID {
                return Err(format!("{} has no budget", cat.name));
            }
            let id = cat.id.clone();
            let cents = match amount.as_str() {
                "none" | "-" | "" => None,
                s => Some(parse_amount(s).ok_or(format!("Not an amount: {s}"))?.abs()),
            };
            s.mutate(|ds| store::set_budget(ds, &id, &month, cents))?;
            // What was stored: an amount that doesn't fit the year's income is capped.
            let stored = s.data.budget_for(&id, &month);
            Ok(json!({ "category_id": id, "month": month, "amount_cents": stored, "capped": stored != cents }))
        }
        Some("average") => {
            let [_, year, last, months] = a.take::<4>("budget average YEAR LAST_MONTH MONTHS [--overwrite]")?;
            let year: i32 = year.parse().map_err(|_| format!("Not a year: {year}"))?;
            check_month(&last)?;
            let months: u32 = months.parse().map_err(|_| format!("Not a number: {months}"))?;
            let overwrite = a.has("overwrite");
            let set = s.mutate(|ds| store::budgets_from_average(ds, year, &last, months, overwrite))?;
            Ok(json!({ "budgets_set": set }))
        }
        Some("group") => {
            let [_, group, kind, month, amount] = a.take::<5>("budget group GROUP variable YYYY-MM AMOUNT|none")?;
            check_month(&month)?;
            let group = group_name(&s.data, &group)?;
            let kind = match kind.to_lowercase().as_str() {
                "variable" | "variabel" => CategoryKind::Variable,
                "fixed" | "vast" => {
                    return Err("Group budgets are for variable costs: budget fixed costs per category with `budget set`".into())
                }
                k => return Err(format!("Not variable: {k}")),
            };
            let cents = match amount.as_str() {
                "none" | "-" | "" => None,
                s => Some(parse_amount(s).ok_or(format!("Not an amount: {s}"))?.abs()),
            };
            s.mutate(|ds| store::set_group_budget(ds, &group, kind, &month, cents))?;
            let stored = s.data.group_budget_for(&group, kind, &month);
            Ok(json!({ "group": group, "kind": kind, "month": month, "amount_cents": stored, "capped": stored != cents }))
        }
        _ => Err("Usage: fin-cli budget set|group|average ...".into()),
    }
}

/// A group as the categories have it, by its name in either language (any case) or
/// its catalog key ("transport").
fn group_name(ds: &Dataset, query: &str) -> R<String> {
    let q = query.trim();
    let lower = q.to_lowercase();
    let current = crate::locale::names(ds.language);
    let is_key = current.group_name(&lower) != lower;
    let key = if is_key { Some(lower.as_str()) } else { Lang::ALL.iter().find_map(|l| crate::locale::names(*l).group_key(q)) };
    let wanted = key.map(|k| current.group_name(k).to_lowercase()).unwrap_or_else(|| lower.clone());
    ds.categories
        .iter()
        .find(|c| c.group.to_lowercase() == wanted)
        .map(|c| c.group.clone())
        .ok_or_else(|| format!("Group not found: {q}"))
}

/// `import ACCOUNT FILE...` puts everything on one account. `import FILE...` sends each
/// statement to the account with its IBAN (bank exports often hold several accounts).
fn import(s: &mut Store, a: &mut Args) -> R<Value> {
    let by_iban = a.pos.first().is_some_and(|p| std::path::Path::new(p).is_file());
    let (account, paths) = match by_iban {
        true => (None, &a.pos[..]),
        false if a.pos.len() >= 2 => (Some(account_id(&s.data, &a.pos[0])?), &a.pos[1..]),
        false => return Err("Usage: fin-cli import [ACCOUNT] FILE.xml...".into()),
    };
    // Parse everything first, so a bad file imports nothing.
    let mut parsed = Vec::new();
    for path in paths {
        let xml = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        let statements = camt::parse_with_progress(&xml, |_| {}).map_err(|e| format!("{path}: {e}"))?;
        let name = std::path::Path::new(path).file_name().map_or(path.clone(), |n| n.to_string_lossy().into_owned());
        parsed.push(store::ParsedFile { name, statements });
    }
    // Group per account, keeping the file name for the import log.
    let mut per_account: Vec<(String, Vec<store::ParsedFile>)> = Vec::new();
    for file in parsed {
        if let Some(id) = &account {
            per_account.push((id.clone(), vec![file]));
            continue;
        }
        for st in file.statements {
            let iban = st.iban.clone().ok_or(format!("{}: statement without IBAN; name the account", file.name))?;
            let id = s
                .data
                .accounts
                .iter()
                .find(|a| a.iban.as_deref() == Some(iban.as_str()))
                .map(|a| a.id.clone())
                .ok_or(format!("No account with IBAN {iban}; add it first or name the account"))?;
            per_account.push((id, vec![store::ParsedFile { name: file.name.clone(), statements: vec![st] }]));
        }
    }
    s.backup(backup::BEFORE_IMPORT)?;
    let at = backup::rfc3339(backup::now_secs());
    // One write: all accounts import, or none do.
    let records = s.mutate(|ds| {
        let mut grouped: Vec<(String, Vec<store::ParsedFile>)> = Vec::new();
        for (id, files) in per_account {
            match grouped.iter_mut().find(|(g, _)| *g == id) {
                Some((_, fs)) => fs.extend(files),
                None => grouped.push((id, files)),
            }
        }
        grouped.into_iter().map(|(id, files)| store::apply_import(ds, &id, files, &at)).collect::<R<Vec<_>>>()
    })?;
    let out: Vec<Value> = records
        .iter()
        .map(|r| {
            let name = s.data.accounts.iter().find(|a| a.id == r.account_id).map(|a| a.name.as_str());
            json!({ "account": name, "import": r })
        })
        .collect();
    Ok(Value::Array(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cli(file: &std::path::Path, args: &[&str]) -> R<Value> {
        let mut v: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        v.extend(["--data".into(), file.display().to_string()]);
        run(v)
    }

    #[test]
    fn import_rule_categorize_budget() {
        let dir = std::env::temp_dir().join(format!("fin-cli-{}", uuid::Uuid::new_v4()));
        let file = dir.join("fin.json");
        let xml = dir.join("a.xml");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&xml, camt::SAMPLE_V02).unwrap();
        let mut s = Store::load(file.clone()).unwrap();
        s.mutate(|ds| store::add_account(ds, "Betaal", None)).unwrap();

        let rec = cli(&file, &["import", "Betaal", xml.to_str().unwrap()]).unwrap();
        assert!(rec[0]["import"]["imported"].as_u64().unwrap() > 0);
        // Without an account, statements go by IBAN (the first import stored it on Betaal).
        let again = cli(&file, &["import", xml.to_str().unwrap()]).unwrap();
        assert_eq!(again[0]["account"], "Betaal");
        assert_eq!(again[0]["import"]["imported"], 0);
        let all = cli(&file, &["transactions", "--limit", "0"]).unwrap();
        let first = &all["transactions"][0];
        let id = first["id"].as_str().unwrap();

        let t = cli(&file, &["categorize", id, "Boodschappen"]).unwrap();
        assert_eq!(t["category_id"], "sys-groceries");
        // The language setting renames the standard categories; either name still works.
        assert_eq!(cli(&file, &["language", "en"]).unwrap()["language"], "en");
        assert_eq!(cli(&file, &["language"]).unwrap()["language"], "en");
        assert_eq!(cli(&file, &["categorize", id, "Boodschappen"]).unwrap()["category"], "Groceries");
        assert!(cli(&file, &["language", "de"]).is_err());
        cli(&file, &["language", "nl-NL"]).unwrap();
        let only = cli(&file, &["tx", "--category", "groceries", "--limit", "0"]).unwrap();
        assert!(only["transactions"].as_array().unwrap().iter().any(|t| t["id"] == id));

        cli(&file, &["rule", "add", "groceries", "text", "testwinkel"]).unwrap();
        let r = cli(&file, &["rules", "groceries"]).unwrap();
        assert!(r[0]["rules"]["text"].as_array().unwrap().iter().any(|v| v == "testwinkel"));
        cli(&file, &["rule", "remove", "groceries", "text", "testwinkel"]).unwrap();

        // Variable costs are budgeted per group, fixed costs per category.
        let err = cli(&file, &["budget", "set", "groceries", "2026-01", "400"]).unwrap_err();
        assert_eq!(err, "Variable categories are budgeted per group: set the budget on Huishouden");
        cli(&file, &["budget", "group", "Huishouden", "variable", "2026-01", "400"]).unwrap();
        cli(&file, &["budget", "set", "road_tax", "2026-01", "40"]).unwrap();
        let o = cli(&file, &["overview", "--month", "2026-01"]).unwrap();
        assert_eq!((o["budget"]["variable"].as_i64(), o["budget"]["fixed"].as_i64()), (Some(40000), Some(4000)));

        // A group budget, by name in either language or by key; the overview shows the group's line.
        let g = cli(&file, &["budget", "group", "Transport", "variable", "2026-01", "115"]).unwrap();
        assert_eq!((g["group"].as_str(), g["amount_cents"].as_i64()), (Some("Vervoer"), Some(11500)));
        assert!(cli(&file, &["budget", "group", "finances", "vast", "2026-01", "40"]).unwrap_err().starts_with("Group budgets are for variable costs"));
        assert!(cli(&file, &["budget", "group", "Nergens", "variable", "2026-01", "50"]).is_err());
        let o = cli(&file, &["overview", "--month", "2026-01"]).unwrap();
        assert_eq!(o["budget"]["variable"], 40000 + 11500);
        assert!(o["variable"].as_array().unwrap().iter().any(|l| l["group_line"] == true && l["name"] == "Vervoer"));

        // Categories respect their group's kind.
        let err = cli(&file, &["category", "add", "Parkeervergunning", "Vervoer", "fixed"]).unwrap_err();
        assert_eq!(err, "Vervoer holds variable costs: a fixed cost needs a group of fixed costs");
        cli(&file, &["category", "add", "Fietsen", "Vervoer", "variable"]).unwrap();
        let err = cli(&file, &["category", "group", "Fietsen", "Financiën"]).unwrap_err();
        assert_eq!(err, "Financiën holds fixed costs: a variable cost needs a group of variable costs");
        assert_eq!(cli(&file, &["category", "group", "Fietsen", "Huishouden"]).unwrap()["group"], "Huishouden");

        // The app's store sees CLI writes and does not save over them.
        assert!(s.reload_if_changed().unwrap());
        assert_eq!(s.data.group_budget_for("Huishouden", CategoryKind::Variable, "2026-01"), Some(40000));

        assert!(cli(&file, &["categorize", "nope", "groceries"]).is_err());
        assert!(cli(&file, &["frobnicate"]).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
