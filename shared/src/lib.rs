//! Types shared between the Tauri backend and the Leptos frontend.
//!
//! Amounts are integer cents. Positive is income, negative is expense.
//! Dates are ISO `YYYY-MM-DD` strings so this crate stays dependency-free on wasm.

use serde::{Deserialize, Serialize};

pub mod catalog;
pub mod contract;

pub use contract::{Contract, EnergyTerms, NettingPosition, PeriodCost, UsagePeriod, Utility};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub iban: Option<String>,
    /// Balances from the bank, oldest first: the first is the starting balance, later
    /// ones check that the imported transactions add up.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub balances: Vec<AccountBalance>,
}

/// The bank's balance at the end of `date` (`YYYY-MM-DD`): after that day's
/// transactions, as in a bank's account balances export.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AccountBalance {
    pub date: String,
    pub cents: i64,
}

/// A later bank balance against what the transactions since the previous one give.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BalanceCheck {
    pub date: String,
    /// The bank's balance on `date`.
    pub bank_cents: i64,
    /// The previous balance plus the transactions after it, up to and including `date`.
    pub computed_cents: i64,
}

impl BalanceCheck {
    pub fn difference(&self) -> i64 {
        self.bank_cents - self.computed_cents
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CategoryKind {
    Income,
    /// Extra income: money in that doesn't come every month (holiday pay, refunds,
    /// interest). Counts as income and in the net result; budgeted in the year budget only.
    IrregularIncome,
    /// Fixed costs
    Fixed,
    #[default]
    Variable,
    /// Investments: one-off spending such as a renovation or insulation. Counts as
    /// spending and in the net result, but apart from the regular fixed and variable
    /// costs; budgeted in the year budget only.
    Investment,
    /// Only the system category "Internal transfers": money moved between own accounts.
    /// Holds transactions but counts nowhere - not as income or expense, not in budgets,
    /// charts or reports. Not a kind users pick.
    Transfer,
}

impl CategoryKind {
    pub const ALL: [CategoryKind; 6] = [
        CategoryKind::Income,
        CategoryKind::IrregularIncome,
        CategoryKind::Fixed,
        CategoryKind::Variable,
        CategoryKind::Investment,
        CategoryKind::Transfer,
    ];
    /// The kinds a user can give a category.
    pub const SELECTABLE: [CategoryKind; 5] = [
        CategoryKind::Income,
        CategoryKind::IrregularIncome,
        CategoryKind::Fixed,
        CategoryKind::Variable,
        CategoryKind::Investment,
    ];

    /// English label (the app has its own, translated).
    pub fn label(self) -> &'static str {
        match self {
            CategoryKind::Income => "Regular income",
            CategoryKind::IrregularIncome => "Extra income",
            CategoryKind::Fixed => "Fixed costs",
            CategoryKind::Variable => "Variable",
            CategoryKind::Investment => "Investment",
            CategoryKind::Transfer => "Transfer",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            CategoryKind::Income => "Income",
            CategoryKind::IrregularIncome => "IrregularIncome",
            CategoryKind::Fixed => "Fixed",
            CategoryKind::Variable => "Variable",
            CategoryKind::Investment => "Investment",
            CategoryKind::Transfer => "Transfer",
        }
    }

    /// Money going out: fixed, variable and investments.
    pub fn is_expense(self) -> bool {
        matches!(self, CategoryKind::Fixed | CategoryKind::Variable | CategoryKind::Investment)
    }

    /// Money coming in: income and extra income.
    pub fn is_income(self) -> bool {
        matches!(self, CategoryKind::Income | CategoryKind::IrregularIncome)
    }

    /// Whether categories of this kind are budgeted: every kind that counts. The year
    /// budget holds them all, the month budget only the `in_month_budget` ones.
    pub fn budgetable(self) -> bool {
        self.counts()
    }

    /// In the month budget, what comes back every month: fixed income, fixed and
    /// variable costs. Extra income and investments are in the year budget only.
    pub fn in_month_budget(self) -> bool {
        matches!(self, CategoryKind::Income | CategoryKind::Fixed | CategoryKind::Variable)
    }

    /// Whether money in this kind counts in totals, budgets and charts.
    pub fn counts(self) -> bool {
        self != CategoryKind::Transfer
    }

    pub fn from_key(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.key() == s)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Category {
    pub id: String,
    pub name: String,
    /// Heading the category is listed under, e.g. "Housing".
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub kind: CategoryKind,
    /// One of the standard categories: can be disabled but not removed or renamed.
    #[serde(default)]
    pub system: bool,
    /// Switched off: holds no transactions, is not offered anywhere and has no budget.
    #[serde(default)]
    pub disabled: bool,
    /// Counterparty IBANs that always map to this category. An IBAN is in at most one category.
    #[serde(default)]
    pub ibans: Vec<String>,
    /// Text rules: lower-case fragments; a transaction whose description contains one
    /// maps to this category (card payments and iDEAL often have no useful IBAN).
    #[serde(default)]
    pub patterns: Vec<String>,
    /// Text rules that only apply to money coming in (for counterparties that both pay
    /// and charge, like the Belastingdienst). Used when no plain text rule matches.
    #[serde(default)]
    pub patterns_in: Vec<String>,
    /// Text rules that only apply to money going out. Used when no plain text rule matches.
    #[serde(default)]
    pub patterns_out: Vec<String>,
}

/// The kinds of rule a category can have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RuleKind {
    /// Counterparty IBAN is exactly this.
    Iban,
    /// Description contains this text (case-insensitive).
    Text,
    /// Description contains this text and the money comes in.
    TextIn,
    /// Description contains this text and the money goes out.
    TextOut,
}

impl RuleKind {
    pub const ALL: [RuleKind; 4] = [RuleKind::Iban, RuleKind::Text, RuleKind::TextIn, RuleKind::TextOut];

    pub fn is_text(self) -> bool {
        self != RuleKind::Iban
    }
}

impl Category {
    /// Whether this category takes part in budgeting: switched on and of a budgeted kind
    /// (not the transfer category).
    pub fn budgetable(&self) -> bool {
        !self.disabled && self.kind.budgetable()
    }

    /// The rules of one kind.
    pub fn rules(&self, kind: RuleKind) -> &Vec<String> {
        match kind {
            RuleKind::Iban => &self.ibans,
            RuleKind::Text => &self.patterns,
            RuleKind::TextIn => &self.patterns_in,
            RuleKind::TextOut => &self.patterns_out,
        }
    }

    pub fn rules_mut(&mut self, kind: RuleKind) -> &mut Vec<String> {
        match kind {
            RuleKind::Iban => &mut self.ibans,
            RuleKind::Text => &mut self.patterns,
            RuleKind::TextIn => &mut self.patterns_in,
            RuleKind::TextOut => &mut self.patterns_out,
        }
    }

    pub fn rule_count(&self) -> usize {
        RuleKind::ALL.iter().map(|k| self.rules(*k).len()).sum()
    }
}

/// Which rule sorts a transaction (see Dataset::matching_rule). `kind` is None when the
/// counterparty is one of the own accounts; `value` is then that IBAN.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleMatch {
    pub category_id: String,
    pub kind: Option<RuleKind>,
    pub value: String,
}

/// One rule as listed on the rules page; `default` rules come with the locale and
/// can't be removed (a personal rule can override them).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleInfo {
    pub category_id: String,
    pub kind: RuleKind,
    pub value: String,
    pub default: bool,
}

/// Where a payment was made: derived from the description, not stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Channel {
    Online,
    InStore,
}

impl Channel {
    pub fn label(self) -> &'static str {
        match self {
            Channel::Online => "Online",
            Channel::InStore => "In store",
        }
    }
}

/// Payment types that only happen at a till: tapping a card or phone, PIN. These win
/// over everything ("Google Pay betaling" is a phone at a till, not Google).
const TILL_MARKERS: &[&str] = &[
    "contactloze betaling", "google pay betaling", "apple pay betaling", "pinbetaling", "betaalautomaat", "bea",
];
/// Online signs: iDEAL, payment processors and online-only shops.
const ONLINE_MARKERS: &[&str] = &[
    "ideal", "mollie", "buckaroo", "multisafepay", "adyen", "stripe", "worldline", "mangopay", "klarna",
    "paypal", "riverty", "cm.com", "online payments", "bol.com", "amazon", "zalando", "vinted",
    "marktplaats", "thuisbezorgd", "google", "apple.com",
];

impl Transaction {
    /// Online or in a shop, when the description tells. In order: a till payment type
    /// (in store); iDEAL, a payment processor or an online-only shop (online); a card
    /// payment with card data (MCC) but no till type, i.e. card not present (online);
    /// any other card payment (">place", "betaalpas", "CCV"; in store). `None` for
    /// transfers, direct debits and anything else without such a sign.
    pub fn channel(&self) -> Option<Channel> {
        let d = &self.description;
        if TILL_MARKERS.iter().any(|m| contains_words(d, m)) {
            return Some(Channel::InStore);
        }
        if ONLINE_MARKERS.iter().any(|m| contains_words(d, m)) {
            return Some(Channel::Online);
        }
        let card = d.contains('>');
        if card && contains_words(d, "mcc") {
            return Some(Channel::Online);
        }
        if card || contains_words(d, "betaalpas") || contains_words(d, "ccv") {
            return Some(Channel::InStore);
        }
        None
    }
}

/// The standard category "To categorise": where imports land when no rule matches.
pub const UNSORTED_CATEGORY_ID: &str = catalog::ids::UNSORTED;

impl Transaction {
    /// Still to be categorised: no category, or "To categorise".
    pub fn is_unsorted(&self) -> bool {
        self.category_id.as_deref().is_none_or(|c| c == UNSORTED_CATEGORY_ID)
    }
}

/// Normalises a rule value: an IBAN (spaces removed, upper case, shape checked) or a
/// text fragment (see normalize_pattern).
pub fn normalize_rule(kind: RuleKind, value: &str) -> Option<String> {
    match kind {
        RuleKind::Iban => normalize_iban(value),
        RuleKind::Text | RuleKind::TextIn | RuleKind::TextOut => normalize_pattern(value),
    }
}

/// Whether a normalised rule applies to a transaction.
pub fn rule_matches(kind: RuleKind, value: &str, t: &Transaction) -> bool {
    match kind {
        RuleKind::Iban => t.counterparty_iban.as_deref() == Some(value),
        RuleKind::Text => contains_words(&t.description, value),
        RuleKind::TextIn => t.amount_cents >= 0 && contains_words(&t.description, value),
        RuleKind::TextOut => t.amount_cents < 0 && contains_words(&t.description, value),
    }
}

/// Text reduced to lower-case words separated by single spaces; punctuation counts as a
/// space, so "KPN - Mobiel", "Nationale-Nederlanden" and "bol.com" compare as
/// "kpn mobiel", "nationale nederlanden" and "bol com".
pub fn match_form(text: &str) -> String {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// `pattern` occurs in `text` as whole words, ignoring case and punctuation: "spar"
/// matches "SPAR Alders" but not "sparen", "kpn mobiel" matches "KPN - Mobiel".
pub fn contains_words(text: &str, pattern: &str) -> bool {
    let (text, pattern) = (match_form(text), match_form(pattern));
    if pattern.is_empty() {
        return false;
    }
    text.match_indices(&pattern).any(|(i, _)| {
        let before = i == 0 || text.as_bytes()[i - 1] == b' ';
        let end = i + pattern.len();
        let after = end == text.len() || text.as_bytes()[end] == b' ';
        before && after
    })
}

/// Normalises a text rule: trimmed, lower case, single spaces, at least 3 characters.
pub fn normalize_pattern(input: &str) -> Option<String> {
    let s = input.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    (s.chars().count() >= 3).then_some(s)
}

/// A suggested text rule from a description: the counterparty as it appears on every
/// payment, without what differs per payment. Keeps the part before " – " (imported
/// descriptions put the counterparty first), before ">" (card payments: merchant, then
/// place and time) and before " via " / " by " (payment processors), drops a card
/// terminal prefix like "BCK*" or "WL*", stops at the first word with a digit (store and
/// terminal numbers) and drops a trailing legal form (B.V., N.V., SA, ...).
pub fn suggest_pattern(description: &str) -> String {
    const LEGAL: &[&str] = &["b.v.", "bv", "n.v.", "nv", "v.o.f.", "vof", "sa", "sarl", "ab", "ltd", "gmbh", "inc."];
    let mut head = description.split(" – ").next().unwrap_or(description);
    head = head.split('>').next().unwrap_or(head);
    let lower = head.to_lowercase();
    for sep in [" via ", " by "] {
        if let Some(i) = lower.find(sep) {
            head = &head[..i];
            break;
        }
    }
    // "BCK*AGRI GO FUEL" → "AGRI GO FUEL": a short terminal prefix before '*'.
    if let Some((prefix, rest)) = head.split_once('*') {
        if prefix.len() <= 4 {
            head = rest;
        }
    }
    let mut words: Vec<&str> =
        head.split_whitespace().take_while(|w| !w.chars().any(|c| c.is_ascii_digit())).collect();
    while words.len() > 1 && words.last().is_some_and(|w| LEGAL.contains(&w.to_lowercase().as_str())) {
        words.pop();
    }
    normalize_pattern(&words.join(" ")).unwrap_or_default()
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Transaction {
    pub id: String,
    pub date: String,
    pub description: String,
    pub amount_cents: i64,
    pub category_id: Option<String>,
    pub account_id: String,
    /// Bank reference for imported transactions, used to skip duplicates on re-import.
    #[serde(default)]
    pub import_ref: Option<String>,
    /// The other party's IBAN (imported transactions), used by the category IBAN rules.
    #[serde(default)]
    pub counterparty_iban: Option<String>,
    /// The import that created this transaction, see `Dataset.imports`.
    #[serde(default)]
    pub import_id: Option<String>,
    /// The day it counts on when that differs from the bank date (`YYYY-MM-DD`): a
    /// reversal in September for a failed August payment counts in August.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub counts_on: Option<String>,
    /// Parts of the amount that count in another category (salary with the holiday pay
    /// in it); the rest stays in `category_id`. Same sign as `amount_cents`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub splits: Vec<Split>,
}

/// A part of a transaction counted in its own category.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Split {
    pub amount_cents: i64,
    pub category_id: String,
}

impl Transaction {
    /// The day this transaction counts on in overviews, budgets and reports.
    pub fn day(&self) -> &str {
        self.counts_on.as_deref().unwrap_or(&self.date)
    }

    /// What stays in `category_id` after the splits.
    pub fn rest_cents(&self) -> i64 {
        self.amount_cents - self.splits.iter().map(|s| s.amount_cents).sum::<i64>()
    }
}

impl Dataset {
    /// The transactions as they count: a split transaction becomes one per part (the
    /// rest in its own category, each split in its category); others as they are.
    pub fn parts(&self) -> Vec<std::borrow::Cow<'_, Transaction>> {
        use std::borrow::Cow;
        let mut out = Vec::with_capacity(self.transactions.len());
        for t in &self.transactions {
            if t.splits.is_empty() {
                out.push(Cow::Borrowed(t));
                continue;
            }
            let part = |amount_cents: i64, category_id: Option<String>| {
                Cow::Owned(Transaction { amount_cents, category_id, splits: Vec::new(), ..t.clone() })
            };
            out.push(part(t.rest_cents(), t.category_id.clone()));
            for s in &t.splits {
                out.push(part(s.amount_cents, Some(s.category_id.clone())));
            }
        }
        out
    }
}

/// One entry in the import log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportRecord {
    pub id: String,
    /// RFC 3339, UTC.
    pub at: String,
    pub account_id: String,
    pub files: Vec<ImportFileStat>,
    pub imported: usize,
    pub skipped_duplicates: usize,
    pub classified: usize,
    /// Set when the import was undone; the record stays in the log.
    #[serde(default)]
    pub undone_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportFileStat {
    pub name: String,
    /// Booked entries in the file.
    pub entries: usize,
    pub imported: usize,
    pub skipped_duplicates: usize,
    /// Booking date range of the entries, `YYYY-MM-DD`.
    pub date_from: Option<String>,
    pub date_to: Option<String>,
}

/// A backup file in the app data dir.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupInfo {
    pub name: String,
    /// RFC 3339, UTC.
    pub created: String,
    /// As in the file name: dagelijks (daily), handmatig (manual), voor-import (before an
    /// import), voor-herstel (before a restore).
    pub reason: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dataset {
    pub accounts: Vec<Account>,
    pub categories: Vec<Category>,
    pub transactions: Vec<Transaction>,
    #[serde(default)]
    pub budgets: Vec<Budget>,
    /// Import log, newest first.
    #[serde(default)]
    pub imports: Vec<ImportRecord>,
    /// Which set of default rules has been added, so a default the user removes stays gone.
    #[serde(default)]
    pub default_rules_version: u32,
    /// The app's language; the standard category and group names follow it.
    #[serde(default)]
    pub language: Lang,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contracts: Vec<Contract>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub group_budgets: Vec<GroupBudget>,
}

/// A language the app speaks, stored as a locale tag ("nl-NL", "en").
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Lang {
    #[default]
    #[serde(rename = "nl-NL")]
    Nl,
    #[serde(rename = "en")]
    En,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::Nl, Lang::En];

    pub fn tag(self) -> &'static str {
        match self {
            Lang::Nl => "nl-NL",
            Lang::En => "en",
        }
    }

    /// `nl-NL`, `nl` or `en` (any case).
    pub fn from_tag(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "nl-nl" | "nl" => Some(Lang::Nl),
            "en" => Some(Lang::En),
            _ => None,
        }
    }

    /// The language's own name, for the picker.
    pub fn name(self) -> &'static str {
        match self {
            Lang::Nl => "Nederlands",
            Lang::En => "English",
        }
    }
}

/// Budgeted amount for one category in one month, in positive cents. Applies to
/// expense and income categories alike.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Budget {
    pub category_id: String,
    /// `YYYY-MM`
    pub month: String,
    pub amount_cents: i64,
}

/// Budget for all variable categories of one group ("Transport") in one month, in
/// positive cents: their total. Variable costs are only budgeted this way; income and
/// fixed costs per category.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GroupBudget {
    /// The group's name, as in `Category::group`.
    pub group: String,
    /// Variable (a group budget of another kind left in an older file counts nowhere).
    pub kind: CategoryKind,
    /// `YYYY-MM`
    pub month: String,
    pub amount_cents: i64,
}

/// Result of copying budgets from the previous year.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopyResult {
    pub copied: usize,
    pub data: Dataset,
}

/// Transaction input from the UI. `id` is `None` when creating.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionInput {
    pub id: Option<String>,
    pub date: String,
    pub description: String,
    pub amount_cents: i64,
    pub category_id: Option<String>,
    pub account_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportFile {
    pub name: String,
    pub xml: String,
}

/// Streamed from the backend during an import.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportProgress {
    /// 0-based index of the file being parsed; equals `file_count` while saving.
    pub file_index: usize,
    pub file_count: usize,
    pub file_name: String,
    /// Progress within the current file, 0.0–1.0.
    pub fraction: f64,
}

impl ImportProgress {
    pub fn overall(&self) -> f64 {
        if self.file_count == 0 {
            return 1.0;
        }
        ((self.file_index as f64 + self.fraction) / self.file_count as f64).min(1.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportResult {
    pub imported: usize,
    pub skipped_duplicates: usize,
    /// Imported transactions that got a category from an IBAN rule.
    #[serde(default)]
    pub classified: usize,
    /// Newest booking date among the imported transactions, so the UI can jump there.
    #[serde(default)]
    pub latest_date: Option<String>,
    pub data: Dataset,
}

/// Result of adding an IBAN rule: how many existing uncategorised transactions it filled in.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleResult {
    pub applied: usize,
    pub data: Dataset,
}

/// Normalises an IBAN (no spaces, upper case) and checks its shape: country code,
/// check digits, 11–30 alphanumerics. Does not verify the checksum.
pub fn normalize_iban(input: &str) -> Option<String> {
    let s: String = input.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_uppercase();
    let b = s.as_bytes();
    let ok = (15..=34).contains(&b.len())
        && b[..2].iter().all(u8::is_ascii_uppercase)
        && b[2..4].iter().all(u8::is_ascii_digit)
        && b[4..].iter().all(u8::is_ascii_alphanumeric);
    ok.then_some(s)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CategoryTotal {
    pub category_id: Option<String>,
    pub name: String,
    pub group: String,
    pub income_cents: i64,
    pub expense_cents: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonthlyOverview {
    /// `YYYY-MM`
    pub month: String,
    pub income_cents: i64,
    pub expense_cents: i64,
    /// Expenses in fixed-cost categories; the rest of `expense_cents` is variable or uncategorised.
    pub fixed_expense_cents: i64,
    /// Ordered by group, then by size.
    pub per_category: Vec<CategoryTotal>,
}

/// One category's month against its budget. `actual_cents` is positive: money spent
/// for expense categories, money received for income (refunds lower it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetLine {
    pub category_id: Option<String>,
    pub name: String,
    pub group: String,
    pub kind: CategoryKind,
    pub actual_cents: i64,
    pub budget_cents: Option<i64>,
    /// A group budget's line (see `budget_overview_grouped`): `name` is the group,
    /// `category_id` is `None`.
    #[serde(default)]
    pub group_line: bool,
    /// For a group line: the categories it holds, with what each spent. Empty for
    /// every other line.
    #[serde(default)]
    pub members: Vec<GroupMember>,
}

/// One month of a year forecast: actual totals, or the expected ones (see
/// `Dataset::forecast`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonthForecast {
    pub month: String,
    pub totals: KindTotals,
    /// True for months that already happened.
    pub actual: bool,
}

/// A category counted in a group budget.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupMember {
    pub category_id: Option<String>,
    pub name: String,
    pub actual_cents: i64,
}

/// Progress bar fractions (0.0–1.0): `fill` is the blue part, `over` the overshoot
/// segment drawn at the start when the budget is exceeded.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bar {
    pub fill: f64,
    pub over: f64,
}

impl BudgetLine {
    /// Budget minus actual: still to spend, pay or receive. Negative means over budget.
    pub fn open_cents(&self) -> Option<i64> {
        self.budget_cents.map(|b| b - self.actual_cents)
    }

    /// Over budget is only a problem for expenses; extra income is not.
    pub fn is_over(&self) -> bool {
        !self.kind.is_income() && self.open_cents().is_some_and(|o| o < 0)
    }

    pub fn bar(&self) -> Option<Bar> {
        let budget = self.budget_cents.filter(|b| *b > 0)? as f64;
        let actual = self.actual_cents.max(0) as f64;
        if !self.is_over() {
            return Some(Bar { fill: (actual / budget).min(1.0), over: 0.0 });
        }
        Some(Bar { fill: 1.0, over: ((actual - budget) / actual).max(0.04) })
    }
}

/// Per-kind sums in positive cents.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KindTotals {
    pub income: i64,
    pub fixed: i64,
    pub variable: i64,
    #[serde(default)]
    pub investment: i64,
}

impl KindTotals {
    pub fn saldo(&self) -> i64 {
        self.income - self.expenses()
    }

    pub fn expenses(&self) -> i64 {
        self.fixed + self.variable + self.investment
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetOverview {
    pub month: String,
    pub budget: KindTotals,
    pub actual: KindTotals,
    pub income: Vec<BudgetLine>,
    pub fixed: Vec<BudgetLine>,
    pub variable: Vec<BudgetLine>,
    #[serde(default)]
    pub investment: Vec<BudgetLine>,
}

impl BudgetOverview {
    /// Budget minus actual per kind; for the net result, actual minus expected (ahead of plan when positive).
    pub fn open(&self) -> (KindTotals, i64) {
        let open = KindTotals {
            income: self.budget.income - self.actual.income,
            fixed: self.budget.fixed - self.actual.fixed,
            variable: self.budget.variable - self.actual.variable,
            investment: self.budget.investment - self.actual.investment,
        };
        (open, self.actual.saldo() - self.budget.saldo())
    }
}

impl Dataset {
    /// Budget vs actual for one month. Every category with a budget or with transactions
    /// gets a line, in category-list order. Uncategorised money goes to a
    /// "No category" line: received under income, spent under variable. Income and
    /// fixed costs are budgeted per category, variable costs per group: the variable
    /// lines carry no budget and the variable budget is the sum of the group budgets.
    pub fn budget_overview(&self, month: &str) -> BudgetOverview {
        let mut lines: Vec<BudgetLine> = Vec::new();
        let parts = self.parts();
        let in_month: Vec<&Transaction> = parts.iter().map(|t| t.as_ref()).filter(|t| t.day().starts_with(month)).collect();
        for c in self.categories.iter().filter(|c| c.kind.counts()) {
            let net: i64 = in_month
                .iter()
                .filter(|t| t.category_id.as_deref() == Some(c.id.as_str()))
                .map(|t| t.amount_cents)
                .sum();
            let has_tx = in_month.iter().any(|t| t.category_id.as_deref() == Some(c.id.as_str()));
            let budget = self.own_budget(c, month);
            if !has_tx && budget.is_none() {
                continue;
            }
            let actual = if c.kind.is_income() { net } else { -net };
            lines.push(BudgetLine {
                category_id: Some(c.id.clone()),
                name: c.name.clone(),
                group: c.group.clone(),
                kind: c.kind,
                actual_cents: actual,
                budget_cents: budget,
                group_line: false,
                members: Vec::new(),
            });
        }
        let uncategorised = in_month.iter().filter(|t| self.category(t.category_id.as_deref()).is_none());
        let (mut received, mut spent) = (0i64, 0i64);
        for t in uncategorised {
            if t.amount_cents >= 0 {
                received += t.amount_cents;
            } else {
                spent -= t.amount_cents;
            }
        }
        for (kind, amount) in [(CategoryKind::Income, received), (CategoryKind::Variable, spent)] {
            if amount != 0 {
                lines.push(BudgetLine {
                    category_id: None,
                    name: NO_CATEGORY.into(),
                    group: String::new(),
                    kind,
                    actual_cents: amount,
                    budget_cents: None,
                    group_line: false,
                    members: Vec::new(),
                });
            }
        }

        let (mut budget, mut actual) = (KindTotals::default(), KindTotals::default());
        let (mut income, mut fixed, mut variable, mut investment) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for l in lines {
            // Variable lines carry no budget: theirs is the group's (added below).
            let b = l.budget_cents.unwrap_or(0);
            let (bt, at, list) = match l.kind {
                // Irregular income is income too (in totals and the net result), just not budgeted.
                CategoryKind::Income | CategoryKind::IrregularIncome => (&mut budget.income, &mut actual.income, &mut income),
                CategoryKind::Fixed => (&mut budget.fixed, &mut actual.fixed, &mut fixed),
                CategoryKind::Variable => (&mut budget.variable, &mut actual.variable, &mut variable),
                CategoryKind::Investment => (&mut budget.investment, &mut actual.investment, &mut investment),
                CategoryKind::Transfer => continue, // filtered out above
            };
            *bt += b;
            *at += l.actual_cents;
            list.push(l);
        }
        budget.variable += self.group_budgets_in(month).map(|g| g.amount_cents).sum::<i64>();
        BudgetOverview { month: month.to_string(), budget, actual, income, fixed, variable, investment }
    }

    /// Like `budget_overview`, but each variable group with a group budget that month
    /// gets one line instead of its categories' lines: the group's budget against what
    /// they spent together, with the categories in `members`. The line takes the place
    /// of the group's first category. Totals don't change.
    pub fn budget_overview_grouped(&self, month: &str) -> BudgetOverview {
        let mut o = self.budget_overview(month);
        for g in self.group_budgets_in(month) {
            let list = &mut o.variable;
            let inside = |l: &BudgetLine| {
                self.category(l.category_id.as_deref()).is_some_and(|c| !c.disabled && c.group == g.group && c.kind == g.kind)
            };
            let at = list.iter().position(inside).unwrap_or(list.len());
            let (members, kept): (Vec<BudgetLine>, Vec<BudgetLine>) = std::mem::take(list).into_iter().partition(inside);
            *list = kept;
            list.insert(
                at.min(list.len()),
                BudgetLine {
                    category_id: None,
                    name: g.group.clone(),
                    group: g.group.clone(),
                    kind: g.kind,
                    actual_cents: members.iter().map(|l| l.actual_cents).sum(),
                    budget_cents: Some(g.amount_cents),
                    group_line: true,
                    members: members
                        .into_iter()
                        .map(|l| GroupMember { category_id: l.category_id, name: l.name, actual_cents: l.actual_cents })
                        .collect(),
                },
            );
        }
        o
    }

    /// The group budget of a group's variable categories in `month`. Only variable costs
    /// have group budgets; one of another kind left in a file doesn't count.
    pub fn group_budget_for(&self, group: &str, kind: CategoryKind, month: &str) -> Option<i64> {
        if kind != CategoryKind::Variable {
            return None;
        }
        self.group_budgets
            .iter()
            .find(|b| b.group == group && b.kind == kind && b.month == month)
            .map(|b| b.amount_cents)
            .filter(|a| *a != 0)
    }

    /// The group budgets that count in `month`: those whose group still has switched-on
    /// variable categories.
    pub fn group_budgets_in<'a>(&'a self, month: &'a str) -> impl Iterator<Item = &'a GroupBudget> + 'a {
        self.group_budgets.iter().filter(move |g| g.month == month && self.group_budget_counts(g))
    }

    /// Whether a group budget counts: variable, nonzero, and its group still has
    /// switched-on variable categories.
    pub fn group_budget_counts(&self, g: &GroupBudget) -> bool {
        g.kind == CategoryKind::Variable
            && g.amount_cents != 0
            && self.categories.iter().any(|c| !c.disabled && c.group == g.group && c.kind == g.kind)
    }

    /// Whether a variable category counts through its group's budget in `month`.
    pub fn in_group_budget(&self, c: &Category, month: &str) -> bool {
        !c.disabled && self.group_budget_for(&c.group, c.kind, month).is_some()
    }

    /// A category's own budget in `month`: income, extra income, fixed costs and
    /// investments.
    /// Variable categories are budgeted per group, so a budget of theirs left in a file
    /// doesn't count, nor does one of a switched-off category.
    pub fn own_budget(&self, c: &Category, month: &str) -> Option<i64> {
        if c.disabled || c.kind == CategoryKind::Variable {
            return None;
        }
        self.budget_for(&c.id, month).filter(|b| *b != 0)
    }

    /// Variable spending per month at the current pace: the average of the six complete
    /// months before `now` (`YYYY-MM`, the month in progress), across years.
    pub fn variable_pace(&self, now: &str) -> i64 {
        let months = months_ending(&shift_month(now, -1), 6);
        let with_data: Vec<i64> = months
            .iter()
            .map(|m| self.budget_overview(m).actual.variable)
            .filter(|v| *v != 0)
            .collect();
        if with_data.is_empty() {
            0
        } else {
            with_data.iter().sum::<i64>() / with_data.len() as i64
        }
    }

    /// `forecast`, but with the months to come at the current pace of variable spending
    /// (`variable_pace`) instead of the budget: what happens if spending stays as it is.
    pub fn forecast_at_pace(&self, year: i32, current: &str, now: &str) -> Vec<MonthForecast> {
        let pace = self.variable_pace(now);
        let mut months = self.forecast(year, current);
        for m in months.iter_mut().filter(|m| !m.actual) {
            m.totals.variable = pace;
        }
        months
    }

    /// How the year ends, with `current` (`YYYY-MM`) the month in progress: complete
    /// months (before it) are actual; the current and later months get the budget: an
    /// income or fixed category's own, a variable group's once for its categories. A
    /// category budgeted in some months of the year (quarterly child benefit, school
    /// instalments; for a variable one, through its group) gets nothing in the others;
    /// only one without any budget that year continues at its average over the
    /// complete months. An unfinished month isn't real yet. Investments and extra
    /// income are in the year budget: they count as budgeted, never at an average.
    pub fn forecast(&self, year: i32, current: &str) -> Vec<MonthForecast> {
        let months: Vec<String> = (1..=12).map(|m| format!("{year:04}-{m:02}")).collect();
        let actual_months: Vec<&String> = months.iter().filter(|m| m.as_str() < current).collect();
        let n = actual_months.len().max(1) as i64;
        // Average actual per month so far, per month-budget category (and uncategorised spending).
        let mut avg: Vec<(Option<String>, CategoryKind, i64)> = Vec::new();
        for m in &actual_months {
            let o = self.budget_overview(m);
            for l in o.income.iter().chain(&o.fixed).chain(&o.variable) {
                let budgetable = match &l.category_id {
                    Some(id) => self.category(Some(id)).is_some_and(|c| c.budgetable() && c.kind.in_month_budget()),
                    None => l.kind == CategoryKind::Variable,
                };
                if !budgetable {
                    continue;
                }
                match avg.iter_mut().find(|(id, _, _)| *id == l.category_id) {
                    Some(x) => x.2 += l.actual_cents,
                    None => avg.push((l.category_id.clone(), l.kind, l.actual_cents)),
                }
            }
        }
        // Planned in some month of the year: an income or fixed category with a budget of
        // its own, a variable one through its group's budget.
        let planned_in_year = |c: &Category| {
            if c.kind == CategoryKind::Variable {
                months.iter().any(|m| self.in_group_budget(c, m))
            } else {
                months.iter().any(|m| self.own_budget(c, m).is_some())
            }
        };
        months
            .iter()
            .map(|m| {
                if m.as_str() < current {
                    let a = self.budget_overview(m).actual;
                    return MonthForecast { month: m.clone(), totals: a, actual: true };
                }
                let mut t = KindTotals::default();
                let mut add = |kind: CategoryKind, v: i64| match kind {
                    CategoryKind::Income | CategoryKind::IrregularIncome => t.income += v,
                    CategoryKind::Fixed => t.fixed += v,
                    CategoryKind::Variable => t.variable += v,
                    CategoryKind::Investment => t.investment += v,
                    CategoryKind::Transfer => {}
                };
                for g in self.group_budgets_in(m) {
                    add(g.kind, g.amount_cents);
                }
                // Variable categories in a group budget are covered by it.
                for c in self.categories.iter().filter(|c| c.budgetable() && !self.in_group_budget(c, m)) {
                    match self.own_budget(c, m) {
                        Some(b) => add(c.kind, b),
                        // Budgeted elsewhere in the year: this month is planned at nothing.
                        None if planned_in_year(c) => {}
                        None => {
                            if let Some((_, kind, sum)) = avg.iter().find(|(id, _, _)| id.as_deref() == Some(c.id.as_str())) {
                                add(*kind, sum / n);
                            }
                        }
                    }
                }
                if let Some((_, kind, sum)) = avg.iter().find(|(id, _, _)| id.is_none()) {
                    add(*kind, sum / n);
                }
                MonthForecast { month: m.clone(), totals: t, actual: false }
            })
            .collect()
    }

    /// The account's balance now: the latest bank balance (end of its day) plus the
    /// transactions after that day (bank dates), or the sum of all transactions without one.
    pub fn account_balance(&self, account_id: &str) -> i64 {
        let Some(acc) = self.accounts.iter().find(|a| a.id == account_id) else { return 0 };
        let from = acc.balances.last();
        from.map_or(0, |b| b.cents)
            + self
                .transactions
                .iter()
                .filter(|t| t.account_id == account_id)
                .filter(|t| from.is_none_or(|b| t.date > b.date))
                .map(|t| t.amount_cents)
                .sum::<i64>()
    }

    /// Each bank balance after the first against the previous one plus the transactions
    /// in between (bank dates), so a missing or doubled import shows as a difference.
    pub fn balance_checks(&self, account_id: &str) -> Vec<BalanceCheck> {
        let Some(acc) = self.accounts.iter().find(|a| a.id == account_id) else { return Vec::new() };
        acc.balances
            .windows(2)
            .map(|w| {
                let moved: i64 = self
                    .transactions
                    .iter()
                    .filter(|t| t.account_id == account_id && t.date > w[0].date && t.date <= w[1].date)
                    .map(|t| t.amount_cents)
                    .sum();
                BalanceCheck { date: w[1].date.clone(), bank_cents: w[1].cents, computed_cents: w[0].cents + moved }
            })
            .collect()
    }

    pub fn category(&self, id: Option<&str>) -> Option<&Category> {
        id.and_then(|id| self.categories.iter().find(|c| c.id == id))
    }

    /// The enabled category whose IBAN rules contain `iban`.
    pub fn category_for_iban(&self, iban: &str) -> Option<&Category> {
        self.categories.iter().find(|c| !c.disabled && c.ibans.iter().any(|i| i == iban))
    }

    /// The category the rules assign to a transaction: money to or from one of the
    /// user's own accounts is an internal transfer; then an IBAN rule; then the longest
    /// (most specific) plain text rule in the description; and only then the longest
    /// text rule for this direction (in for `amount_cents >= 0`, out otherwise).
    pub fn category_by_rules(&self, description: &str, iban: Option<&str>, amount_cents: i64) -> Option<&Category> {
        self.matching_rule(description, iban, amount_cents).and_then(|m| self.category(Some(&m.category_id)))
    }

    /// The rule that decides a transaction's category, in order: the counterparty is one
    /// of the own accounts (internal transfer); an IBAN rule; the longest matching text
    /// rule; the longest direction rule for money in or out.
    pub fn matching_rule(&self, description: &str, iban: Option<&str>, amount_cents: i64) -> Option<RuleMatch> {
        if let Some(i) = iban {
            if self.accounts.iter().any(|a| a.iban.as_deref() == Some(i)) {
                if let Some(c) = self.category(Some(catalog::ids::INTERNAL_TRANSFERS)).filter(|c| !c.disabled) {
                    return Some(RuleMatch { category_id: c.id.clone(), kind: None, value: i.to_string() });
                }
            }
            if let Some(c) = self.category_for_iban(i) {
                return Some(RuleMatch { category_id: c.id.clone(), kind: Some(RuleKind::Iban), value: i.to_string() });
            }
        }
        let desc = match_form(description);
        let longest = |kind: RuleKind| {
            self.categories
                .iter()
                .filter(|c| !c.disabled)
                .flat_map(|c| c.rules(kind).iter().filter(|p| contains_words(&desc, p)).map(move |p| (match_form(p).len(), c, p)))
                .max_by_key(|(len, _, _)| *len)
                .map(|(_, c, p)| RuleMatch { category_id: c.id.clone(), kind: Some(kind), value: p.clone() })
        };
        longest(RuleKind::Text).or_else(|| longest(if amount_cents >= 0 { RuleKind::TextIn } else { RuleKind::TextOut }))
    }

    pub fn budget_for(&self, category_id: &str, month: &str) -> Option<i64> {
        self.budgets
            .iter()
            .find(|b| b.category_id == category_id && b.month == month)
            .map(|b| b.amount_cents)
    }

    /// Totals for one month (`YYYY-MM`). Expenses are reported as negative cents.
    /// Transfers between own accounts are left out entirely.
    pub fn monthly_overview(&self, month: &str) -> MonthlyOverview {
        let mut per_category: Vec<CategoryTotal> = Vec::new();
        let (mut income, mut expense, mut fixed) = (0i64, 0i64, 0i64);
        let parts = self.parts();
        for t in parts.iter().filter(|t| t.day().starts_with(month)) {
            let cat = self.category(t.category_id.as_deref());
            if cat.is_some_and(|c| !c.kind.counts()) {
                continue;
            }
            let idx = match per_category.iter().position(|c| c.category_id == t.category_id) {
                Some(i) => i,
                None => {
                    per_category.push(CategoryTotal {
                        category_id: t.category_id.clone(),
                        name: cat.map(|c| c.name.clone()).unwrap_or_else(|| NO_CATEGORY.into()),
                        group: cat.map(|c| c.group.clone()).unwrap_or_default(),
                        income_cents: 0,
                        expense_cents: 0,
                    });
                    per_category.len() - 1
                }
            };
            if t.amount_cents >= 0 {
                income += t.amount_cents;
                per_category[idx].income_cents += t.amount_cents;
            } else {
                expense += t.amount_cents;
                per_category[idx].expense_cents += t.amount_cents;
                if cat.is_some_and(|c| c.kind == CategoryKind::Fixed) {
                    fixed += t.amount_cents;
                }
            }
        }
        // Groups in the order they first appear in the category list; uncategorised last.
        let group_rank = |g: &str| {
            if g.is_empty() {
                return usize::MAX;
            }
            self.categories.iter().position(|c| c.group == g).unwrap_or(usize::MAX - 1)
        };
        per_category.sort_by_key(|c| (group_rank(&c.group), c.expense_cents + c.income_cents));
        MonthlyOverview {
            month: month.to_string(),
            income_cents: income,
            expense_cents: expense,
            fixed_expense_cents: fixed,
            per_category,
        }
    }
}

/// Money in and out for one month, excluding excluded categories. Both positive cents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonthFlow {
    /// `YYYY-MM`
    pub month: String,
    pub income_cents: i64,
    pub expense_cents: i64,
}

impl MonthFlow {
    pub fn net_cents(&self) -> i64 {
        self.income_cents - self.expense_cents
    }
}

/// `YYYY-MM` shifted by `delta` months.
pub fn shift_month(month: &str, delta: i32) -> String {
    let y: i32 = month.get(..4).and_then(|s| s.parse().ok()).unwrap_or(1970);
    let m: i32 = month.get(5..7).and_then(|s| s.parse().ok()).unwrap_or(1);
    let idx = y * 12 + (m - 1) + delta;
    format!("{:04}-{:02}", idx.div_euclid(12), idx.rem_euclid(12) + 1)
}

/// The `n` months ending with `last`, oldest first.
pub fn months_ending(last: &str, n: usize) -> Vec<String> {
    (0..n as i32).rev().map(|i| shift_month(last, -i)).collect()
}

/// What a report shows per month.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReportSeries {
    ExpensesTotal,
    ExpensesFixed,
    ExpensesVariable,
    IncomeTotal,
    /// One category's actual (spent for expenses, received for income).
    Category(String),
    /// Income minus expenses.
    Saldo,
}

impl Dataset {
    /// One value per month for a report, from the same actuals as the overview
    /// (transfers excluded, refunds netted, uncategorised spending counts as variable).
    pub fn report_series(&self, months: &[String], series: &ReportSeries) -> Vec<i64> {
        months
            .iter()
            .map(|m| {
                let o = self.budget_overview(m);
                match series {
                    ReportSeries::ExpensesTotal => o.actual.expenses(),
                    ReportSeries::ExpensesFixed => o.actual.fixed,
                    ReportSeries::ExpensesVariable => o.actual.variable,
                    ReportSeries::IncomeTotal => o.actual.income,
                    ReportSeries::Saldo => o.actual.saldo(),
                    ReportSeries::Category(id) => o
                        .income
                        .iter()
                        .chain(&o.fixed)
                        .chain(&o.variable)
                        .chain(&o.investment)
                        .find(|l| l.category_id.as_deref() == Some(id.as_str()))
                        .map_or(0, |l| l.actual_cents),
                }
            })
            .collect()
    }

    /// Like report_series, but only transactions paid through `channel`. Sums the
    /// transactions directly (per sign for uncategorised ones), since the overview's
    /// per-category netting doesn't split by channel. Transfers are left out.
    pub fn report_series_channel(&self, months: &[String], series: &ReportSeries, channel: Channel) -> Vec<i64> {
        self.report_series_where(months, series, |t| t.channel() == Some(channel))
    }

    /// Like report_series, with an optional channel and without the transactions in
    /// `exclude` (category ids), e.g. a one-off renovation, to see normal spending.
    /// Uncategorised transactions count as "To categorise" (UNSORTED_CATEGORY_ID).
    pub fn report_series_filtered(
        &self,
        months: &[String],
        series: &ReportSeries,
        channel: Option<Channel>,
        exclude: &[String],
    ) -> Vec<i64> {
        if channel.is_none() && exclude.is_empty() {
            return self.report_series(months, series);
        }
        self.report_series_where(months, series, |t| {
            channel.is_none_or(|c| t.channel() == Some(c))
                && !exclude.iter().any(|x| x == t.category_id.as_deref().unwrap_or(UNSORTED_CATEGORY_ID))
        })
    }

    /// Sums the transactions `keep` accepts directly, per kind (per sign for
    /// uncategorised ones); refunds net within their kind like in the overview.
    fn report_series_where(&self, months: &[String], series: &ReportSeries, keep: impl Fn(&Transaction) -> bool) -> Vec<i64> {
        let parts = self.parts();
        months
            .iter()
            .map(|m| {
                let mut income = 0;
                let mut fixed = 0;
                let mut variable = 0;
                let mut investment = 0;
                let mut category = 0;
                for t in parts.iter().map(|t| t.as_ref()).filter(|t| t.day().starts_with(m.as_str()) && keep(t)) {
                    // Still to categorise: by sign, like uncategorised (not as its category's kind).
                    let kind = if t.is_unsorted() { None } else { self.category(t.category_id.as_deref()).map(|c| c.kind) };
                    match kind {
                        Some(CategoryKind::Transfer) => continue,
                        Some(CategoryKind::Income | CategoryKind::IrregularIncome) => income += t.amount_cents,
                        Some(CategoryKind::Fixed) => fixed -= t.amount_cents,
                        Some(CategoryKind::Variable) => variable -= t.amount_cents,
                        Some(CategoryKind::Investment) => investment -= t.amount_cents,
                        None if t.amount_cents >= 0 => income += t.amount_cents,
                        None => variable -= t.amount_cents,
                    }
                    if let ReportSeries::Category(id) = series {
                        if t.category_id.as_deref() == Some(id.as_str()) {
                            category += if kind.is_some_and(|k| k.is_income()) { t.amount_cents } else { -t.amount_cents };
                        }
                    }
                }
                match series {
                    ReportSeries::ExpensesTotal => fixed + variable + investment,
                    ReportSeries::ExpensesFixed => fixed,
                    ReportSeries::ExpensesVariable => variable,
                    ReportSeries::IncomeTotal => income,
                    ReportSeries::Saldo => income - fixed - variable - investment,
                    ReportSeries::Category(_) => category,
                }
            })
            .collect()
    }

    /// Average actual per month for every category in the month budget (switched on; not
    /// extra income, investments or transfers) over `months`: spent for expenses,
    /// received for income, refunds netted. Months without transactions count as zero.
    /// Rounded up to whole euros; categories averaging nothing are left out. Returns
    /// (category id, cents).
    pub fn average_per_month(&self, months: &[String]) -> Vec<(String, i64)> {
        if months.is_empty() {
            return Vec::new();
        }
        let parts = self.parts();
        self.categories
            .iter()
            .filter(|c| c.kind.in_month_budget() && !c.disabled)
            .filter_map(|c| {
                let net: i64 = parts
                    .iter()
                    .filter(|t| t.category_id.as_deref() == Some(c.id.as_str()))
                    .filter(|t| months.iter().any(|m| t.day().starts_with(m.as_str())))
                    .map(|t| t.amount_cents)
                    .sum();
                let total = if c.kind.is_income() { net } else { -net };
                let avg = total / months.len() as i64;
                (avg > 0).then(|| (c.id.clone(), (avg + 99) / 100 * 100))
            })
            .collect()
    }

    /// Income and expenses per month, in the given order.
    pub fn cash_flow(&self, months: &[String]) -> Vec<MonthFlow> {
        months
            .iter()
            .map(|m| {
                let o = self.monthly_overview(m);
                MonthFlow { month: m.clone(), income_cents: o.income_cents, expense_cents: -o.expense_cents }
            })
            .collect()
    }
}

/// The name of the line for money without a category (the app translates it).
pub const NO_CATEGORY: &str = "No category";

/// Budgets are whole euros: cents rounded to the nearest euro (half up), `912,49` →
/// `912`, `912,50` → `913`.
pub fn whole_euros(cents: i64) -> i64 {
    (cents + 50).div_euclid(100) * 100
}

/// Formats cents as `-1.234,56` (Dutch notation).
pub fn format_cents(cents: i64) -> String {
    format_cents_in(cents, Lang::Nl)
}

/// Formats cents in a language's notation: `-1.234,56` (nl-NL) or `-1,234.56` (en).
pub fn format_cents_in(cents: i64, lang: Lang) -> String {
    let (group, decimal) = match lang {
        Lang::Nl => ('.', ','),
        Lang::En => (',', '.'),
    };
    let sign = if cents < 0 { "-" } else { "" };
    let abs = cents.unsigned_abs();
    let euros = (abs / 100).to_string();
    let mut grouped = String::new();
    for (i, ch) in euros.chars().enumerate() {
        if i > 0 && (euros.len() - i) % 3 == 0 {
            grouped.push(group);
        }
        grouped.push(ch);
    }
    format!("{sign}{grouped}{decimal}{:02}", abs % 100)
}

/// Parses a user-typed amount like `12,34`, `-12.3`, `1.234,56` or `1234` into cents.
pub fn parse_amount(input: &str) -> Option<i64> {
    let s: String = input.trim().chars().filter(|c| !c.is_whitespace()).collect();
    let (neg, s) = match s.strip_prefix('-') {
        Some(rest) => (true, rest.to_string()),
        None => (false, s.strip_prefix('+').unwrap_or(&s).to_string()),
    };
    // The last ',' or '.' followed by 1–2 digits is the decimal separator; others are grouping.
    let dec_pos = s
        .rfind([',', '.'])
        .filter(|&p| (1..=2).contains(&(s.len() - p - 1)));
    let (int_part, frac_part) = match dec_pos {
        Some(p) => (&s[..p], &s[p + 1..]),
        None => (&s[..], ""),
    };
    let int_digits: String = int_part.chars().filter(|c| *c != '.' && *c != ',').collect();
    if int_digits.is_empty() && frac_part.is_empty() {
        return None;
    }
    if !int_digits.chars().all(|c| c.is_ascii_digit()) || !frac_part.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let euros: i64 = if int_digits.is_empty() { 0 } else { int_digits.parse().ok()? };
    let cents: i64 = match frac_part.len() {
        0 => 0,
        1 => frac_part.parse::<i64>().ok()? * 10,
        _ => frac_part.parse().ok()?,
    };
    let total = euros.checked_mul(100)?.checked_add(cents)?;
    Some(if neg { -total } else { total })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_format_amounts() {
        assert_eq!(parse_amount("12,34"), Some(1234));
        assert_eq!(parse_amount("-12.3"), Some(-1230));
        assert_eq!(parse_amount("1.234,56"), Some(123456));
        assert_eq!(parse_amount("1,234.56"), Some(123456));
        assert_eq!(parse_amount("1234"), Some(123400));
        assert_eq!(parse_amount("abc"), None);
        assert_eq!(parse_amount(""), None);
        assert_eq!(format_cents(-123456), "-1.234,56");
        assert_eq!(format_cents(5), "0,05");
        assert_eq!(format_cents_in(-123456, Lang::En), "-1,234.56");
        assert_eq!(format_cents_in(123_456_789, Lang::En), "1,234,567.89");
        assert_eq!(parse_amount(&format_cents_in(-123456, Lang::En)), Some(-123456));
        assert_eq!(parse_amount("1,250"), Some(125000), "English thousands");
    }

    #[test]
    fn monthly_overview_splits_income_and_expense() {
        let tx = |date: &str, amt, cat: Option<&str>| Transaction {
            id: date.into(),
            date: date.into(),
            description: String::new(),
            amount_cents: amt,
            category_id: cat.map(Into::into),
            account_id: "a".into(),
            ..Default::default()
        };
        let cat = |id: &str, kind| Category {
            id: id.into(),
            name: if id == "food" { "Boodschappen".into() } else { id.into() },
            group: "Huishouden".into(),
            kind,
            ..Default::default()
        };
        let ds = Dataset {
            accounts: vec![],
            categories: vec![cat("food", CategoryKind::Variable), cat("rent", CategoryKind::Fixed)],
            transactions: vec![
                tx("2026-09-01", 300000, None),
                tx("2026-09-02", -5000, Some("food")),
                tx("2026-09-03", -2500, Some("food")),
                tx("2026-09-04", -100000, Some("rent")),
                tx("2026-10-01", -9999, Some("food")),
            ],
            ..Default::default()
        };
        let o = ds.monthly_overview("2026-09");
        assert_eq!(o.income_cents, 300000);
        assert_eq!(o.expense_cents, -107500);
        assert_eq!(o.fixed_expense_cents, -100000);
        assert_eq!(o.per_category.last().unwrap().name, NO_CATEGORY);
        let food = o.per_category.iter().find(|c| c.category_id.as_deref() == Some("food")).unwrap();
        assert_eq!(food.expense_cents, -7500);
        assert_eq!(food.name, "Boodschappen");
    }

    fn budget_fixture() -> Dataset {
        let cat = |id: &str, kind, disabled| Category {
            id: id.into(),
            name: id.into(),
            group: "G".into(),
            kind,
            disabled,
            ..Default::default()
        };
        let tx = |amt, cat: Option<&str>| Transaction {
            id: format!("{amt}"),
            date: "2026-09-10".into(),
            description: String::new(),
            amount_cents: amt,
            category_id: cat.map(Into::into),
            account_id: "a".into(),
            ..Default::default()
        };
        let budget = |id: &str, cents| Budget { category_id: id.into(), month: "2026-09".into(), amount_cents: cents };
        Dataset {
            accounts: vec![],
            categories: vec![
                cat("salary", CategoryKind::Income, false),
                cat("rent", CategoryKind::Fixed, false),
                cat("internet", CategoryKind::Fixed, false),
                cat("food", CategoryKind::Variable, false),
                cat("eating", CategoryKind::Variable, false),
                cat("disabled", CategoryKind::Variable, true),
                cat("unused", CategoryKind::Variable, false),
                cat("transfer", CategoryKind::Transfer, false),
            ],
            transactions: vec![
                tx(-50000, Some("transfer")),
                tx(325000, Some("salary")),
                tx(-125000, Some("rent")),
                tx(-41230, Some("food")),
                tx(1000, Some("food")), // refund
                tx(-17900, Some("eating")),
                tx(-2000, None),
                tx(700, None),
            ],
            budgets: vec![
                budget("salary", 325000),
                budget("rent", 125000),
                budget("internet", 4500),
                budget("food", 45000),
                budget("eating", 12000),
                budget("disabled", 99999),
                budget("transfer", 88888),
            ],
            ..Default::default()
        }
    }

    #[test]
    fn budget_overview_totals_and_lines() {
        let o = budget_fixture().budget_overview("2026-09");
        // The variable categories' own budgets are leftovers: variable costs are
        // budgeted per group, and the fixture has no group budget.
        assert_eq!(o.budget, KindTotals { income: 325000, fixed: 129500, variable: 0, investment: 0 });
        assert!(o.variable.iter().all(|l| l.budget_cents.is_none()));
        // food: 41230 - 1000 refund; plus 2000 uncategorised spending.
        assert_eq!(o.actual, KindTotals { income: 325700, fixed: 125000, variable: 40230 + 17900 + 2000, investment: 0 });
        let (open, saldo_open) = o.open();
        assert_eq!(open.fixed, 4500, "internet still to pay");
        assert_eq!(saldo_open, o.actual.saldo() - o.budget.saldo());
        // Transfers count nowhere (their budget neither); disabled budgets are hidden.
        assert!(o.variable.iter().chain(&o.fixed).chain(&o.income).all(|l| !["transfer", "unused", "disabled"].contains(&l.name.as_str())));
        let flows = budget_fixture().cash_flow(&["2026-09".to_string()]);
        assert_eq!(flows[0].expense_cents, 125000 + 41230 + 17900 + 2000, "the 50000 transfer is not in cash flow");
        let internet = o.fixed.iter().find(|l| l.name == "internet").unwrap();
        assert_eq!((internet.actual_cents, internet.open_cents()), (0, Some(4500)));
        assert_eq!(o.income.last().unwrap().name, NO_CATEGORY);
        assert_eq!(o.variable.last().unwrap().actual_cents, 2000);
    }

    #[test]
    fn cash_flow_per_month() {
        assert_eq!(shift_month("2026-01", -1), "2025-12");
        assert_eq!(shift_month("2026-12", 1), "2027-01");
        assert_eq!(months_ending("2026-02", 3), vec!["2025-12", "2026-01", "2026-02"]);

        let mut ds = budget_fixture();
        ds.transactions.push(Transaction {
            id: "aug".into(),
            date: "2026-08-15".into(),
            amount_cents: -9000,
            account_id: "a".into(),
            ..Default::default()
        });
        let flows = ds.cash_flow(&months_ending("2026-09", 3));
        assert_eq!(flows.len(), 3);
        assert_eq!(flows[0], MonthFlow { month: "2026-07".into(), income_cents: 0, expense_cents: 0 });
        assert_eq!((flows[1].income_cents, flows[1].expense_cents, flows[1].net_cents()), (0, 9000, -9000));
        // September: salary + uncategorised 700 in; the excluded transfer is left out.
        assert_eq!(flows[2].income_cents, 325000 + 1000 + 700);
        assert_eq!(flows[2].expense_cents, 125000 + 41230 + 17900 + 2000);
    }

    #[test]
    fn report_series_per_month() {
        let ds = budget_fixture();
        let months = months_ending("2026-09", 2);
        assert_eq!(ds.report_series(&months, &ReportSeries::ExpensesTotal), vec![0, 125000 + 40230 + 17900 + 2000]);
        assert_eq!(ds.report_series(&months, &ReportSeries::ExpensesFixed), vec![0, 125000]);
        assert_eq!(ds.report_series(&months, &ReportSeries::IncomeTotal), vec![0, 325700]);
        assert_eq!(ds.report_series(&months, &ReportSeries::Category("food".into())), vec![0, 40230]);
        assert_eq!(ds.report_series(&months, &ReportSeries::Category("transfer".into())), vec![0, 0], "transfers count nowhere");
        let saldo = ds.report_series(&months, &ReportSeries::Saldo);
        assert_eq!(saldo[1], 325700 - (125000 + 40230 + 17900 + 2000));

        // Leaving out a category: gone from totals and the net result; nothing left out = the overview.
        let none: Vec<String> = vec![];
        let food = vec!["food".to_string()];
        let total = ReportSeries::ExpensesTotal;
        assert_eq!(ds.report_series_filtered(&months, &total, None, &none), ds.report_series(&months, &total));
        assert_eq!(ds.report_series_filtered(&months, &total, None, &food), vec![0, 125000 + 17900 + 2000]);
        assert_eq!(ds.report_series_filtered(&months, &ReportSeries::Saldo, None, &food)[1], saldo[1] + 40230);

        // An investment category: its own section, counted in expenses and the net result.
        let mut inv = ds.clone();
        inv.categories.iter_mut().find(|c| c.id == "food").unwrap().kind = CategoryKind::Investment;
        let o = inv.budget_overview("2026-09");
        assert_eq!(o.actual.investment, 40230);
        assert!(o.investment.iter().any(|l| l.name == "food") && o.variable.iter().all(|l| l.name != "food"));
        assert_eq!(inv.report_series(&months, &total)[1], ds.report_series(&months, &total)[1]);
        assert_eq!(o.actual.saldo(), saldo[1]);

        // A group budget: G's variable categories share 700; the budget total is the
        // group's 700 (their own leftover budgets of 450 and 120 don't count).
        let mut gb = ds.clone();
        gb.group_budgets.push(GroupBudget { group: "G".into(), kind: CategoryKind::Variable, month: "2026-09".into(), amount_cents: 70000 });
        let plain = gb.budget_overview("2026-09");
        assert_eq!(plain.budget.variable, 70000);
        assert_eq!(plain.budget.fixed, 129500);
        let grouped = gb.budget_overview_grouped("2026-09");
        let line = grouped.variable.iter().find(|l| l.group_line).unwrap();
        assert_eq!((line.name.as_str(), line.budget_cents, line.actual_cents), ("G", Some(70000), 40230 + 17900));
        assert!(line.members.iter().any(|m| m.name == "food" && m.actual_cents == 40230));
        assert!(grouped.variable.iter().all(|l| l.name != "food" && l.name != "eating"));
        assert_eq!(grouped.variable.last().unwrap().name, NO_CATEGORY, "uncategorised stays its own line");
        assert_eq!((grouped.actual, grouped.budget), (plain.actual, plain.budget));
        // A group budget of another kind (fixed, left in a file) counts nowhere.
        let mut fixed = gb.clone();
        fixed.group_budgets[0].kind = CategoryKind::Fixed;
        let o = fixed.budget_overview("2026-09");
        assert_eq!((o.budget.fixed, o.budget.variable), (129500, 0));
        assert!(fixed.budget_overview_grouped("2026-09").fixed.iter().all(|l| !l.group_line));
        // The forecast adds the group budget once instead of its categories. December,
        // without a group budget while G has one in other months, plans nothing for G.
        let mut gf = gb.clone();
        for m in ["2026-10", "2026-11"] {
            gf.group_budgets.push(GroupBudget { group: "G".into(), kind: CategoryKind::Variable, month: m.into(), amount_cents: 70000 });
        }
        let f = gf.forecast(2026, "2026-10");
        let uncategorised = 2000 / 9; // its average over January–September
        assert_eq!(f[9].totals.variable, 70000 + uncategorised);
        assert_eq!(f[11].totals.variable, uncategorised);

        // Forecast: September actual, October at budget. Salary is budgeted in September
        // and October, so it plans its budget; food and eating have no group budget this
        // year (their own budgets are leftovers), so they continue at their average, as
        // does uncategorised spending.
        let mut fc = ds.clone();
        fc.budgets.push(Budget { category_id: "salary".into(), month: "2026-10".into(), amount_cents: 300000 });
        let f = fc.forecast(2026, "2026-10");
        assert!(f[8].actual && !f[9].actual);
        assert_eq!(f[8].totals, fc.budget_overview("2026-09").actual);
        assert_eq!(f[9].totals.income, 300000);
        assert_eq!(f[9].totals.variable, 40230 / 9 + 17900 / 9 + 2000 / 9, "at their average");
        // Rent, budgeted in September only, plans nothing in October.
        assert_eq!(f[9].totals.fixed, 0);

        // At the current pace: the months to come spend what September (the only month
        // with spending in the six before October) spent, whatever the budget says.
        let sept = fc.budget_overview("2026-09").actual.variable;
        assert_eq!(fc.variable_pace("2026-10"), sept);
        let p = fc.forecast_at_pace(2026, "2026-10", "2026-10");
        assert_eq!(p[8].totals, f[8].totals, "actual months stay");
        assert_eq!((p[9].totals.variable, p[9].totals.income), (sept, 300000));
        assert_eq!(fc.variable_pace("2026-09"), 0, "no complete month with spending");

        // A split: part of the salary counts elsewhere; the total stays the same.
        let mut sp = ds.clone();
        let salary = sp.transactions.iter_mut().find(|t| t.category_id.as_deref() == Some("salary")).unwrap();
        salary.splits = vec![Split { amount_cents: 25700, category_id: "food".into() }];
        let rest = salary.rest_cents();
        let o = sp.budget_overview("2026-09");
        assert_eq!(o.income.iter().find(|l| l.name == "salary").unwrap().actual_cents, rest);
        assert_eq!(o.variable.iter().find(|l| l.name == "food").unwrap().actual_cents, 40230 - 25700, "an income part nets in food");
        assert_eq!(sp.parts().len(), sp.transactions.len() + 1);

        // A transaction moved to another day counts in that month, not its bank month.
        let mut moved = ds.clone();
        let t = moved.transactions.iter_mut().find(|t| t.category_id.as_deref() == Some("food")).unwrap();
        t.counts_on = Some("2026-08-31".into());
        let food_only = moved.report_series(&months, &ReportSeries::Category("food".into()));
        assert_eq!(food_only.iter().sum::<i64>(), 40230, "still counted once");
        assert!(food_only[0] > 0, "now (partly) in August");

        // Only one category left: its own actual, without uncategorised spending.
        let mut ds = ds;
        ds.transactions.push(Transaction { date: "2026-09-20".into(), amount_cents: -999, ..Default::default() });
        let all_but_food: Vec<String> =
            ds.categories.iter().map(|c| c.id.clone()).filter(|id| id != "food").chain([UNSORTED_CATEGORY_ID.to_string()]).collect();
        assert_eq!(ds.report_series_filtered(&months, &total, None, &all_but_food), vec![0, 40230]);
    }

    #[test]
    fn channels() {
        let t = |d: &str| Transaction { description: d.into(), amount_cents: -1000, ..Default::default() };
        assert_eq!(t("ALBERT HEIJN 1620 >UTRECHT 25.09.2026").channel(), Some(Channel::InStore));
        assert_eq!(t("BEA, Betaalpas Jumbo Zeist,PAS123").channel(), Some(Channel::InStore));
        assert_eq!(t("CCV*TWEEK NL").channel(), Some(Channel::InStore));
        assert_eq!(t("bol.com – P1234").channel(), Some(Channel::Online));
        assert_eq!(t("Hollandbikeshop.com via Multisafepay – 1").channel(), Some(Channel::Online));
        assert_eq!(t("Amazon Prime Video UK iDeal – D01").channel(), Some(Channel::Online));
        assert_eq!(t("Google Play Apps >Dublin 1.09.2026").channel(), Some(Channel::Online), "online sign beats the card marker");
        // Bank card shapes: a phone at the till is in store, card data without a till type is online.
        assert_eq!(t("SPAR Centrum >UTRECHT 7.09.2026 19U23 KV003 379RNL MCC:5411 Google Pay betaling NLNEDERLAND").channel(), Some(Channel::InStore));
        assert_eq!(t("Jumbo >UTRECHT 12.09.2026 KV002 MCC:5411 Contactloze betaling NLNEDERLAND").channel(), Some(Channel::InStore));
        assert_eq!(t("WL*GOOGLE >Dublin EUR 9,99 KV002 MCC:5818 IEIERLAND").channel(), Some(Channel::Online));
        assert_eq!(t("WL*GOOGLE >Dublin EUR 9,99 KV002 IEIERLAND 12:34:56").channel(), Some(Channel::Online), "no MCC");
        assert_eq!(t("Stichting Hypotheken Incasso – LENINGREK").channel(), None);

        let mut ds = budget_fixture();
        ds.transactions = vec![
            Transaction { date: "2026-09-02".into(), description: "Jumbo >ZEIST".into(), amount_cents: -4000, category_id: Some("food".into()), ..Default::default() },
            Transaction { date: "2026-09-03".into(), description: "Picnic via Adyen".into(), amount_cents: -6000, category_id: Some("food".into()), ..Default::default() },
            Transaction { date: "2026-09-04".into(), description: "Huur".into(), amount_cents: -100000, category_id: Some("rent".into()), ..Default::default() },
        ];
        let m = vec!["2026-09".to_string()];
        assert_eq!(ds.report_series_channel(&m, &ReportSeries::ExpensesTotal, Channel::Online), vec![6000]);
        assert_eq!(ds.report_series_channel(&m, &ReportSeries::ExpensesTotal, Channel::InStore), vec![4000]);
        assert_eq!(ds.report_series_channel(&m, &ReportSeries::Category("food".into()), Channel::InStore), vec![4000]);
        assert_eq!(ds.report_series(&m, &ReportSeries::ExpensesTotal), vec![110000], "without a channel: everything");
    }

    #[test]
    fn average_per_month_for_budgets() {
        let mut ds = budget_fixture();
        // A second month of food, and nothing in the third.
        ds.transactions.push(Transaction {
            id: "aug-food".into(),
            date: "2026-08-10".into(),
            amount_cents: -30001,
            category_id: Some("food".into()),
            account_id: "a".into(),
            ..Default::default()
        });
        let avg = ds.average_per_month(&months_ending("2026-09", 3));
        let get = |id: &str| avg.iter().find(|(c, _)| c == id).map(|(_, v)| *v);
        // food: (40230 + 30001) / 3 = 23410.33 → € 235 rounded up.
        assert_eq!(get("food"), Some(23500));
        assert_eq!(get("salary"), Some(108400), "325000 / 3 = 108333.3 → € 1.084");
        assert_eq!(get("transfer"), None, "transfers get no budget");
        assert_eq!(get("unused"), None, "nothing spent, nothing proposed");
        assert!(ds.average_per_month(&[]).is_empty());
    }

    /// Extra income and investments are in the year budget: the forecast plans them as
    /// budgeted, and they never get an average (that fills the month budget).
    #[test]
    fn year_budget_kinds() {
        assert!(CategoryKind::Income.in_month_budget() && !CategoryKind::IrregularIncome.in_month_budget());
        assert!(CategoryKind::Investment.budgetable() && !CategoryKind::Investment.in_month_budget());
        assert!(!CategoryKind::Transfer.budgetable());

        let mut ds = budget_fixture();
        let cat = |id: &str, kind| Category { id: id.into(), name: id.into(), group: "G".into(), kind, ..Default::default() };
        ds.categories.push(cat("holiday_pay", CategoryKind::IrregularIncome));
        ds.categories.push(cat("insulation", CategoryKind::Investment));
        let tx = |id: &str, cents, c: &str| Transaction {
            id: id.into(),
            date: "2026-09-12".into(),
            amount_cents: cents,
            category_id: Some(c.into()),
            account_id: "a".into(),
            ..Default::default()
        };
        ds.transactions.push(tx("hp", 250000, "holiday_pay"));
        ds.transactions.push(tx("ins", -400000, "insulation"));
        let avg = ds.average_per_month(&months_ending("2026-09", 3));
        assert!(avg.iter().all(|(id, _)| id != "holiday_pay" && id != "insulation"));

        ds.budgets.push(Budget { category_id: "holiday_pay".into(), month: "2026-11".into(), amount_cents: 260000 });
        ds.budgets.push(Budget { category_id: "insulation".into(), month: "2026-12".into(), amount_cents: 500000 });
        let f = ds.forecast(2026, "2026-10");
        assert_eq!((f[9].totals.income, f[9].totals.investment), (0, 0), "no budget in October, no average");
        assert_eq!(f[10].totals.income, 260000);
        assert_eq!(f[11].totals.investment, 500000);
    }

    #[test]
    fn iban_rules_and_disabled_categories() {
        assert_eq!(normalize_iban("nl91 abna 0417 1643 00").as_deref(), Some("NL91ABNA0417164300"));
        assert_eq!(normalize_iban("NL91"), None);
        assert_eq!(normalize_iban("9191ABNA0417164300"), None);

        let mut ds = budget_fixture();
        ds.categories[3].ibans = vec!["NL91ABNA0417164300".into()]; // food
        assert_eq!(ds.category_for_iban("NL91ABNA0417164300").map(|c| c.id.as_str()), Some("food"));
        ds.categories[3].disabled = true;
        assert!(ds.category_for_iban("NL91ABNA0417164300").is_none(), "disabled categories don't classify");
    }

    #[test]
    fn text_rules() {
        assert_eq!(normalize_pattern("  Albert   HEIJN "), Some("albert heijn".into()));
        assert_eq!(normalize_pattern("ah"), None);
        assert_eq!(suggest_pattern("ALBERT HEIJN 1234 UTRECHT NLD"), "albert heijn");
        assert_eq!(suggest_pattern("Albert Heijn 1234"), "albert heijn");
        assert_eq!(suggest_pattern("Bol.com – Bestelling 5566"), "bol.com");
        assert_eq!(suggest_pattern("12345"), "");

        let mut ds = budget_fixture();
        ds.categories[3].patterns = vec!["albert heijn".into()]; // food
        ds.categories[4].patterns = vec!["albert heijn to go".into()]; // eating
        ds.categories[0].ibans = vec!["NL20INGB0001234567".into()]; // salary
        let id = |c: Option<&Category>| c.map(|c| c.id.clone());
        assert_eq!(id(ds.category_by_rules("ALBERT HEIJN 1234 Utrecht", None, -100)).as_deref(), Some("food"));
        assert_eq!(id(ds.category_by_rules("Albert Heijn To Go 88", None, -100)).as_deref(), Some("eating"), "longest wins");
        assert_eq!(
            id(ds.category_by_rules("Albert Heijn", Some("NL20INGB0001234567"), -100)).as_deref(),
            Some("salary"),
            "IBAN rule first"
        );
        assert_eq!(id(ds.category_by_rules("Jumbo", None, -100)), None);

        // Direction-aware rules: same counterparty, category by direction, and only when
        // no plain text rule matches (so a specific word still wins).
        ds.categories[0].patterns_in = vec!["acme".into()]; // salary, money in
        ds.categories[3].patterns_out = vec!["acme".into()]; // food, money out (lunch)
        ds.categories[4].patterns = vec!["lunch extra".into()];
        assert_eq!(id(ds.category_by_rules("Acme B.V. – salaris", None, 300000)).as_deref(), Some("salary"));
        assert_eq!(id(ds.category_by_rules("Acme B.V. – lunch", None, -7375)).as_deref(), Some("food"));
        assert_eq!(id(ds.category_by_rules("Acme – lunch extra", None, -500)).as_deref(), Some("eating"));
        let t = |amount_cents| Transaction { description: "ACME B.V.".into(), amount_cents, ..Default::default() };
        assert!(rule_matches(RuleKind::TextIn, "acme", &t(1)) && !rule_matches(RuleKind::TextIn, "acme", &t(-1)));
        assert!(rule_matches(RuleKind::TextOut, "acme", &t(-1)) && !rule_matches(RuleKind::TextOut, "acme", &t(1)));
        assert!(contains_words("plus zeist", "plus") && !contains_words("surplus bv", "plus"));
        assert!(!contains_words("sparen", "spar") && contains_words("spar city", "spar"));
        assert!(contains_words("bol.com – bestelling", "bol.com"));
        assert!(contains_words("KPN - Mobiel – Factuur", "kpn mobiel"), "punctuation counts as a space");
        assert!(contains_words("NATIONALE-NEDERLANDEN", "nationale nederlanden"));
        assert!(contains_words("ETOS 7485 >UTRECHT 16.09.2026", "etos"));

        // Suggestions from bank description shapes (card payments, processors).
        assert_eq!(suggest_pattern("ALBERT HEIJN 1620 >UTRECHT 25.09.2026 16U26 KV006"), "albert heijn");
        assert_eq!(suggest_pattern("SPAR City Utrecht  >UTRECHT 7.09.2026"), "spar city utrecht");
        assert_eq!(suggest_pattern("Hollandbikeshop.com via Multisafepay – 123"), "hollandbikeshop.com");
        assert_eq!(suggest_pattern("Nederlandse Loterij by Buckaroo – 9988"), "nederlandse loterij");
        assert_eq!(suggest_pattern("BCK*TANK EXPRESS B.V. >ZWOLLE"), "tank express");
        assert_eq!(suggest_pattern("WL*GOOGLE >DUBLIN EUR"), "google");
        assert_eq!(suggest_pattern("Vinted via Mangopay SA – x"), "vinted");
        assert_eq!(suggest_pattern("ANWB B.V. – 1234"), "anwb");

        // Money between own accounts is an internal transfer, before any rule.
        let mut own = budget_fixture();
        own.accounts.push(Account { id: "s".into(), name: "Spaar".into(), iban: Some("NL02RABO0123456789".into()), balances: Vec::new() });
        own.categories.push(Category { id: catalog::ids::INTERNAL_TRANSFERS.into(), kind: CategoryKind::Transfer, ..Default::default() });
        own.categories[3].ibans = vec!["NL02RABO0123456789".into()];
        assert_eq!(
            own.category_by_rules("Spaarrekening", Some("NL02RABO0123456789"), -100).map(|c| c.id.as_str()),
            Some(catalog::ids::INTERNAL_TRANSFERS)
        );
        ds.categories[3].disabled = true;
        assert_eq!(id(ds.category_by_rules("Albert Heijn 1234", None, -100)), None);
    }

    #[test]
    fn budget_bars() {
        let o = budget_fixture().budget_overview("2026-09");
        // The bars work the same for a group line; here on category lines with a budget.
        let line = |n: &str, budget| BudgetLine {
            budget_cents: Some(budget),
            ..o.variable.iter().chain(&o.fixed).find(|l| l.name == n).unwrap().clone()
        };

        let food = line("food", 45000);
        assert!(!food.is_over());
        let bar = food.bar().unwrap();
        assert!((bar.fill - 40230.0 / 45000.0).abs() < 1e-9 && bar.over == 0.0);

        let eating = line("eating", 12000); // 17900 of 12000
        assert!(eating.is_over());
        assert_eq!(eating.open_cents(), Some(-5900));
        let bar = eating.bar().unwrap();
        assert_eq!(bar.fill, 1.0);
        assert!((bar.over - 5900.0 / 17900.0).abs() < 1e-9);

        // Barely over still shows a visible segment; no budget means no bar.
        let mut tiny = eating.clone();
        tiny.actual_cents = 12001;
        assert_eq!(tiny.bar().unwrap().over, 0.04);
        tiny.budget_cents = None;
        assert!(tiny.bar().is_none());

        // Extra income is never "over".
        let mut salary = o.income[0].clone();
        salary.actual_cents = 400000;
        assert!(!salary.is_over());
        assert_eq!(salary.bar().unwrap(), Bar { fill: 1.0, over: 0.0 });
    }
}
