use std::future::Future;

use fin_shared::{
    months_ending, normalize_iban, normalize_pattern, normalize_rule, parse_amount, rule_matches, shift_month,
    suggest_pattern, Account, BudgetLine, BudgetOverview, Category, CategoryKind, Channel, Dataset, ImportFile,
    ImportProgress, Lang, ReportSeries, RuleInfo, RuleKind, Transaction, TransactionInput, UNSORTED_CATEGORY_ID,
};
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

mod contracts;

use crate::combo::{ComboBox, ComboOption};
use crate::{api, i18n};

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Overview,
    Reports,
    Transactions,
    Budget,
    Accounts,
    Contracts,
    Categories,
    Data,
    Settings,
}

#[derive(Clone, Copy)]
struct Ctx {
    data: RwSignal<Dataset>,
    error: RwSignal<Option<String>>,
    /// `YYYY-MM`, shared by the overview and the transaction list.
    month: RwSignal<String>,
    page: RwSignal<Page>,
    /// Shown after a rule is added, when this year has transactions it would change.
    rule_offer: RwSignal<Option<RuleOffer>>,
}

#[derive(Clone)]
struct RuleOffer {
    category_id: String,
    category_name: String,
    kind: RuleKind,
    /// Normalised rule value (IBAN or lower-case text).
    value: String,
    /// `YYYY-01-01` of the current year.
    from: String,
    count: usize,
}

/// After adding a rule: offer to also recategorise this year's matching transactions
/// that sit in another category.
fn offer_rule_for_year(ctx: Ctx, category_id: &str, kind: RuleKind, value: &str) {
    let Some(value) = normalize_rule(kind, value) else { return };
    let from = format!("{}-01-01", &today()[..4]);
    let (count, category_name) = ctx.data.with_untracked(|d| {
        let count = d
            .transactions
            .iter()
            .filter(|t| {
                rule_matches(kind, &value, t)
                    && t.date.as_str() >= from.as_str()
                    && t.category_id.as_deref() != Some(category_id)
            })
            .count();
        (count, d.category(Some(category_id)).map(|c| c.name.clone()).unwrap_or_default())
    });
    ctx.rule_offer.set((count > 0).then(|| RuleOffer {
        category_id: category_id.to_string(),
        category_name,
        kind,
        value,
        from,
        count,
    }));
}

/// How a rule reads in a sentence: `with IBAN NL44…` or `with "albert heijn" in the description`.
fn rule_phrase(kind: RuleKind, value: &str) -> String {
    match kind {
        RuleKind::Iban => t!("with IBAN {}", value),
        RuleKind::Text => t!("with “{}” in the description", value),
        RuleKind::TextIn => t!("from “{}” (money in)", value),
        RuleKind::TextOut => t!("to “{}” (money out)", value),
    }
}

/// How a rule kind is labelled in a category's rule list.
fn rule_label(kind: RuleKind) -> &'static str {
    match kind {
        RuleKind::Iban => "IBAN",
        RuleKind::Text => t!("Description contains"),
        RuleKind::TextIn => t!("Contains, money in only"),
        RuleKind::TextOut => t!("Contains, money out only"),
    }
}

#[component]
fn RuleOfferBar() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    move || {
        ctx.rule_offer.get().map(|o| {
            let year = o.from[..4].to_string();
            let apply = {
                let o = o.clone();
                move |_| {
                    let o = o.clone();
                    ctx.rule_offer.set(None);
                    spawn_local(async move {
                        match api::apply_category_rule(&o.category_id, o.kind, &o.value, &o.from).await {
                            Ok(r) => {
                                ctx.data.set(r.data);
                                ctx.error.set(None);
                            }
                            Err(e) => ctx.error.set(Some(e)),
                        }
                    });
                }
            };
            view! {
                <div class="offer" role="status">
                    <span>
                        {tn!(
                            o.count,
                            "{} transaction from {} {} has another category. Categorise it as ",
                            "{} transactions from {} {} have another category. Categorise them all as ",
                            o.count,
                            year,
                            rule_phrase(o.kind, &o.value)
                        )}
                        <strong>{o.category_name.clone()}</strong>"?"
                    </span>
                    <button class="small primary" on:click=apply>{t!("Yes, all from {}", year)}</button>
                    <button class="small" on:click=move |_| ctx.rule_offer.set(None)>{t!("No, only new ones")}</button>
                </div>
            }
        })
    }
}

/// Runs a backend mutation and replaces the dataset with its result.
fn apply(ctx: Ctx, fut: impl Future<Output = Result<Dataset, String>> + 'static) {
    spawn_local(async move {
        match fut.await {
            Ok(d) => {
                ctx.data.set(d);
                ctx.error.set(None);
            }
            Err(e) => ctx.error.set(Some(e)),
        }
    });
}

fn today() -> String {
    let d = js_sys::Date::new_0();
    format!("{:04}-{:02}-{:02}", d.get_full_year(), d.get_month() + 1, d.get_date())
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// `September 2026`.
pub(crate) fn month_label(month: &str) -> String {
    let m: usize = month[5..7].parse().unwrap_or(1);
    format!("{} {}", capitalize(i18n::month_name(m.max(1) - 1)), &month[..4])
}

/// Short month name of a `YYYY-MM`.
fn month_short_of(month: &str) -> &'static str {
    i18n::month_short(month[5..7].parse::<usize>().unwrap_or(1).clamp(1, 12) - 1)
}

fn euro(cents: i64) -> String {
    i18n::euro(cents)
}

/// Budget amounts without the cents when they are whole: `1.250`, `12,50`.
fn budget_text(cents: i64) -> String {
    i18n::whole(cents)
}

/// A budget amount with the € sign, without the cents when they are whole: `€ 1.350`,
/// `€ 912,09`.
fn euro_text(cents: i64) -> String {
    format!("€ {}", budget_text(cents))
}


/// A line's name; the line for money without a category in the app's language.
fn line_name(category_id: Option<&str>, name: &str) -> String {
    match category_id {
        None => t!("No category").to_string(),
        Some(_) => name.to_string(),
    }
}

/// What a budget line stands for across months: its category, or for a group line its
/// group and kind.
fn line_key(l: &BudgetLine) -> Option<String> {
    if l.group_line {
        Some(format!("group:{}:{}", l.kind.key(), l.name))
    } else {
        l.category_id.clone()
    }
}

fn kind_label(k: CategoryKind) -> &'static str {
    match k {
        CategoryKind::Income => t!("Regular income"),
        CategoryKind::IrregularIncome => t!("Extra income"),
        CategoryKind::Fixed => t!("Fixed costs"),
        CategoryKind::Variable => t!("Variable"),
        CategoryKind::Investment => t!("Investment"),
        CategoryKind::Transfer => t!("Transfer"),
    }
}

fn channel_label(c: Channel) -> &'static str {
    match c {
        Channel::Online => t!("Online"),
        Channel::InStore => t!("In store"),
    }
}

#[component]
pub fn App() -> impl IntoView {
    let ctx = Ctx {
        data: RwSignal::new(Dataset::default()),
        error: RwSignal::new(None),
        month: RwSignal::new(today()[..7].to_string()),
        page: RwSignal::new(Page::Overview),
        rule_offer: RwSignal::new(None),
    };
    provide_context(ctx);
    apply(ctx, api::get_data());
    // Pick up changes made with fin-cli while the window was in the background.
    let _ = window_event_listener(leptos::ev::focus, move |_| apply(ctx, api::get_data()));

    // The language is in the data file. Everything renders again when it changes, with
    // the new language set first (page, month and sidebar live outside and stay).
    let lang = Memo::new(move |_| ctx.data.with(|d| d.language));
    // Icons only; remembered per window (a convenience, fine to lose).
    let collapsed = RwSignal::new(stored_flag(SIDEBAR_KEY));
    move || {
        let l = lang.get();
        i18n::set_lang(l);
        if let Some(root) = document().document_element() {
            let _ = root.set_attribute("lang", l.tag());
        }
        view! { <Shell collapsed=collapsed/> }
    }
}

#[component]
fn Shell(collapsed: RwSignal<bool>) -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let page = ctx.page;
    let nav = move |p: Page, label: &'static str| {
        // Transactions shows how many still need a category.
        let badge = move || {
            (p == Page::Transactions)
                .then(|| ctx.data.with(|d| d.transactions.iter().filter(|t| is_unsorted(t)).count()))
                .filter(|n| *n > 0)
                .map(|n| view! { <span class="count" title=t!("To categorise")>{n}</span> })
        };
        view! {
            <button class="nav-item" class:active=move || page.get() == p title=label aria-label=label on:click=move |_| page.set(p)>
                <NavIcon page=p/>
                <span class="label">{label}</span>
                {badge}
            </button>
        }
    };
    let (expand, fold) = (t!("Expand menu"), t!("Collapse menu"));

    view! {
        <div class="shell" class:collapsed=move || collapsed.get()>
            <nav class="sidebar">
                <div class="brand"><Logo/><span class="label">"Fin"</span></div>
                {nav(Page::Overview, t!("Overview"))}
                {nav(Page::Reports, t!("Reports"))}
                {nav(Page::Transactions, t!("Transactions"))}
                {nav(Page::Budget, t!("Budget"))}
                {nav(Page::Accounts, t!("Accounts"))}
                {nav(Page::Contracts, t!("Contracts"))}
                {nav(Page::Categories, t!("Categories"))}
                {nav(Page::Data, t!("Import & backup"))}
                {nav(Page::Settings, t!("Settings"))}
                <button
                    class="nav-item collapse"
                    aria-label=move || if collapsed.get() { expand } else { fold }
                    title=move || if collapsed.get() { expand } else { fold }
                    aria-expanded=move || (!collapsed.get()).to_string()
                    on:click=move |_| {
                        let now = !collapsed.get_untracked();
                        collapsed.set(now);
                        store_flag(SIDEBAR_KEY, now);
                    }
                >
                    <svg width="18" height="18" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                        <rect x="1.8" y="2.5" width="12.4" height="11" rx="2"></rect>
                        <path d="M6 2.5v11"></path>
                    </svg>
                    <span class="label">{t!("Collapse")}</span>
                </button>
            </nav>
            <main class="content">
                {move || ctx.error.get().map(|e| view! {
                    <div class="error" role="alert">
                        // Backend messages are English; the app's own are translated already.
                        <span>{i18n::error(&e)}</span>
                        <button class="link" on:click=move |_| ctx.error.set(None)>{t!("Close")}</button>
                    </div>
                })}
                <RuleOfferBar/>
                {move || match page.get() {
                    Page::Overview => view! { <Overview/> }.into_any(),
                    Page::Reports => view! { <ReportsPage/> }.into_any(),
                    Page::Transactions => view! { <Transactions/> }.into_any(),
                    Page::Budget => view! { <BudgetPage/> }.into_any(),
                    Page::Accounts => view! { <Accounts/> }.into_any(),
                    Page::Contracts => view! { <contracts::ContractsPage/> }.into_any(),
                    Page::Categories => view! { <Categories/> }.into_any(),
                    Page::Data => view! { <DataPage/> }.into_any(),
                    Page::Settings => view! { <SettingsPage/> }.into_any(),
                }}
            </main>
        </div>
    }
}

const SIDEBAR_KEY: &str = "fin.sidebar-collapsed";
/// Setting: leave investment categories out of Overview and Reports by default.
const EXCLUDE_INVESTMENTS_KEY: &str = "fin.exclude-investments";

/// A per-window preference from localStorage; false when storage is unavailable.
fn stored_flag(key: &str) -> bool {
    window().local_storage().ok().flatten().and_then(|s| s.get_item(key).ok().flatten()).is_some_and(|v| v == "1")
}

fn store_flag(key: &str, on: bool) {
    if let Some(s) = window().local_storage().ok().flatten() {
        let _ = s.set_item(key, if on { "1" } else { "0" });
    }
}

/// Line icons for the menu, 16-unit box, drawn in the text color.
#[component]
fn NavIcon(page: Page) -> impl IntoView {
    let d = match page {
        Page::Overview => "M2.5 2.5h4.5v4.5h-4.5zM9 2.5h4.5v4.5H9zM2.5 9h4.5v4.5h-4.5zM9 9h4.5v4.5H9z",
        Page::Reports => "M1.5 14h13M3.5 14V8.5M8 14V3M12.5 14V6",
        Page::Transactions => "M2.5 5.5h10.5l-2.5-2.5M13.5 10.5H3l2.5 2.5",
        Page::Budget => "M8 1.8V8h6.2M13.6 10.6A6.2 6.2 0 1 1 5.4 2.4",
        Page::Accounts => "M1.5 6 8 2.2 14.5 6zM3.2 7.5v5M6.4 7.5v5M9.6 7.5v5M12.8 7.5v5M1.5 14h13",
        Page::Contracts => "M3.5 1.5h6L12.5 4.5v10h-9zM9.5 1.5v3h3M5.5 8h5M5.5 10.5h5",
        Page::Categories => "M2 2h5.6L14 8.4 8.4 14 2 7.6zM5.2 5.2h.01",
        Page::Data => "M8 2v7.5M5 6.5l3 3 3-3M2.5 10.5v3h11v-3",
        Page::Settings => "M2 4.5h6.5M11.5 4.5H14M2 11.5h2.5M7.5 11.5H14M10 3v3M6 10v3",
    };
    view! {
        <svg class="nav-icon" width="18" height="18" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d=d></path>
        </svg>
    }
}

/// The fin mark from src-tauri/icons/icon.svg, in a 100-unit box.
#[component]
fn Logo() -> impl IntoView {
    view! {
        <svg class="logo" width="32" height="32" viewBox="0 0 100 100" aria-hidden="true">
            <rect width="100" height="100" rx="22" fill="#0C447C"></rect>
            <path d="M24 68 C40 60 52 38 72 22 C66 40 67 56 78 68 Z" fill="#FFFFFF"></path>
            <path d="M18 80 q10.5 -7 21 0 t21 0 t21 0" fill="none" stroke="#85B7EB" stroke-width="6.5" stroke-linecap="round"></path>
        </svg>
    }
}

#[component]
fn MonthNav() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    view! {
        <div class="month-nav">
            <button class="round" aria-label=t!("Previous month") on:click=move |_| ctx.month.update(|m| *m = shift_month(m, -1))>"‹"</button>
            <h1 class="month">{move || month_label(&ctx.month.get())}</h1>
            <button class="round" aria-label=t!("Next month") on:click=move |_| ctx.month.update(|m| *m = shift_month(m, 1))>"›"</button>
        </div>
    }
}

#[component]
fn Overview() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    // Categories left out, as on Reports; investments by default. Stays across months.
    let exclude = investments_left_out(ctx);
    let all_kinds = RwSignal::new(Report::Saldo);
    // "prev" (default), "year" (same month last year), "none", or a month (YYYY-MM).
    let compare = RwSignal::new("prev".to_string());
    let compare_month = move || {
        let m = ctx.month.get();
        match compare.get().as_str() {
            "none" => None,
            "prev" => Some(shift_month(&m, -1)),
            "year" => Some(shift_month(&m, -12)),
            other => Some(other.to_string()),
        }
    };
    let left_out = move |id: &Option<String>| exclude.with(|ex| ex.contains(&id.clone().unwrap_or_else(|| UNSORTED_CATEGORY_ID.into())));
    // Lines without the categories left out; a group line keeps its budget and loses
    // what those categories spent.
    let visible = move |mut o: BudgetOverview| {
        for lines in [&mut o.income, &mut o.fixed, &mut o.variable, &mut o.investment] {
            lines.retain(|l| l.group_line || !left_out(&l.category_id));
            for l in lines.iter_mut().filter(|l| l.group_line) {
                l.members.retain(|m| !left_out(&m.category_id));
                l.actual_cents = l.members.iter().map(|m| m.actual_cents).sum();
            }
        }
        o
    };
    let ov = Memo::new(move |_| visible(ctx.data.with(|d| d.budget_overview_grouped(&ctx.month.get()))));
    // Actual per line in the comparison month.
    let before = Memo::new(move |_| {
        compare_month().map(|m| {
            let o = visible(ctx.data.with(|d| d.budget_overview_grouped(&m)));
            // Lines, and a group's categories by their id, so those compare too.
            o.income
                .into_iter()
                .chain(o.fixed)
                .chain(o.variable)
                .chain(o.investment)
                .flat_map(|l| {
                    let members: Vec<_> = l.members.iter().map(|m| (m.category_id.clone(), m.actual_cents)).collect();
                    std::iter::once((line_key(&l), l.actual_cents)).chain(members)
                })
                .collect::<Vec<_>>()
        })
    });
    let compare_label = Signal::derive(move || compare_month().map(|m| month_label(&m)));
    let empty = move || ov.with(|o| o.income.is_empty() && o.fixed.is_empty() && o.variable.is_empty() && o.investment.is_empty());
    let has_investments = move || ov.with(|o| !o.investment.is_empty());
    // Totals from the lines shown, so leaving a category out changes them too.
    let sums = move |pick: fn(&BudgetOverview) -> &Vec<BudgetLine>| {
        ov.with(|o| {
            let ls = pick(o);
            (ls.iter().map(|l| l.actual_cents).sum::<i64>(), ls.iter().filter_map(|l| l.budget_cents).reduce(|a, b| a + b))
        })
    };
    let card = move |title: &'static str, kind: CategoryKind, pick: fn(&BudgetOverview) -> &Vec<BudgetLine>, tone: &'static str| {
        view! {
            <div class=format!("sum-card {tone}")>
                <span class="sum-title">{title}</span>
                {move || {
                    let (actual, budget) = sums(pick);
                    let (amount, label, over) = budget_status(kind, budget.map(|b| b - actual), budget);
                    let fill = budget.filter(|b| *b > 0).map(|b| (actual as f64 / b as f64).clamp(0.0, 1.0) * 100.0);
                    view! {
                        <strong class="sum-value">"€ "{budget_text(actual)}</strong>
                        <span class="muted">{budget.map(|b| t!("of € {}", budget_text(b))).unwrap_or_else(|| t!("no budget").into())}</span>
                        <div class="sum-bar">{fill.map(|f| view! { <div style:width=format!("{f:.1}%")></div> })}</div>
                        <span class="sum-status">{status_badge(amount.clone(), label, over)}</span>
                    }
                }}
            </div>
        }
    };
    let saldo_card = move || {
        let (inc, inc_b) = sums(|o| &o.income);
        let (fix, fix_b) = sums(|o| &o.fixed);
        let (var, var_b) = sums(|o| &o.variable);
        let (inv, inv_b) = sums(|o| &o.investment);
        let saldo = inc - fix - var - inv;
        let budget = inc_b.unwrap_or(0) - fix_b.unwrap_or(0) - var_b.unwrap_or(0) - inv_b.unwrap_or(0);
        let diff = saldo - budget;
        // Within 2% of the budgeted income counts as on plan, like the budget lines.
        let on_plan = diff == 0 || within_tolerance(diff, inc_b.unwrap_or(0));
        let worse = !on_plan && diff < 0;
        view! {
            <div class="sum-card saldo">
                <span class="sum-title">{t!("Net")}</span>
                <strong class="sum-value">{euro(saldo)}</strong>
                <span class="muted">{t!("budgeted {}", euro(budget))}</span>
                <div class="sum-bar"></div>
                <span class="sum-status">
                    <span class="badge" class:over=worse class:ok=!worse>
                        {match diff {
                            0 => t!("exactly as planned").to_string(),
                            _ if on_plan => t!("as planned").to_string(),
                            d if d > 0 => t!("{} better than budgeted", euro(d)),
                            d => t!("{} worse than budgeted", euro(-d)),
                        }}
                    </span>
                </span>
            </div>
        }
    };
    // How the year of the month shown ends: actual so far, then the budget.
    let forecast_card = move || {
        let year: i32 = ctx.month.get()[..4].parse().unwrap_or(2026);
        let result: i64 = ctx.data.with(|d| year_forecast(d, year).iter().map(|m| m.totals.saldo()).sum());
        view! {
            <div class="sum-card saldo" class:neg={result < 0} title=t!("Actual so far, then as budgeted; with investments and extra income")>
                <span class="sum-title">{t!("Expected {}", year)}</span>
                <strong class="sum-value">{euro(result)}</strong>
                <span class="muted">{t!("{} per month", euro(result / 12))}</span>
                <div class="sum-bar"></div>
                <span class="sum-status">{t!("actual + budget")}</span>
            </div>
        }
    };
    // Months to compare with: the twelve before the current one.
    let compare_options = Signal::derive(move || {
        let m = ctx.month.get();
        [("prev", t!("Previous month")), ("year", t!("Same month last year")), ("none", t!("Nothing"))]
            .into_iter()
            .map(|(v, l)| ComboOption::new(v, l))
            .chain((1..=12).map(|i| shift_month(&m, -i)).map(|mm| {
                let label = month_label(&mm);
                ComboOption::new(mm, label).in_group(t!("Another month"))
            }))
            .collect::<Vec<_>>()
    });

    view! {
        <header class="page-head">
            <MonthNav/>
            <div class="spacer"></div>
            <button on:click=move |_| ctx.page.set(Page::Budget)>{t!("Edit budget")}</button>
        </header>
        <section class="sum-cards">
            {card(t!("Income"), CategoryKind::Income, |o| &o.income, "income")}
            {card(t!("Fixed costs"), CategoryKind::Fixed, |o| &o.fixed, "fixed")}
            {card(t!("Variable"), CategoryKind::Variable, |o| &o.variable, "variable")}
            <Show when=has_investments>
                {card(t!("Investments"), CategoryKind::Investment, |o| &o.investment, "investment")}
            </Show>
            {saldo_card}
            {forecast_card}
        </section>
        <div class="filters">
            <CategoryFilter exclude=exclude report=all_kinds/>
            <label class="inline">{t!("Compare with")}
                <ComboBox label=t!("Compare with") options=compare_options value=compare on_change=move |v| compare.set(v)/>
            </label>
            <span class="legend">
                <span><i class="key actual"></i>{t!("Actual")}</span>
                <span><i class="key budget"></i>{t!("Budget")}</span>
                {move || compare_label.get().map(|l| view! { <span><i class="key before"></i>{l}</span> })}
            </span>
        </div>
        <Show when=empty>
            <div class="panel getting-started">
                {move || {
                    let (accounts, transactions, budgets) =
                        ctx.data.with(|d| (!d.accounts.is_empty(), !d.transactions.is_empty(), !d.budgets.is_empty()));
                    let step = |done: bool, text: &'static str, button: &'static str, to: Page| {
                        view! {
                            <li class:done=done>
                                <span>{text}</span>
                                {(!done).then(|| view! { <button class="small" on:click=move |_| ctx.page.set(to)>{button}</button> })}
                            </li>
                        }
                    };
                    if accounts && transactions && budgets {
                        view! { <p class="empty">{t!("No budget or transactions this month.")}</p> }.into_any()
                    } else {
                        view! {
                            <h2>{t!("Getting started")}</h2>
                            <ol>
                                {step(accounts, t!("Add an account"), t!("Accounts"), Page::Accounts)}
                                {step(transactions, t!("Import your statements (CAMT.053)"), t!("Transactions"), Page::Transactions)}
                                {step(budgets, t!("Set a budget per month"), t!("Budget"), Page::Budget)}
                            </ol>
                        }.into_any()
                    }
                }}
            </div>
        </Show>
        <BudgetSection title=t!("Income") kind=CategoryKind::Income lines=Signal::derive(move || ov.get().income) before=before/>
        <BudgetSection title=t!("Fixed costs") kind=CategoryKind::Fixed lines=Signal::derive(move || ov.get().fixed) before=before/>
        <BudgetSection title=t!("Variable spending") kind=CategoryKind::Variable lines=Signal::derive(move || ov.get().variable) before=before/>
        <BudgetSection title=t!("Investments") kind=CategoryKind::Investment lines=Signal::derive(move || ov.get().investment) before=before/>
    }
}

/// All payments, or only online / in-store ones (derived from the description).
#[component]
fn ChannelSelect(channel: RwSignal<Option<Channel>>) -> impl IntoView {
    let options = vec![
        ComboOption::new("", t!("All channels")),
        ComboOption::new("online", channel_label(Channel::Online)),
        ComboOption::new("store", channel_label(Channel::InStore)),
    ];
    let value = move || match channel.get() {
        None => "",
        Some(Channel::Online) => "online",
        Some(Channel::InStore) => "store",
    };
    view! {
        <ComboBox
            label=t!("Channel")
            title=t!("Online or in store, derived from the description")
            options=options
            value=Signal::derive(move || value().to_string())
            on_change=move |v: String| {
                channel.set(match v.as_str() {
                    "online" => Some(Channel::Online),
                    "store" => Some(Channel::InStore),
                    _ => None,
                })
            }
        />
    }
}

impl Report {
    /// The categories a report is made of: expenses, income, or both for the net.
    fn covers(self, kind: CategoryKind) -> bool {
        match self {
            Report::Expenses => kind.is_expense(),
            Report::Income => kind.is_income(),
            Report::Saldo | Report::Budget => kind.counts(),
        }
    }
}

/// The categories left out of a chart, starting with the investments (a renovation
/// would dwarf normal spending); check them in the category filter to see them. Set
/// once the data is there, since the page can open before it has loaded.
fn investments_left_out(ctx: Ctx) -> RwSignal<Vec<String>> {
    let exclude = RwSignal::new(Vec::<String>::new());
    // Only with the setting on (Settings); by default everything counts.
    if !stored_flag(EXCLUDE_INVESTMENTS_KEY) {
        return exclude;
    }
    let done = StoredValue::new(false);
    Effect::new(move |_| {
        let ids: Vec<String> = ctx.data.with(|d| {
            d.categories.iter().filter(|c| c.kind == CategoryKind::Investment).map(|c| c.id.clone()).collect()
        });
        if !done.get_value() && ctx.data.with(|d| !d.categories.is_empty()) {
            done.set_value(true);
            exclude.set(ids);
        }
    });
    exclude
}

/// The categories a report can show, grouped: those of its kind(s), plus "To
/// categorise" (money in and out still to categorise).
fn report_categories(d: &Dataset, report: Report) -> Vec<(String, Vec<Category>)> {
    grouped(d)
        .into_iter()
        .map(|(g, cats)| {
            let cats = cats
                .into_iter()
                .filter(|c| !c.disabled && (c.id == UNSORTED_CATEGORY_ID || report.covers(c.kind)))
                .collect::<Vec<_>>();
            (g, cats)
        })
        .filter(|(_, cats)| !cats.is_empty())
        .collect()
}

/// What the category selection shows, in words: None for the total, else "fixed
/// costs", one name, "without X", or a count.
fn selection_label(d: &Dataset, report: Report, exclude: &[String]) -> Option<String> {
    let cats: Vec<Category> = report_categories(d, report).into_iter().flat_map(|(_, c)| c).collect();
    let (out, inn): (Vec<&Category>, Vec<&Category>) = cats.iter().partition(|c| exclude.contains(&c.id));
    let only = |k: CategoryKind| inn.iter().all(|c| c.kind == k && c.id != UNSORTED_CATEGORY_ID)
        && cats.iter().filter(|c| c.kind == k && c.id != UNSORTED_CATEGORY_ID).all(|c| !exclude.contains(&c.id));
    Some(match (inn.as_slice(), out.as_slice()) {
        (_, []) => return None,
        ([], _) => t!("no categories").into(),
        ([one], _) => one.name.clone(),
        (_, [one]) => t!("without {}", one.name),
        _ if report == Report::Expenses && only(CategoryKind::Fixed) => t!("fixed costs").into(),
        _ if report == Report::Expenses && only(CategoryKind::Variable) => t!("variable").into(),
        (i, o) if i.len() <= o.len() => t!("{} categories", i.len()),
        (_, o) => t!("without {} categories", o.len()),
    })
}

/// Which categories a report counts, as checkboxes in one dropdown: all checked is the
/// total, uncheck one (a renovation) to leave it out, or keep one for just that
/// category. Stores what is left out, so new categories count by default.
#[component]
fn CategoryFilter(exclude: RwSignal<Vec<String>>, report: RwSignal<Report>) -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let label = move || ctx.data.with(|d| exclude.with(|ex| selection_label(d, report.get(), ex)));
    let summary = move || match label() {
        None => t!("All categories").to_string(),
        Some(l) => capitalize(&l),
    };
    // Leave out every category of the report except those `keep` accepts.
    let keep_only = move |keep: fn(&Category) -> bool| {
        let out = ctx.data.with(|d| {
            report_categories(d, report.get_untracked()).into_iter().flat_map(|(_, c)| c).filter(|c| !keep(c)).map(|c| c.id).collect()
        });
        exclude.set(out);
    };
    let toggle = move |id: String| {
        exclude.update(|ex| match ex.iter().position(|x| *x == id) {
            Some(i) => {
                ex.remove(i);
            }
            None => ex.push(id),
        })
    };
    view! {
        <details class="dropdown" class:on=move || label().is_some()>
            <summary title=t!("Which categories this report counts")>{summary}</summary>
            <div class="dropdown-menu">
                <div class="quick">
                    <button class="link" on:click=move |_| exclude.set(Vec::new())>{t!("All")}</button>
                    <button class="link" on:click=move |_| keep_only(|_| false)>{t!("Nothing")}</button>
                    <Show when=move || report.get() == Report::Expenses>
                        <button class="link" on:click=move |_| keep_only(|c| c.kind == CategoryKind::Fixed && c.id != UNSORTED_CATEGORY_ID)>{t!("Fixed costs")}</button>
                        <button class="link" on:click=move |_| keep_only(|c| c.kind == CategoryKind::Variable && c.id != UNSORTED_CATEGORY_ID)>{t!("Variable")}</button>
                    </Show>
                </div>
                {move || ctx.data.with(|d| report_categories(d, report.get()).into_iter().map(|(group, cats)| {
                    let items = cats.into_iter().map(|c| {
                        let (id, id2) = (c.id.clone(), c.id.clone());
                        view! {
                            <label class="check">
                                <input
                                    type="checkbox"
                                    prop:checked=move || exclude.with(|e| !e.contains(&id))
                                    on:change=move |_| toggle(id2.clone())
                                />
                                {c.name.clone()}
                            </label>
                        }
                    }).collect_view();
                    view! { <fieldset><legend>{group}</legend>{items}</fieldset> }
                }).collect_view())}
            </div>
        </details>
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Report {
    Expenses,
    Income,
    Saldo,
    /// The result per month against the budgeted result.
    Budget,
}

/// Trends through a year: expenses, income or the net, one bar per month, over the
/// categories picked in the category filter.
#[component]
fn ReportsPage() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let (this_year, this_month) = {
        let t = today();
        (t[..4].parse::<i32>().unwrap_or(2026), t[..7].to_string())
    };
    let year = RwSignal::new(this_year);
    let report = RwSignal::new(Report::Expenses);
    let channel = RwSignal::new(None::<Channel>);
    // Categories left out (the filter shows the rest checked); investments by default.
    let exclude = investments_left_out(ctx);

    let series = move || match report.get() {
        Report::Expenses => ReportSeries::ExpensesTotal,
        Report::Income => ReportSeries::IncomeTotal,
        Report::Saldo | Report::Budget => ReportSeries::Saldo,
    };
    let this_month_c = this_month.clone();
    let points = Signal::derive(move || {
        let y = year.get();
        let months: Vec<String> = (1..=12).map(|m| format!("{y:04}-{m:02}")).collect();
        if report.get() == Report::Budget {
            // Only complete months: one just begun isn't against its budget yet.
            let done = complete_months(y);
            return ctx.data.with(|d| {
                months
                    .into_iter()
                    .enumerate()
                    .map(|(i, month)| {
                        let cents = result_against_budget(d, &month, i < done);
                        crate::charts::MonthPoint { month, cents, forecast: None, pace: None }
                    })
                    .collect::<Vec<_>>()
            });
        }
        let values = exclude.with(|ex| ctx.data.with(|d| d.report_series_filtered(&months, &series(), channel.get(), ex)));
        months
            .into_iter()
            .zip(values)
            .map(|(month, v)| {
                let cents = (month.as_str() <= this_month_c.as_str()).then_some(v);
                crate::charts::MonthPoint { month, cents, forecast: None, pace: None }
            })
            .collect::<Vec<_>>()
    });
    let tone = Signal::derive(move || match report.get() {
        Report::Expenses => crate::charts::Tone::Expense,
        Report::Income => crate::charts::Tone::Income,
        Report::Saldo | Report::Budget => crate::charts::Tone::Signed,
    });
    let label = Signal::derive(move || {
        let r = report.get();
        let sel = ctx.data.with(|d| exclude.with(|ex| selection_label(d, r, ex)));
        let name = match r {
            Report::Saldo => t!("Net"),
            Report::Expenses => t!("Expenses"),
            Report::Income => t!("Income"),
            Report::Budget => t!("Result against budget"),
        };
        match (r, sel) {
            (Report::Saldo | Report::Budget, _) => name.to_string(),
            (_, s) => format!("{name} · {}", s.unwrap_or_else(|| t!("total").into())),
        }
    });
    let stats = move || {
        points.with(|p| {
            let vals: Vec<(&str, i64)> = p.iter().filter_map(|x| x.cents.map(|c| (x.month.as_str(), c))).collect();
            let total: i64 = vals.iter().map(|(_, c)| c).sum();
            let avg = if vals.is_empty() { 0 } else { total / vals.len() as i64 };
            let peak = vals.iter().max_by_key(|(_, c)| *c).map(|(m, c)| (m.to_string(), *c));
            let low = vals.iter().min_by_key(|(_, c)| *c).map(|(m, c)| (m.to_string(), *c));
            (total, avg, peak, low)
        })
    };
    // With one category picked, the bars take that category's color.
    let single_tone = move || {
        ctx.data.with(|d| {
            exclude.with(|ex| {
                let mut kept = report_categories(d, report.get()).into_iter().flat_map(|(_, c)| c).filter(|c| !ex.contains(&c.id));
                match (kept.next(), kept.next()) {
                    (Some(c), None) => cat_tone(Some(&c)),
                    _ => String::new(),
                }
            })
        })
    };
    // Left-out categories carry over (Net without the renovation too).
    let pick_report = move |r: Report| report.set(r);
    let month_short = month_short_of;
    let tab_on = move |r: Report| report.get() == r;
    let (this_year_so_far, whole_year) = (t!("This year so far"), t!("Whole year"));
    let (highest, lowest) = (t!("Highest month"), t!("Lowest month"));
    // The chart's months as CSV in Downloads: semicolons and decimal commas, as Dutch
    // spreadsheets read them; months still to come are left empty.
    let exported = RwSignal::new(None::<String>);
    let export_csv = move |_| {
        let name = format!("Fin {} {}", label.get_untracked(), year.get_untracked());
        let mut csv = format!("{};{}\n", t!("Month"), label.get_untracked());
        for p in points.get_untracked() {
            let amount = p.cents.map(|c| format!("{}{},{:02}", if c < 0 { "-" } else { "" }, c.abs() / 100, c.abs() % 100)).unwrap_or_default();
            csv.push_str(&format!("{};{}\n", month_label(&p.month), amount));
        }
        spawn_local(async move {
            match api::export_csv(name, csv).await {
                Ok(path) => exported.set(Some(t!("Saved: {}", path))),
                Err(e) => ctx.error.set(Some(e)),
            }
        });
    };

    view! {
        <header class="page-head">
            <button class="round" aria-label=t!("Previous year") on:click=move |_| year.update(|y| *y -= 1)>"‹"</button>
            <h1 class="year">{t!("Reports")}" "{move || year.get()}</h1>
            <button class="round" aria-label=t!("Next year") on:click=move |_| year.update(|y| *y += 1)>"›"</button>
            <div class="spacer"></div>
            <button title=t!("Save this report's months as a CSV file in Downloads") on:click=export_csv>{t!("Export CSV")}</button>
        </header>
        {move || exported.get().map(|m| view! { <p class="hint">{m}</p> })}
        <div class="filters">
            <div class="segmented" role="group" aria-label=t!("Report")>
                <button class:on=move || tab_on(Report::Expenses) aria-pressed=move || tab_on(Report::Expenses).to_string() on:click=move |_| pick_report(Report::Expenses)>{t!("Expenses")}</button>
                <button class:on=move || tab_on(Report::Income) aria-pressed=move || tab_on(Report::Income).to_string() on:click=move |_| pick_report(Report::Income)>{t!("Income")}</button>
                <button class:on=move || tab_on(Report::Budget) aria-pressed=move || tab_on(Report::Budget).to_string() on:click=move |_| pick_report(Report::Budget)>{t!("Budget")}</button>
            </div>
            // The budget report has no category or channel to filter on.
            <Show when=move || !tab_on(Report::Budget)>
                <CategoryFilter exclude=exclude report=report/>
                <ChannelSelect channel=channel/>
            </Show>
        </div>
        <Show when=move || !tab_on(Report::Budget)>
        <section class="panel totals report-totals">
            <span></span>
            <span class="t-head">{move || if year.get() == this_year { this_year_so_far } else { whole_year }}</span>
            <span class="t-head">{t!("Average per month")}</span>
            <span class="t-head">{highest}</span>
            <span class="t-head">{lowest}</span>
            <span class="t-label">{move || label.get()}</span>
            <span class="t-cell">{move || euro(stats().0)}</span>
            <span class="t-cell">{move || euro(stats().1)}</span>
            <span class="t-cell">{move || stats().2.map(|(m, c)| format!("{} · {}", month_short(&m), euro(c))).unwrap_or_else(|| "–".into())}</span>
            <span class="t-cell">{move || stats().3.map(|(m, c)| format!("{} · {}", month_short(&m), euro(c))).unwrap_or_else(|| "–".into())}</span>
        </section>
        </Show>
        <section class=move || format!("panel chart-panel {}", single_tone())>
            <div class="section-head">
                <h2>{move || label.get()}</h2>
                <span class="muted">{move || if tab_on(Report::Budget) {
                    t!("Per complete month: up is better than budgeted, down worse")
                } else {
                    t!("Per month, with the average as a line")
                }}</span>
            </div>
            <crate::charts::MonthlyBarChart points=points tone=tone label=label plain=Signal::derive(move || tab_on(Report::Budget))/>
        </section>
        <details class="panel table-view">
            <summary>{t!("As a table")}</summary>
            <table>
                <thead><tr><th>{t!("Month")}</th><th>{move || label.get()}</th></tr></thead>
                <tbody>
                    {move || points.get().into_iter().map(|p| view! {
                        <tr>
                            <td>{month_label(&p.month)}</td>
                            <td>{p.cents.map(euro).unwrap_or_else(|| "–".into())}</td>
                        </tr>
                    }).collect_view()}
                </tbody>
            </table>
        </details>
    }
}

/// How many months of `year` are complete: all 12 for a past year, none for a future
/// one, and this year the months before the current one (a month just begun would
/// make every average drop).
fn complete_months(year: i32) -> usize {
    let now = today();
    let this_year: i32 = now[..4].parse().unwrap_or(year);
    match year.cmp(&this_year) {
        std::cmp::Ordering::Less => 12,
        std::cmp::Ordering::Greater => 0,
        std::cmp::Ordering::Equal => now[5..7].parse::<usize>().unwrap_or(1).saturating_sub(1),
    }
}

/// The year's forecast (Dataset::forecast) as of today: a past year is all actual, a
/// future one all forecast, this year actual up to the current month.
fn year_forecast(d: &Dataset, year: i32) -> Vec<fin_shared::MonthForecast> {
    d.forecast(year, &forecast_from(year))
}

/// The first month of `year` that is forecast rather than actual.
fn forecast_from(year: i32) -> String {
    let now = today();
    let this_year: i32 = now[..4].parse().unwrap_or(year);
    match year.cmp(&this_year) {
        std::cmp::Ordering::Less => format!("{year:04}-13"),
        std::cmp::Ordering::Greater => format!("{year:04}-00"),
        std::cmp::Ordering::Equal => now[..7].to_string(),
    }
}

/// One month's result against its budget: fixed income minus fixed and variable costs,
/// without extra income and investments, minus the budgeted result. None for a month
/// that isn't complete yet.
fn result_against_budget(d: &Dataset, month: &str, complete: bool) -> Option<i64> {
    if !complete {
        return None;
    }
    let o = d.budget_overview(month);
    let fixed_income: i64 = o.income.iter().filter(|l| l.kind == CategoryKind::Income && l.category_id.is_some()).map(|l| l.actual_cents).sum();
    let (a, b) = (o.actual, o.budget);
    Some((fixed_income - a.fixed - a.variable) - (b.income - b.fixed - b.variable))
}


/// "€ x left" style status for a line or a section: (amount text, label, over budget).
/// Within 2% of the budget: close enough to call it paid / received / on budget, so
/// €2,76 of €180 doesn't show as still to pay.
fn within_tolerance(open: i64, budget: i64) -> bool {
    budget > 0 && open.abs() * 50 <= budget
}

/// A budget status as a badge: red when over budget, green when done (paid,
/// received, on budget), grey while still open.
fn status_badge(amount: String, label: &'static str, over: bool) -> impl IntoView {
    let done = !over && amount.is_empty() && !label.is_empty();
    (!label.is_empty()).then(|| view! {
        <span class="badge" class:over=over class:ok=done>
            {(!amount.is_empty()).then(|| view! { <strong>{amount}</strong>" " })}{label}
        </span>
    })
}

fn budget_status(kind: CategoryKind, open: Option<i64>, budget: Option<i64>) -> (String, &'static str, bool) {
    let Some(open) = open else { return (String::new(), t!("no budget"), false) };
    if budget.is_some_and(|b| within_tolerance(open, b)) {
        let done = match kind {
            CategoryKind::Fixed => t!("paid"),
            CategoryKind::Income | CategoryKind::IrregularIncome => t!("received"),
            _ => t!("on budget"),
        };
        return (String::new(), done, false);
    }
    match kind {
        _ if open < 0 && !kind.is_income() => (euro(-open), t!("over"), true),
        CategoryKind::Fixed if open == 0 => (String::new(), t!("paid"), false),
        CategoryKind::Fixed => (euro(open), t!("still to pay"), false),
        CategoryKind::Variable | CategoryKind::Investment => (euro(open), t!("left"), false),
        CategoryKind::Income | CategoryKind::IrregularIncome if open > 0 => (euro(open), t!("still to receive"), false),
        CategoryKind::Income | CategoryKind::IrregularIncome => (String::new(), t!("received"), false),
        // Transfers never reach the budget overview.
        CategoryKind::Transfer => (String::new(), "", false),
    }
}

#[component]
fn BudgetSection(
    title: &'static str,
    kind: CategoryKind,
    lines: Signal<Vec<BudgetLine>>,
    /// Actual per category in the comparison month, if any.
    before: Memo<Option<Vec<(Option<String>, i64)>>>,
) -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let summary = move || {
        lines.with(|ls| {
            let actual: i64 = ls.iter().map(|l| l.actual_cents).sum();
            let budget: Option<i64> = ls.iter().filter_map(|l| l.budget_cents).reduce(|a, b| a + b);
            let (amount, label, over) = budget_status(kind, budget.map(|b| b - actual), budget);
            let head = match budget {
                Some(b) => t!("€ {} of € {}", budget_text(actual), budget_text(b)),
                None => format!("€ {}", budget_text(actual)),
            };
            view! {
                <span class="muted">{head}" "</span>
                {status_badge(amount, label, over)}
            }
        })
    };

    // A horizontal bar chart, one row per category on a shared scale: the actual amount
    // in the category's color, a line at the budget, and a thin grey bar for the month
    // compared with. Amount and budget at the end; orange when over budget (beyond 2%).
    let chart = move || {
        let ls = lines.get();
        let prev = before.get();
        let prev_of = |key: &Option<String>| prev.as_ref().map(|p| p.iter().find(|(c, _)| c == key).map_or(0, |(_, v)| *v));
        let max = ls
            .iter()
            .map(|l| l.actual_cents.max(l.budget_cents.unwrap_or(0)).max(prev_of(&line_key(l)).unwrap_or(0)))
            .max()
            .unwrap_or(0)
            .max(1) as f64;
        let pct = move |v: i64| format!("{:.2}%", (v.max(0) as f64 / max * 100.0).min(100.0));
        ls.into_iter().map(|l| {
            let (amount, label, over) = budget_status(kind, l.open_cents(), l.budget_cents);
            let was = prev_of(&line_key(&l));
            // A group line takes the look of its first category (they share the group's).
            let (tone, badge) = ctx.data.with(|d| {
                let id = if l.group_line { l.members.first().and_then(|m| m.category_id.clone()) } else { l.category_id.clone() };
                let c = d.category(id.as_deref());
                (cat_tone(c), cat_badge(c))
            });
            let name = if l.group_line { l.name.clone() } else { line_name(l.category_id.as_deref(), &l.name) };
            let mut tip = format!("{name}: € {}", budget_text(l.actual_cents));
            if let Some(b) = l.budget_cents {
                tip.push_str(&format!(" {}", t!("of € {} budget", budget_text(b))));
            }
            if !amount.is_empty() || !label.is_empty() {
                tip.push_str(&format!(" · {} {}", amount, label).replace("  ", " "));
            }
            if let Some(w) = was {
                tip.push_str(&format!(" · {}", t!("compared month € {}", budget_text(w))));
            }
            // One status per budgeted line, in its own column: over budget (spending
            // beyond 2%) or on budget; income only once it is in.
            let flag = l.budget_cents.and_then(|b| {
                let open = b - l.actual_cents;
                if over {
                    Some((t!("over"), true))
                } else if kind.is_income() && open > 0 && !within_tolerance(open, b) {
                    None
                } else {
                    Some((t!("on budget"), false))
                }
            });
            let row = view! {
                <div class="bc-label">
                    {badge}
                    <span>{name.clone()}</span>
                </div>
                <div class="bc-track">
                    <div class="bc-bar" style:width=pct(l.actual_cents)></div>
                    {was.map(|w| view! { <div class="bc-before" style:width=pct(w)></div> })}
                    {l.budget_cents.map(|b| view! { <div class="bc-budget" style:left=pct(b)></div> })}
                </div>
                <div class="bc-value">
                    <strong>{euro_text(l.actual_cents)}</strong>
                    {l.budget_cents.map(|b| view! { <span class="muted">" / "{euro_text(b)}</span> })}
                </div>
                <div class="bc-flag">
                    {flag.map(|(text, over)| view! { <span class="badge" class:over=over class:ok=!over>{text}</span> })}
                </div>
            };
            if !l.group_line {
                return view! { <div class=format!("bc-row {tone}") title=tip>{row}</div> }.into_any();
            }
            // A group: its categories underneath, each with what it spent and the month
            // compared with.
            let members = l.members.into_iter().map(|m| {
                let was = prev_of(&m.category_id);
                view! {
                    <div class="bc-row sub">
                        <div class="bc-label"><span>{line_name(m.category_id.as_deref(), &m.name)}</span></div>
                        <div class="bc-track">
                            <div class="bc-bar" style:width=pct(m.actual_cents)></div>
                            {was.map(|w| view! { <div class="bc-before" style:width=pct(w)></div> })}
                        </div>
                        <div class="bc-value">
                            <strong>{euro_text(m.actual_cents)}</strong>
                        </div>
                        <div class="bc-flag"></div>
                    </div>
                }
            }).collect_view();
            view! {
                <details class=format!("bc-item group {tone}")>
                    <summary class="bc-row" title=tip>{row}</summary>
                    {members}
                </details>
            }
            .into_any()
        }).collect_view()
    };
    view! {
        <Show when=move || lines.with(|l| !l.is_empty())>
            <section class="panel budget-section">
                <div class="section-head">
                    <h2>{title}</h2>
                    <span class="section-status">{summary}</span>
                </div>
                <div class="bc">{chart}</div>
            </section>
        </Show>
    }
}

/// Categories grouped by their group, in list order.
fn grouped(ds: &Dataset) -> Vec<(String, Vec<Category>)> {
    let mut groups: Vec<(String, Vec<Category>)> = Vec::new();
    for c in &ds.categories {
        match groups.iter_mut().find(|(g, _)| *g == c.group) {
            Some((_, list)) => list.push(c.clone()),
            None => groups.push((c.group.clone(), vec![c.clone()])),
        }
    }
    groups
}

impl ComboOption {
    /// A category as a choice: its name and icon, under its group.
    fn category(c: &Category) -> Self {
        let group = if c.group.is_empty() { t!("No group").to_string() } else { c.group.clone() };
        let badge = c.clone();
        ComboOption::new(c.id.clone(), c.name.clone()).in_group(group).with_icon(move || cat_badge(Some(&badge)))
    }
}

/// Every category, grouped, for a filter.
fn all_category_choices(ds: &Dataset) -> Vec<ComboOption> {
    grouped(ds).into_iter().flat_map(|(_, cats)| cats).map(|c| ComboOption::category(&c)).collect()
}

/// The categories a transaction can be put in, grouped. Disabled categories, and "To
/// categorise" (where imports land, not a choice), are only listed when `selected`.
fn category_choices(ds: &Dataset, selected: Option<&str>) -> Vec<ComboOption> {
    grouped(ds)
        .into_iter()
        .flat_map(|(_, cats)| cats)
        .filter(|c| (!c.disabled && c.id != UNSORTED_CATEGORY_ID) || selected == Some(c.id.as_str()))
        .map(|c| ComboOption::category(&c))
        .collect()
}

/// "No category" (value "") and then [`category_choices`].
fn category_choices_or_none(ds: &Dataset, selected: Option<&str>) -> Vec<ComboOption> {
    std::iter::once(ComboOption::new("", t!("No category"))).chain(category_choices(ds, selected)).collect()
}

/// The standard "To categorise" category counts as not categorised yet.
fn is_unsorted(t: &Transaction) -> bool {
    t.is_unsorted()
}

/// Matches description, counterparty IBAN or the amount as shown (`-12,50`). `q` is lower case.
fn matches_query(t: &Transaction, q: &str) -> bool {
    t.description.to_lowercase().contains(q)
        || t.counterparty_iban.as_deref().is_some_and(|i| i.to_lowercase().contains(&q.replace(' ', "")))
        || i18n::cents(t.amount_cents).contains(q)
}

#[component]
fn Transactions() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let import_open = RwSignal::new(false);

    // Filters. Empty means "any". The month comes from the month navigation above.
    // Transactions come from imports; there is no manual entry.
    let query = RwSignal::new(String::new());
    let account = RwSignal::new(String::new());
    let category = RwSignal::new(String::new());
    let amount = RwSignal::new(String::new());
    let only_open = RwSignal::new(false);
    let channel = RwSignal::new(None::<Channel>);
    // The transaction whose "Categorise" / "Make a rule" form is open.
    let editing = RwSignal::new(None::<String>);
    // The transaction whose category is being changed inline (one at a time).
    let cat_editing = RwSignal::new(None::<String>);
    // The transaction whose "counts on" day is being changed.
    let date_editing = RwSignal::new(None::<String>);
    // The transaction whose split form is open.
    let split_open = RwSignal::new(None::<String>);

    let any_filter = move || {
        !query.with(|q| q.trim().is_empty())
            || !account.with(String::is_empty)
            || !category.with(String::is_empty)
            || !amount.with(|a| a.trim().is_empty())
            || channel.get().is_some()
    };
    let clear = move |_| {
        query.set(String::new());
        account.set(String::new());
        category.set(String::new());
        amount.set(String::new());
        channel.set(None);
    };

    // "To categorise" looks across months; otherwise the selected month.
    let rows = Memo::new(move |_| {
        let month = ctx.month.get();
        let (acc, cat) = (account.get(), category.get());
        let q = query.get().trim().to_lowercase();
        let amt = amount.get();
        let wanted_cents = parse_amount(&amt).map(i64::abs);
        let open = only_open.get();
        let ch = channel.get();
        ctx.data.with(|d| {
            d.transactions
                .iter()
                .filter(|t| if open { is_unsorted(t) } else { t.day().starts_with(&month) })
                .filter(|t| acc.is_empty() || t.account_id == acc)
                .filter(|t| cat.is_empty() || t.category_id.as_deref() == Some(cat.as_str()) || t.splits.iter().any(|s| s.category_id == cat))
                .filter(|t| ch.is_none() || t.channel() == ch)
                .filter(|t| q.is_empty() || matches_query(t, &q))
                // An amount matches either sign ("12,50" finds -12,50 too).
                .filter(|t| amt.trim().is_empty() || wanted_cents.is_some_and(|w| t.amount_cents.abs() == w))
                .cloned()
                .collect::<Vec<Transaction>>()
        })
    });
    let open_count = move || ctx.data.with(|d| d.transactions.iter().filter(|t| is_unsorted(t)).count());

    let account_name = move |id: &str| {
        ctx.data.with_untracked(|d| d.accounts.iter().find(|a| a.id == id).map(|a| a.name.clone())).unwrap_or_default()
    };

    view! {
        <header class="page-head">
            <MonthNav/>
            <div class="spacer"></div>
            <button class="primary" on:click=move |_| import_open.set(true)>{t!("Import…")}</button>
        </header>

        <div class="tx-filters panel" role="search">
            <label>{t!("Description")}
                <input type="search" placeholder=t!("Text or IBAN") prop:value=move || query.get() on:input=move |ev| query.set(event_target_value(&ev))/>
            </label>
            <label>{t!("Account")}
                <ComboBox
                    label=t!("Account")
                    options=Signal::derive(move || ctx.data.with(|d| {
                        std::iter::once(ComboOption::new("", t!("All accounts")))
                            .chain(d.accounts.iter().map(|a| ComboOption::new(a.id.clone(), a.name.clone())))
                            .collect::<Vec<_>>()
                    }))
                    value=account
                    on_change=move |v| account.set(v)
                />
            </label>
            <label>{t!("Category")}
                <ComboBox
                    label=t!("Category")
                    options=Signal::derive(move || ctx.data.with(|d| {
                        std::iter::once(ComboOption::new("", t!("All categories"))).chain(all_category_choices(d)).collect::<Vec<_>>()
                    }))
                    value=category
                    on_change=move |v| category.set(v)
                />
            </label>
            <label>{t!("Amount")}
                <input class="right" placeholder=i18n::cents(1250) prop:value=move || amount.get() on:input=move |ev| amount.set(event_target_value(&ev))/>
            </label>
            <label>{t!("Channel")}<ChannelSelect channel=channel/></label>
        </div>
        <div class="filters">
            <button class="chip" class:on=move || only_open.get() aria-pressed=move || only_open.get().to_string() on:click=move |_| only_open.update(|v| *v = !*v)>
                {t!("To categorise")}
                <span class="count">{open_count}</span>
            </button>
            <span class="muted">{move || {
                let n = rows.with(Vec::len);
                let scope = if only_open.get() { t!("all months").to_string() } else { month_label(&ctx.month.get()) };
                tn!(n, "{} transaction · {}", "{} transactions · {}", n, scope)
            }}</span>
            <Show when=any_filter>
                <button class="link" on:click=clear>{t!("Clear filters")}</button>
            </Show>
        </div>

        <section class="panel list">
            <Show when=move || rows.with(|r| r.is_empty())>
                <p class="empty">{move || match (only_open.get(), any_filter()) {
                    (true, false) => t!("Everything is categorised."),
                    (_, true) => t!("Nothing found with these filters."),
                    (false, false) => t!("No transactions this month."),
                }}</p>
            </Show>
            <For
                each=move || rows.get()
                key=|t| t.clone()
                children=move |t| {
                    let for_change = t.clone();
                    let id = t.id.clone();
                    let (open_id, form_id, link_id) = (t.id.clone(), t.id.clone(), t.id.clone());
                    let unsorted = t.is_unsorted();
                    let form_tx = t.clone();
                    let (split_id, split_toggle_id, split_tx) = (t.id.clone(), t.id.clone(), t.clone());
                    // The rule that put it in its category, if any (not when set by hand).
                    let rule = ctx.data.with_untracked(|d| {
                        d.matching_rule(&t.description, t.counterparty_iban.as_deref(), t.amount_cents)
                    }).filter(|m| t.category_id.as_deref() == Some(m.category_id.as_str()));
                    let toggle_form = move |_| {
                        let id = link_id.clone();
                        editing.update(|e| *e = if e.as_deref() == Some(id.as_str()) { None } else { Some(id) });
                    };
                    view! {
                        <div class=format!("tx-row {}", ctx.data.with_untracked(|d| tone_class(d, &t))) class:unsorted=unsorted>
                            <DateCell tx=t.clone() date_editing=date_editing/>
                            {ctx.data.with_untracked(|d| type_badge(d, &t))}
                            <div class="desc" title=t.description.clone()>
                                <span>{t.description.clone()}</span>
                                <small class="muted">
                                    {t.channel().map(|c| view! { <span class="tag">{channel_label(c)}</span> })}
                                    {t.counterparty_iban.clone()}
                                    {(!unsorted).then(|| match rule.clone() {
                                        Some(m) => {
                                            let (what, text) = match m.kind {
                                                None => (t!("own account").to_string(), t!("own account").to_string()),
                                                Some(k) => (format!("{} \u{201c}{}\u{201d}", rule_kind_label(k).to_lowercase(), m.value), m.value.clone()),
                                            };
                                            view! {
                                                <button class="rule-hit" title=t!("Categorised automatically by rule: {}. Click to categorise differently.", what) on:click=toggle_form>
                                                    <BoltIcon filled=true/>{text}
                                                </button>
                                            }.into_any()
                                        }
                                        None => view! {
                                            <button class="rule-make" aria-label=t!("Make a rule") title=t!("Make a rule: similar transactions land here automatically from now on") on:click=toggle_form>
                                                <BoltIcon filled=false/>
                                            </button>
                                        }.into_any(),
                                    })}
                                    // Parts counted elsewhere, and the button to split.
                                    {t.splits.iter().map(|s| {
                                        let name = ctx.data.with_untracked(|d| d.category(Some(&s.category_id)).map(|c| c.name.clone()).unwrap_or_default());
                                        view! { <span class="split-tag" title=t!("Part that counts in another category")>{name}" "{i18n::cents(s.amount_cents)}</span> }
                                    }).collect_view()}
                                    {(!unsorted).then(|| {
                                        let sid = split_toggle_id.clone();
                                        view! {
                                            <button class="rule-make" aria-label=t!("Split") title=t!("Split: let part of it count in another category") on:click=move |_| {
                                                let id = sid.clone();
                                                split_open.update(|e| *e = if e.as_deref() == Some(id.as_str()) { None } else { Some(id) });
                                            }>
                                                <SplitIcon/>
                                            </button>
                                        }
                                    })}
                                </small>
                            </div>
                            <span class="muted">{account_name(&t.account_id)}</span>
                            {if unsorted {
                                // Actionable: categorise with or without a rule.
                                view! {
                                    <button class="small primary" on:click=move |_| {
                                        let id = open_id.clone();
                                        editing.update(|e| *e = if e.as_deref() == Some(id.as_str()) { None } else { Some(id) });
                                    }>{t!("Categorise…")}</button>
                                }.into_any()
                            } else {
                                view! { <CategoryCell tx=for_change.clone() cat_editing=cat_editing/> }.into_any()
                            }}
                            <span class="right amount">{i18n::cents(t.amount_cents)}</span>
                            <button class="icon" aria-label=t!("Delete") on:click=move |_| {
                                let id = id.clone();
                                apply(ctx, async move { api::delete_transaction(&id).await })
                            }>
                                <TrashIcon/>
                            </button>
                        </div>
                        <Show when=move || editing.with(|e| e.as_deref() == Some(form_id.as_str()))>
                            <RuleForm tx=form_tx.clone() editing=editing/>
                        </Show>
                        <Show when=move || split_open.with(|e| e.as_deref() == Some(split_id.as_str()))>
                            <SplitForm tx=split_tx.clone() open=split_open/>
                        </Show>
                    }
                }
            />
        </section>

        <Show when=move || import_open.get()>
            <ImportDialog open=import_open/>
        </Show>
    }
}

#[derive(Clone, Copy, PartialEq)]
enum RuleChoice {
    Text,
    Iban,
    /// One-off: categorise only this transaction.
    None,
}

/// Categorise one transaction, optionally with a rule for similar ones: "description
/// contains …" (prefilled from the description), the counterparty IBAN, or no rule.
#[component]
fn RuleForm(tx: Transaction, editing: RwSignal<Option<String>>) -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let category = RwSignal::new(if tx.is_unsorted() { String::new() } else { tx.category_id.clone().unwrap_or_default() });
    let pattern = RwSignal::new(suggest_pattern(&tx.description));
    let has_iban = tx.counterparty_iban.is_some();
    let choice = RwSignal::new(if !pattern.get_untracked().is_empty() {
        RuleChoice::Text
    } else if has_iban {
        RuleChoice::Iban
    } else {
        RuleChoice::None
    });
    let tx = StoredValue::new(tx);

    // The rule as it stands: (kind, normalised value).
    // "Only for money in/out": the text rule follows this transaction's direction.
    let one_way = RwSignal::new(false);
    let incoming = tx.with_value(|t| t.amount_cents >= 0);
    let rule = move || -> Option<(RuleKind, String)> {
        match choice.get() {
            RuleChoice::Text => {
                let kind = match (one_way.get(), incoming) {
                    (false, _) => RuleKind::Text,
                    (true, true) => RuleKind::TextIn,
                    (true, false) => RuleKind::TextOut,
                };
                normalize_pattern(&pattern.get()).map(|p| (kind, p))
            }
            RuleChoice::Iban => tx.with_value(|t| t.counterparty_iban.clone()).map(|i| (RuleKind::Iban, i)),
            RuleChoice::None => None,
        }
    };
    let preview = move || {
        let (kind, value) = rule()?;
        let this = tx.with_value(|t| t.id.clone());
        let fits_this = tx.with_value(|t| rule_matches(kind, &value, t));
        let others = ctx.data.with(|d| {
            d.transactions.iter().filter(|t| t.id != this && t.is_unsorted() && rule_matches(kind, &value, t)).count()
        });
        Some(match (fits_this, others) {
            (false, _) => t!("Note: this text does not appear in this transaction's description.").to_string(),
            (true, 0) => t!("No other transactions to categorise match this; the rule applies to new imports.").to_string(),
            (true, n) => tn!(
                n,
                "Also matches {} other transaction still to categorise; it comes along.",
                "Also matches {} other transactions still to categorise; they come along.",
                n
            ),
        })
    };
    let submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let cat = category.get_untracked();
        if cat.is_empty() {
            ctx.error.set(Some(t!("Choose a category").into()));
            return;
        }
        if choice.get_untracked() == RuleChoice::Text && rule().is_none() {
            ctx.error.set(Some(t!("A text rule needs at least 3 characters").into()));
            return;
        }
        let rule = rule();
        let t = tx.get_value();
        let input = TransactionInput {
            id: Some(t.id),
            date: t.date,
            description: t.description,
            amount_cents: t.amount_cents,
            category_id: Some(cat.clone()),
            account_id: t.account_id,
        };
        spawn_local(async move {
            let saved = match api::save_transaction(input).await {
                Ok(d) => d,
                Err(e) => return ctx.error.set(Some(e)),
            };
            ctx.data.set(saved);
            if let Some((kind, value)) = rule {
                match api::add_category_rule(&cat, kind, &value).await {
                    Ok(r) => {
                        ctx.data.set(r.data);
                        offer_rule_for_year(ctx, &cat, kind, &value);
                    }
                    Err(e) => return ctx.error.set(Some(e)),
                }
            }
            ctx.error.set(None);
            editing.set(None);
        });
    };

    view! {
        <form class="rule-form" on:submit=submit>
            <label class="inline">{t!("Category")}
                <ComboBox
                    label=t!("Category")
                    options=ctx.data.with_untracked(|d| {
                        let selected = Some(category.get_untracked()).filter(|c| !c.is_empty());
                        category_choices_or_none(d, selected.as_deref())
                    })
                    value=category
                    on_change=move |v| category.set(v)
                />
            </label>
            <div class="rule-choices">
                <label class="check">
                    <input type="radio" name="rule" checked=move || choice.get() == RuleChoice::Text on:change=move |_| choice.set(RuleChoice::Text)/>
                    {t!("Rule: description contains")}
                </label>
                <input
                    class="pattern"
                    aria-label=t!("Text from the description")
                    prop:value=move || pattern.get()
                    on:input=move |ev| {
                        pattern.set(event_target_value(&ev));
                        choice.set(RuleChoice::Text);
                    }
                />
                <label class="check" title=t!("For counterparties that both pay and receive, like an employer or the tax office")>
                    <input type="checkbox" prop:checked=move || one_way.get() on:change=move |ev| {
                        one_way.set(event_target_checked(&ev));
                        choice.set(RuleChoice::Text);
                    }/>
                    {if incoming { t!("only for money in") } else { t!("only for money out") }}
                </label>
                {has_iban.then(|| view! {
                    <label class="check">
                        <input type="radio" name="rule" checked=move || choice.get() == RuleChoice::Iban on:change=move |_| choice.set(RuleChoice::Iban)/>
                        {t!("Rule: IBAN")}" "<span class="rule-value">{tx.with_value(|t| t.counterparty_iban.clone())}</span>
                    </label>
                })}
                <label class="check">
                    <input type="radio" name="rule" checked=move || choice.get() == RuleChoice::None on:change=move |_| choice.set(RuleChoice::None)/>
                    {t!("No rule, just this once")}
                </label>
            </div>
            {move || preview().map(|p| view! { <p class="muted rule-preview">{p}</p> })}
            <div class="actions">
                <button type="button" on:click=move |_| editing.set(None)>{t!("Cancel")}</button>
                <button type="submit" class="primary">{t!("Categorise")}</button>
            </div>
        </form>
    }
}

/// Settings: every rule in one place. Personal rules can be added and removed; the
/// standard rules that come with Fin are locked (a personal rule can override one:
/// the longest matching text wins, and IBAN rules win over text).
#[component]
fn SettingsPage() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let rules = RwSignal::new(Vec::<RuleInfo>::new());
    // Reload whenever the data changes (a rule added here, on Transactions or by fin-cli).
    Effect::new(move |_| {
        ctx.data.track();
        spawn_local(async move {
            match api::list_rules().await {
                Ok(r) => rules.set(r),
                Err(e) => ctx.error.set(Some(e)),
            }
        });
    });
    let query = RwSignal::new(String::new());
    let category = RwSignal::new(String::new());
    let kind = RwSignal::new(RuleKind::Text);
    let value = RwSignal::new(String::new());

    let add = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let (cat, k, v) = (category.get_untracked(), kind.get_untracked(), value.get_untracked());
        if cat.is_empty() {
            ctx.error.set(Some(t!("Choose a category").into()));
            return;
        }
        spawn_local(async move {
            match api::add_category_rule(&cat, k, &v).await {
                Ok(r) => {
                    ctx.data.set(r.data);
                    ctx.error.set(None);
                    value.set(String::new());
                    offer_rule_for_year(ctx, &cat, k, &normalize_rule(k, &v).unwrap_or(v));
                }
                Err(e) => ctx.error.set(Some(e)),
            }
        });
    };
    let cat_name = move |id: &str| ctx.data.with(|d| d.category(Some(id)).map(|c| c.name.clone()).unwrap_or_default());
    let visible = move |default: bool| {
        let q = query.get().trim().to_lowercase();
        rules.with(|rs| {
            rs.iter()
                .filter(|r| r.default == default)
                .filter(|r| q.is_empty() || r.value.contains(&q) || cat_name(&r.category_id).to_lowercase().contains(&q))
                .cloned()
                .collect::<Vec<_>>()
        })
    };
    let row = move |r: RuleInfo| {
        let RuleInfo { category_id, kind, value, default } = r;
        let (cid, v) = (category_id.clone(), value.clone());
        let (tone, badge) = ctx.data.with(|d| {
            let c = d.category(Some(&category_id));
            (cat_tone(c), cat_badge(c))
        });
        view! {
            <div class=format!("rule-row tinted {tone}")>
                <span class="rule-cat">{badge}{cat_name(&category_id)}</span>
                <span class="muted">{rule_kind_label(kind)}</span>
                <span class="rule-value">{value}</span>
                {if default {
                    view! { <span class="icon muted" title=t!("Default rule: fixed, but your own rule can override it")><LockIcon/></span> }.into_any()
                } else {
                    view! {
                        <button class="icon" aria-label=t!("Remove rule") on:click=move |_| {
                            let (cid, v) = (cid.clone(), v.clone());
                            apply(ctx, async move { api::remove_category_rule(&cid, kind, &v).await })
                        }><TrashIcon/></button>
                    }.into_any()
                }}
            </div>
        }
    };

    // Kept in the data file, so the backend names the standard categories in it too.
    let language = ctx.data.with_untracked(|d| d.language);
    let pick_language = move |ev: leptos::ev::Event| {
        if let Some(l) = Lang::from_tag(&event_target_value(&ev)).filter(|l| *l != language) {
            apply(ctx, api::set_language(l));
        }
    };

    view! {
        <header class="page-head"><h1>{t!("Settings")}</h1></header>
        <section class="panel settings">
            <div class="section-head"><h2>{t!("Display")}</h2></div>
            <label class="inline">{t!("Language")}
                <select on:change=pick_language>
                    {Lang::ALL.map(|l| view! { <option value=l.tag() selected=l == language>{l.name()}</option> }).collect_view()}
                </select>
            </label>
            <label class="check">
                <input type="checkbox" prop:checked=stored_flag(EXCLUDE_INVESTMENTS_KEY)
                    on:change=move |ev| store_flag(EXCLUDE_INVESTMENTS_KEY, event_target_checked(&ev))/>
                {t!("Leave investments out of Overview and Reports by default")}
            </label>
        </section>
        <section class="panel settings">
            <div class="section-head">
                <h2><BoltIcon filled=true/>" "{t!("Rules")}</h2>
                <span class="muted">{t!("On import, Fin categorises transactions automatically with these rules")}</span>
            </div>
            <form class="rule-add" on:submit=add>
                <label>{t!("Category")}
                    <ComboBox
                        label=t!("Category")
                        placeholder=t!("Choose…")
                        options=Signal::derive(move || ctx.data.with(|d| category_choices(d, None)))
                        value=category
                        on_change=move |v| category.set(v)
                    />
                </label>
                <label>{t!("Kind")}
                    <ComboBox
                        label=t!("Kind")
                        options=vec![
                            ComboOption::new("text", t!("Description contains")),
                            ComboOption::new("in", t!("Description contains, money in")),
                            ComboOption::new("out", t!("Description contains, money out")),
                            ComboOption::new("iban", t!("Counterparty IBAN")),
                        ]
                        value=Signal::derive(move || match kind.get() {
                            RuleKind::Iban => "iban",
                            RuleKind::TextIn => "in",
                            RuleKind::TextOut => "out",
                            RuleKind::Text => "text",
                        }.to_string())
                        on_change=move |v: String| kind.set(match v.as_str() {
                            "iban" => RuleKind::Iban,
                            "in" => RuleKind::TextIn,
                            "out" => RuleKind::TextOut,
                            _ => RuleKind::Text,
                        })
                    />
                </label>
                <label>{t!("Value")}
                    <input placeholder=move || if kind.get() == RuleKind::Iban { "NL00BANK0123456789" } else { t!("e.g. albert heijn") }
                        prop:value=move || value.get() on:input=move |ev| value.set(event_target_value(&ev))/>
                </label>
                <button type="submit" class="dark">{t!("Add")}</button>
            </form>
            <input type="search" class="search" placeholder=t!("Search by text, IBAN or category") aria-label=t!("Search rules")
                prop:value=move || query.get() on:input=move |ev| query.set(event_target_value(&ev))/>
            <h3>{t!("Your rules")}" "<span class="count">{move || visible(false).len()}</span></h3>
            <div class="rule-list">
                {move || {
                    let own = visible(false);
                    if own.is_empty() {
                        view! { <p class="empty">{t!("No rules of your own yet. Make one with the lightning icon on a transaction, or above.")}</p> }.into_any()
                    } else {
                        own.into_iter().map(row).collect_view().into_any()
                    }
                }}
            </div>
            <details class="defaults">
                <summary>
                    <h3>{t!("Default rules")}" "<span class="count">{move || visible(true).len()}</span></h3>
                    <span class="muted">{t!("These come with Fin and are fixed. Add your own rule to override one.")}</span>
                </summary>
                <div class="rule-list">{move || visible(true).into_iter().map(row).collect_view()}</div>
            </details>
        </section>
    }
}

/// Split a transaction: parts (amount, category) that count elsewhere, such as the
/// holiday pay in the May salary; the rest stays in the transaction's category.
#[component]
fn SplitForm(tx: Transaction, open: RwSignal<Option<String>>) -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let tx = StoredValue::new(tx);
    let amount_text = |c: i64| i18n::cents(c.abs());
    // Editable parts as (amount as typed, category id); starts with the current split.
    let parts = RwSignal::new(tx.with_value(|t| {
        let mut p: Vec<(String, String)> = t.splits.iter().map(|s| (amount_text(s.amount_cents), s.category_id.clone())).collect();
        if p.is_empty() {
            p.push((String::new(), String::new()));
        }
        p
    }));
    let total = tx.with_value(|t| t.amount_cents.abs());
    let parsed = move || parts.with(|p| p.iter().map(|(a, _)| parse_amount(a).map(i64::abs).unwrap_or(0)).sum::<i64>());
    let rest_name = tx.with_value(|t| ctx.data.with_untracked(|d| d.category(t.category_id.as_deref()).map(|c| c.name.clone()).unwrap_or_default()));
    let save = move |parts_out: Vec<(i64, String)>| {
        let id = tx.with_value(|t| t.id.clone());
        spawn_local(async move {
            match api::set_splits(&id, parts_out).await {
                Ok(d) => {
                    ctx.data.set(d);
                    ctx.error.set(None);
                    open.set(None);
                }
                Err(e) => ctx.error.set(Some(e)),
            }
        });
    };
    let submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let mut out = Vec::new();
        for (a, c) in parts.get_untracked() {
            if a.trim().is_empty() && c.is_empty() {
                continue;
            }
            let Some(cents) = parse_amount(&a).map(i64::abs).filter(|c| *c > 0) else {
                return ctx.error.set(Some(t!("Invalid amount: {}", a)));
            };
            if c.is_empty() {
                return ctx.error.set(Some(t!("Choose a category for each part").into()));
            }
            out.push((cents, c));
        }
        save(out);
    };
    view! {
        <form class="rule-form split-form" on:submit=submit>
            <strong>{t!("Split")}</strong>
            {move || parts.get().into_iter().enumerate().map(|(i, (a, c))| view! {
                <div class="split-row">
                    <input class="right" placeholder=t!("Amount") aria-label=t!("Amount") prop:value=a
                        on:input=move |ev| parts.update(|p| p[i].0 = event_target_value(&ev))/>
                    <ComboBox
                        label=t!("Category")
                        placeholder=t!("Category…")
                        options=ctx.data.with_untracked(|d| category_choices(d, Some(c.as_str()).filter(|c| !c.is_empty())))
                        value=Signal::derive(move || parts.with(|p| p.get(i).map(|x| x.1.clone()).unwrap_or_default()))
                        on_change=move |v| parts.update(|p| p[i].1 = v)
                    />
                    <button type="button" class="icon" aria-label=t!("Remove part") on:click=move |_| parts.update(|p| { p.remove(i); })><CrossIcon/></button>
                </div>
            }).collect_view()}
            <button type="button" class="link" on:click=move |_| parts.update(|p| p.push((String::new(), String::new())))>{t!("+ Add part")}</button>
            <p class="muted rule-preview">{move || {
                let rest = total - parsed();
                if rest < 0 {
                    t!("The parts are € {} more than the amount.", i18n::cents(-rest))
                } else {
                    t!("Rest in {}: € {}", rest_name, i18n::cents(rest))
                }
            }}</p>
            <div class="actions">
                {tx.with_value(|t| !t.splits.is_empty()).then(|| view! {
                    <button type="button" on:click=move |_| save(Vec::new())>{t!("Remove split")}</button>
                })}
                <button type="button" on:click=move |_| open.set(None)>{t!("Cancel")}</button>
                <button type="submit" class="primary">{t!("Save")}</button>
            </div>
        </form>
    }
}

#[component]
fn SplitIcon() -> impl IntoView {
    view! {
        <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M2 8h5l4-5h3M7 8l4 5h3M12 1.5 14 3l-2 1.5M12 11.5 14 13l-2 1.5"></path>
        </svg>
    }
}

/// The date a transaction counts on. Click to set another day (a reversal in September
/// for an August payment counts in August); the bank date stays and shows beneath.
#[component]
fn DateCell(tx: Transaction, date_editing: RwSignal<Option<String>>) -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let tx = StoredValue::new(tx);
    let id = tx.with_value(|t| t.id.clone());
    let open = move || date_editing.with(|e| e.as_deref() == Some(id.as_str()));
    let value = RwSignal::new(tx.with_value(|t| t.day().to_string()));
    let cancel = move || date_editing.set(None);
    let set = move |day: Option<String>| {
        let id = tx.with_value(|t| t.id.clone());
        cancel();
        apply(ctx, async move { api::set_counts_on(&id, day.as_deref()).await });
    };
    let moved = tx.with_value(|t| t.counts_on.is_some());
    view! {
        <Show
            when=open
            fallback=move || view! {
                <button
                    class="date-cell"
                    class:moved=moved
                    title=move || if moved {
                        t!("Counts on {}; bank date {}. Click to change.", tx.with_value(|t| t.day().to_string()), tx.with_value(|t| t.date.clone()))
                    } else {
                        t!("Click to count this transaction on another day").to_string()
                    }
                    on:click=move |_| {
                        value.set(tx.with_value(|t| t.day().to_string()));
                        date_editing.set(Some(tx.with_value(|t| t.id.clone())));
                    }
                >
                    <span>{tx.with_value(|t| t.day().to_string())}</span>
                    {moved.then(|| view! { <small>{t!("bank")}" "{tx.with_value(|t| t.date.clone())}</small> })}
                </button>
            }
        >
            <div class="date-edit" on:keydown=move |ev| match ev.key().as_str() {
                "Escape" => cancel(),
                "Enter" => set(Some(value.get_untracked())),
                _ => {}
            }>
                <input type="date" aria-label=t!("Counts on") prop:value=move || value.get() on:input=move |ev| value.set(event_target_value(&ev))/>
                <div class="date-actions">
                    <button class="icon save" aria-label=t!("Save") title=t!("Save") on:click=move |_| set(Some(value.get_untracked()))><CheckIcon/></button>
                    {moved.then(|| view! {
                        <button class="link" title=t!("Back to the bank date") on:click=move |_| set(None)>{t!("Bank date")}</button>
                    })}
                    <button class="icon" aria-label=t!("Cancel") title=t!("Cancel") on:click=move |_| cancel()><CrossIcon/></button>
                </div>
            </div>
        </Show>
    }
}

/// A transaction's category as text; the pencil opens a picker that only takes effect
/// with the save button (Escape or the cross cancels).
#[component]
fn CategoryCell(tx: Transaction, cat_editing: RwSignal<Option<String>>) -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let tx = StoredValue::new(tx);
    let id = tx.with_value(|t| t.id.clone());
    let open = move || cat_editing.with(|e| e.as_deref() == Some(id.as_str()));
    let choice = RwSignal::new(tx.with_value(|t| t.category_id.clone().unwrap_or_default()));
    let name = move || ctx.data.with(|d| tx.with_value(|t| d.category(t.category_id.as_deref()).map(|c| c.name.clone()).unwrap_or_default()));
    let start = move |_| {
        choice.set(tx.with_value(|t| t.category_id.clone().unwrap_or_default()));
        cat_editing.set(Some(tx.with_value(|t| t.id.clone())));
    };
    let cancel = move || cat_editing.set(None);
    let save = move || {
        let t = tx.get_value();
        let cat = choice.get_untracked();
        if Some(cat.as_str()) == t.category_id.as_deref() {
            return cancel();
        }
        let input = TransactionInput {
            id: Some(t.id),
            date: t.date,
            description: t.description,
            amount_cents: t.amount_cents,
            category_id: Some(cat).filter(|c| !c.is_empty()),
            account_id: t.account_id,
        };
        cancel();
        apply(ctx, api::save_transaction(input));
    };
    view! {
        <Show
            when=open
            fallback=move || view! {
                <div class="cat-cell">
                    <span class="cat-name">{name}</span>
                    <button class="icon edit" aria-label=t!("Change category") title=t!("Change category") on:click=start>
                        <PencilIcon/>
                    </button>
                </div>
            }
        >
            <div class="cat-cell editing" on:keydown=move |ev| match ev.key().as_str() {
                "Escape" => cancel(),
                "Enter" => save(),
                _ => {}
            }>
                <ComboBox
                    label=t!("Category")
                    autofocus=true
                    options=ctx.data.with_untracked(|d| category_choices_or_none(d, Some(choice.get_untracked().as_str()).filter(|c| !c.is_empty())))
                    value=choice
                    on_change=move |v| choice.set(v)
                />
                <button class="icon save" aria-label=t!("Save") title=t!("Save") on:click=move |_| save()><CheckIcon/></button>
                <button class="icon" aria-label=t!("Cancel") title=t!("Cancel") on:click=move |_| cancel()><CrossIcon/></button>
            </div>
        </Show>
    }
}

/// A transaction's category for colors and icons; None while still to sort.
fn tx_category<'a>(d: &'a Dataset, t: &Transaction) -> Option<&'a Category> {
    if t.is_unsorted() { None } else { d.category(t.category_id.as_deref()) }
}

fn tone_class(d: &Dataset, t: &Transaction) -> String {
    cat_tone(tx_category(d, t))
}

fn type_badge(d: &Dataset, t: &Transaction) -> impl IntoView {
    cat_badge(tx_category(d, t))
}

/// A category's color class (styles.css gives each a hue): its group, "transfer",
/// "requests", "unsorted" (also for None), or for the user's own categories one of
/// eight fixed hues picked from the id, so a category keeps its color everywhere.
/// The standard group a category belongs to: its own for a standard category, and for
/// one of the user's the group of the standard categories listed under the same
/// heading, so it takes that group's color and icon.
fn group_key(c: &Category) -> Option<&'static str> {
    fin_shared::catalog::group_of(&c.id).or_else(|| {
        use_context::<Ctx>()?.data.with_untracked(|d| {
            d.categories.iter().filter(|x| x.group == c.group).find_map(|x| fin_shared::catalog::group_of(&x.id))
        })
    })
}

fn cat_tone(cat: Option<&Category>) -> String {
    match cat {
        None => "t-unsorted".into(),
        Some(c) if c.id == UNSORTED_CATEGORY_ID => "t-unsorted".into(),
        Some(c) if c.kind == CategoryKind::Transfer => "t-transfer".into(),
        Some(c) if c.id == fin_shared::catalog::ids::PAYMENT_REQUESTS => "t-requests".into(),
        Some(c) => match group_key(c) {
            Some(g) => format!("t-{g}"),
            None => format!("t-own{}", c.id.bytes().fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32)) % 8),
        },
    }
}

/// Icon per kind of category: its group, with transfers, payment requests and
/// still-to-sort their own. Takes its color from the surrounding tone class.
fn cat_badge(cat: Option<&Category>) -> impl IntoView {
    use fin_shared::catalog::{self, groups as g};
    let cat = cat.filter(|c| c.id != UNSORTED_CATEGORY_ID);
    let title = match cat {
        None => t!("To categorise").to_string(),
        Some(c) => format!("{} · {}", c.group, kind_label(c.kind)),
    };
    let group = cat.and_then(group_key);
    let d = match (cat.map(|c| c.kind), group) {
        (None, _) => "M6 6a2 2 0 1 1 3 1.7c-.7.4-1 .9-1 1.6M8 12h.01",
        (Some(CategoryKind::Transfer), _) => "M2.5 5.5h10.5l-2.5-2.5M13.5 10.5H3l2.5 2.5",
        // Tikkie and payment requests: a message with a euro sign.
        _ if cat.is_some_and(|c| c.id == catalog::ids::PAYMENT_REQUESTS) => {
            "M3 2.5h10a1 1 0 0 1 1 1v7a1 1 0 0 1-1 1H7.5L4.5 14v-2.5H3a1 1 0 0 1-1-1v-7a1 1 0 0 1 1-1zM9.8 5.2A2 2 0 1 0 9.8 8.8M5.8 6.4h3M5.8 7.6h3"
        }
        (_, Some(g::INCOME)) | (Some(CategoryKind::Income), None) => "M8 2v8M4.5 6.5 8 10l3.5-3.5M2.5 13.5h11",
        (_, Some(g::HOUSING)) => "M2 7.5 8 2.5l6 5M3.5 6.5v7h9v-7M6.5 13.5v-4h3v4",
        (_, Some(g::HOUSEHOLD)) => "M1.5 2.5h2l1.5 8h7.5l1.5-5.5H4.3M6 13.5h.01M12 13.5h.01",
        (_, Some(g::MEDICAL)) => "M6.2 2.5h3.6v3.7h3.7v3.6H9.8v3.7H6.2V9.8H2.5V6.2h3.7z",
        (_, Some(g::INSURANCE)) => "M8 1.8 13.5 4v4c0 3.2-2.4 5.4-5.5 6.2C4.9 13.4 2.5 11.2 2.5 8V4z",
        (_, Some(g::TELECOM)) => "M5 1.5h6a1 1 0 0 1 1 1v11a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1v-11a1 1 0 0 1 1-1zM7 12.5h2",
        (_, Some(g::SUBSCRIPTIONS)) => "M2.5 7V5.5a2 2 0 0 1 2-2h8.5l-2-2M13.5 9v1.5a2 2 0 0 1-2 2H3l2 2",
        (_, Some(g::TRANSPORT)) => "M2.5 11.5v-3l1.5-4h8l1.5 4v3zM2.5 11.5V13M13.5 11.5V13M5 9h.01M11 9h.01",
        (_, Some(g::EDUCATION)) => "M1.5 6 8 3l6.5 3L8 9zM4 7.2v3.5c1 1 2.5 1.5 4 1.5s3-.5 4-1.5V7.2",
        (_, Some(g::CLOTHING)) => "M5.5 2 2 4.5l1.5 2.5L5 6.2V14h6V6.2l1.5.8L14 4.5 10.5 2a2.5 2.5 0 0 1-5 0z",
        (_, Some(g::LEISURE)) => "M8 1.8l1.9 3.9 4.3.6-3.1 3 .7 4.3L8 11.6l-3.8 2 .7-4.3-3.1-3 4.3-.6z",
        (_, Some(g::OTHER_EXPENSES)) => "M3.5 8h.01M8 8h.01M12.5 8h.01",
        // The user's own categories: a tag.
        _ => "M2 2h5.6L14 8.4 8.4 14 2 7.6zM5.2 5.2h.01",
    };
    view! {
        <span class=format!("type-badge {}", cat_tone(cat)) title=title>
            <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <path d=d></path>
            </svg>
        </span>
    }
}

#[component]
fn PencilIcon() -> impl IntoView {
    view! {
        <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round" aria-hidden="true">
            <path d="M10.5 2.5 13.5 5.5 5.5 13.5H2.5V10.5z"></path>
        </svg>
    }
}

#[component]
fn CheckIcon() -> impl IntoView {
    view! {
        <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M3 8.5 6.5 12 13 4.5"></path>
        </svg>
    }
}

#[component]
fn CrossIcon() -> impl IntoView {
    view! {
        <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" aria-hidden="true">
            <path d="M4 4l8 8M12 4l-8 8"></path>
        </svg>
    }
}

/// Rules are automations: a lightning bolt. Filled where a rule did the work.
#[component]
fn BoltIcon(filled: bool) -> impl IntoView {
    view! {
        <svg width="14" height="14" viewBox="0 0 16 16" fill={if filled { "currentColor" } else { "none" }} stroke="currentColor" stroke-width="1.4" stroke-linejoin="round" aria-hidden="true">
            <path d="M9.2 1.5 3.5 9h4l-.7 5.5L12.5 7h-4z"></path>
        </svg>
    }
}

#[component]
fn LockIcon() -> impl IntoView {
    view! {
        <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" aria-hidden="true">
            <rect x="3.5" y="7" width="9" height="7" rx="1.5"></rect>
            <path d="M5.5 7V5a2.5 2.5 0 0 1 5 0v2"></path>
        </svg>
    }
}

fn rule_kind_label(k: RuleKind) -> &'static str {
    match k {
        RuleKind::Iban => "IBAN",
        RuleKind::Text => t!("Text"),
        RuleKind::TextIn => t!("Text, money in"),
        RuleKind::TextOut => t!("Text, money out"),
    }
}

#[component]
fn TrashIcon() -> impl IntoView {
    view! {
        <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" aria-hidden="true">
            <path d="M3 4h10M6 4V2.5h4V4M5 4l.5 9h5l.5-9"></path>
        </svg>
    }
}

#[derive(Clone, Copy, PartialEq)]
enum FileState {
    Done,
    Busy(f64),
    Waiting,
}

#[component]
fn ImportDialog(open: RwSignal<bool>) -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    // Focus the dialog so Escape works right away.
    let dialog_ref = NodeRef::<leptos::html::Div>::new();
    Effect::new(move |_| {
        if let Some(el) = dialog_ref.get() {
            let _ = el.focus();
        }
    });
    let account = RwSignal::new(
        ctx.data.with_untracked(|d| d.accounts.first().map(|a| a.id.clone())).unwrap_or_default(),
    );
    let files = RwSignal::new(Vec::<ImportFile>::new());
    let reading = RwSignal::new(false);
    let busy = RwSignal::new(false);
    let progress = RwSignal::new(None::<ImportProgress>);
    let result = RwSignal::new(None::<String>);

    let pick = move |ev: leptos::ev::Event| {
        let Some(input) = ev.target().and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok()) else { return };
        let Some(list) = input.files() else { return };
        let picked: Vec<web_sys::File> = (0..list.length()).filter_map(|i| list.get(i)).collect();
        result.set(None);
        progress.set(None);
        reading.set(true);
        spawn_local(async move {
            let mut out = Vec::new();
            for f in picked {
                match JsFuture::from(f.text()).await {
                    Ok(v) => out.push(ImportFile { name: f.name(), xml: v.as_string().unwrap_or_default() }),
                    Err(_) => ctx.error.set(Some(t!("Cannot read {}", f.name()))),
                }
            }
            files.set(out);
            reading.set(false);
        });
    };

    let run = move |_| {
        let chosen = files.get_untracked();
        if chosen.is_empty() || account.get_untracked().is_empty() {
            return;
        }
        busy.set(true);
        result.set(None);
        let account_id = account.get_untracked();
        spawn_local(async move {
            let r = api::import_camt053(&account_id, &chosen, move |p| progress.set(Some(p))).await;
            busy.set(false);
            match r {
                Ok(r) => {
                    ctx.data.set(r.data);
                    ctx.error.set(None);
                    files.set(Vec::new());
                    // Show the imported period rather than leaving the list on another month.
                    if let Some(d) = &r.latest_date {
                        ctx.month.set(d[..7].to_string());
                    }
                    result.set(Some(tn!(
                        r.imported,
                        "{} transaction imported ({} categorised by rules), {} duplicates skipped.",
                        "{} transactions imported ({} categorised by rules), {} duplicates skipped.",
                        r.imported,
                        r.classified,
                        r.skipped_duplicates
                    )));
                }
                Err(e) => {
                    progress.set(None);
                    result.set(Some(t!("Import failed: {}", i18n::error(&e))));
                }
            }
        });
    };

    let file_state = move |i: usize| match progress.get() {
        None => FileState::Waiting,
        Some(p) if i < p.file_index => FileState::Done,
        Some(p) if i == p.file_index => FileState::Busy(p.fraction),
        Some(_) => FileState::Waiting,
    };
    let overall = move || progress.get().map(|p| p.overall()).unwrap_or(0.0);
    let status = move || match progress.get() {
        Some(p) if p.file_index >= p.file_count => t!("Saving…").to_string(),
        Some(p) => t!("Processing file {} of {}…", p.file_index + 1, p.file_count),
        None => String::new(),
    };

    view! {
        <div class="backdrop" on:keydown=move |ev: leptos::ev::KeyboardEvent| {
            if ev.key() == "Escape" && !busy.get_untracked() {
                open.set(false);
            }
        }>
            <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="imp-title" tabindex="-1" node_ref=dialog_ref>
                <h2 id="imp-title">{t!("Import statements")}</h2>
                <label class="stack">{t!("To account")}
                    <ComboBox
                        label=t!("To account")
                        disabled=Signal::derive(move || busy.get())
                        options=ctx.data.with_untracked(|d| d.accounts.iter().map(|a| {
                            let label = match &a.iban {
                                Some(i) => format!("{} · {i}", a.name),
                                None => a.name.clone(),
                            };
                            ComboOption::new(a.id.clone(), label)
                        }).collect::<Vec<_>>())
                        value=account
                        on_change=move |v| account.set(v)
                    />
                </label>
                <label class="dropzone" class:disabled=move || busy.get()>
                    <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" aria-hidden="true">
                        <path d="M12 16V4M7 9l5-5 5 5M4 16v4h16v-4"></path>
                    </svg>
                    <span><strong>{t!("Choose CAMT.053 files")}</strong>" "{t!("(.xml, several at once)")}</span>
                    <input type="file" accept=".xml,application/xml,text/xml" multiple disabled=move || busy.get() on:change=pick/>
                </label>
                <Show when=move || reading.get()><p class="muted">{t!("Reading files…")}</p></Show>
                <ul class="files">
                    {move || files.get().into_iter().enumerate().map(|(i, f)| {
                        let kb = f.xml.len().div_ceil(1024);
                        view! {
                            <li class:current=move || matches!(file_state(i), FileState::Busy(_))>
                                <span>{f.name.clone()}</span>
                                <span class="muted">{move || match file_state(i) {
                                    FileState::Done => t!("Done").to_string(),
                                    FileState::Busy(x) => format!("{:.0}%", x * 100.0),
                                    FileState::Waiting => format!("{kb} kB"),
                                }}</span>
                            </li>
                        }
                    }).collect_view()}
                </ul>
                <Show when=move || busy.get() || progress.get().is_some()>
                    <div class="progress-wrap">
                        <div
                            class="progress"
                            role="progressbar"
                            aria-label=t!("Import progress")
                            aria-valuemin="0"
                            aria-valuemax="100"
                            aria-valuenow=move || format!("{:.0}", overall() * 100.0)
                        >
                            <div class="progress-fill" style:width=move || format!("{:.1}%", overall() * 100.0)></div>
                        </div>
                        <div class="muted">{move || if busy.get() { status() } else { t!("Done").into() }}</div>
                    </div>
                </Show>
                {move || result.get().map(|r| view! { <p class="result">{r}</p> })}
                <div class="actions">
                    <button disabled=move || busy.get() on:click=move |_| open.set(false)>{t!("Close")}</button>
                    <button class="primary" disabled=move || busy.get() || reading.get() || files.with(|f| f.is_empty()) on:click=run>
                        {t!("Import")}
                    </button>
                </div>
            </div>
        </div>
    }
}

/// The budget grid's blocks: categories that can get a budget, per group split by kind
/// (income, fixed, variable). Disabled categories, the transfer category (internal
/// transfers) and the unsorted one are left out.
fn budget_groups(ds: &Dataset) -> Vec<(String, Vec<Category>)> {
    // Income first, then fixed costs, then variable spending; groups in list order
    // within each.
    let groups = grouped(ds);
    [CategoryKind::Income, CategoryKind::Fixed, CategoryKind::Variable]
        .into_iter()
        .flat_map(|k| {
            groups.iter().map(move |(g, cats)| {
                let budgeted = |c: &&Category| c.budgetable() && c.kind == k && c.id != UNSORTED_CATEGORY_ID;
                (g.clone(), cats.iter().filter(budgeted).cloned().collect::<Vec<_>>())
            })
        })
        .filter(|(_, cats)| !cats.is_empty())
        .collect()
}

/// The budget: the grid where budgets are set up, per month for a year. How the month
/// and the year are going is on the Overview.
#[component]
fn BudgetPage() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let year = RwSignal::new(ctx.month.get_untracked()[..4].parse::<i32>().unwrap_or(2026));
    let notice = RwSignal::new(None::<String>);

    // "From average": the last full months before this one.
    let avg_open = RwSignal::new(false);
    let avg_months = RwSignal::new(12u32);
    let avg_overwrite = RwSignal::new(false);
    let avg_last = StoredValue::new(shift_month(&today()[..7], -1));
    let avg_proposal = Memo::new(move |_| {
        let months = months_ending(&avg_last.get_value(), avg_months.get() as usize);
        ctx.data.with(|d| d.average_per_month(&months))
    });
    let avg_preview = move || {
        let (count, income, expense) = ctx.data.with(|d| {
            avg_proposal.with(|p| {
                let income: i64 = p
                    .iter()
                    .filter(|(id, _)| d.category(Some(id)).is_some_and(|c| c.kind == CategoryKind::Income))
                    .map(|(_, v)| v)
                    .sum();
                let total: i64 = p.iter().map(|(_, v)| v).sum();
                // Budgets, not categories: variable categories land on their group's.
                let mut budgets: Vec<String> = p
                    .iter()
                    .filter_map(|(id, _)| d.category(Some(id)))
                    .map(|c| if c.kind == CategoryKind::Variable { format!("group:{}", c.group) } else { c.id.clone() })
                    .collect();
                budgets.sort();
                budgets.dedup();
                (budgets.len(), income, total - income)
            })
        });
        if count == 0 {
            return t!("No transactions in this period to take an average from.").to_string();
        }
        tn!(
            count,
            "{} budget gets its average every month (variable categories per group): together € {} spending and € {} income per month, rounded to whole euros.",
            "{} budgets get their average every month (variable categories per group): together € {} spending and € {} income per month, rounded to whole euros.",
            count,
            budget_text(expense),
            budget_text(income)
        )
    };
    let run_average = {
        move |_| {
            let (y, n, overwrite, last) =
                (year.get_untracked(), avg_months.get_untracked(), avg_overwrite.get_untracked(), avg_last.get_value());
            spawn_local(async move {
                match api::budgets_from_average(y, &last, n, overwrite).await {
                    Ok(r) => {
                        ctx.data.set(r.data);
                        ctx.error.set(None);
                        avg_open.set(false);
                        notice.set(Some(tn!(
                            r.copied,
                            "{} budget filled in from the average of {} months.",
                            "{} budgets filled in from the average of {} months.",
                            r.copied,
                            n
                        )));
                    }
                    Err(e) => ctx.error.set(Some(e)),
                }
            });
        }
    };
    let (this_year, this_idx): (i32, usize) = {
        let t = today();
        (t[..4].parse().unwrap_or(2026), t[5..7].parse::<usize>().unwrap_or(1) - 1)
    };
    let month_at = |y: i32, i: usize| format!("{y:04}-{:02}", i + 1);
    let is_current = move |i: usize| year.get() == this_year && i == this_idx;

    let groups = Memo::new(move |_| ctx.data.with(budget_groups));
    // Budgeted per kind per month (switched-on categories): income, fixed, variable,
    // investments. The summary rows and the result come from these. Income and fixed
    // costs per category, variable costs per group.
    let totals = Memo::new(move |_| {
        let prefix = format!("{:04}-", year.get());
        ctx.data.with(|d| {
            let mut t = [[0i64; 12]; 4];
            let month_index = |month: &str| month[5..].parse::<usize>().ok().filter(|m| (1..=12).contains(m)).map(|m| m - 1);
            for g in d.group_budgets.iter().filter(|g| g.month.starts_with(&prefix) && d.group_budget_counts(g)) {
                if let Some(i) = month_index(&g.month) {
                    t[2][i] += g.amount_cents;
                }
            }
            for b in d.budgets.iter().filter(|b| b.month.starts_with(&prefix)) {
                let Some(c) = d.category(Some(&b.category_id)) else { continue };
                if d.own_budget(c, &b.month).is_none() {
                    continue;
                }
                let row = match c.kind {
                    _ if !c.budgetable() => continue,
                    CategoryKind::Income => 0,
                    CategoryKind::Fixed => 1,
                    CategoryKind::Investment => 3,
                    CategoryKind::Variable | CategoryKind::IrregularIncome | CategoryKind::Transfer => continue,
                };
                if let Ok(m @ 1..=12) = b.month[5..].parse::<usize>() {
                    t[row][m - 1] += b.amount_cents;
                }
            }
            t
        })
    });
    let summary_row = move |label: &'static str, value: fn(&[[i64; 12]; 4], usize) -> i64, class: &'static str| {
        view! {
            <div class=format!("budget-row total {class}")>
                <span>{label}</span>
                {(0..12).map(|i| view! {
                    <span class="right" class:current=move || is_current(i) class:over=move || class == "result" && totals.with(|t| value(t, i)) < 0>
                        {move || euro(totals.with(|t| value(t, i)))}
                    </span>
                }).collect_view()}
            </div>
        }
    };
    let has_investments = move || totals.with(|t| t[3].iter().any(|v| *v != 0));

    let shift_year = move |delta: i32| {
        notice.set(None);
        year.update(|y| *y += delta);
    };

    let copy_prev = move |_| {
        let y = year.get_untracked();
        spawn_local(async move {
            match api::copy_budgets_from_previous_year(y).await {
                Ok(r) => {
                    ctx.data.set(r.data);
                    ctx.error.set(None);
                    notice.set(Some(match r.copied {
                        0 => t!("Nothing to copy from {}.", y - 1),
                        n => tn!(n, "{} budget copied from {}.", "{} budgets copied from {}.", n, y - 1),
                    }));
                }
                Err(e) => ctx.error.set(Some(e)),
            }
        });
    };

    let copy_to_next = move |i: usize| {
        let month = month_at(year.get_untracked(), i);
        spawn_local(async move {
            match api::copy_budget_month_to_next(&month).await {
                Ok(d) => {
                    ctx.data.set(d);
                    ctx.error.set(None);
                    notice.set(Some(t!("{} copied to {}.", month_label(&month), month_label(&shift_month(&month, 1)))));
                }
                Err(e) => ctx.error.set(Some(e)),
            }
        });
    };

    let cell = move |c: &Category, i: usize| {
        let (id, id_change) = (c.id.clone(), c.id.clone());
        let label = format!("{} {}", c.name, i18n::month_short(i));
        view! {
            <input
                class="budget-cell"
                class:current=move || is_current(i)
                inputmode="decimal"
                placeholder="–"
                aria-label=label
                prop:value=move || {
                    let month = month_at(year.get(), i);
                    ctx.data.with(|d| d.budget_for(&id, &month)).map(budget_text).unwrap_or_default()
                }
                on:change=move |ev| {
                    let month = month_at(year.get_untracked(), i);
                    let text = event_target_value(&ev);
                    let amount = match text.trim() {
                        "" | "–" | "-" => None,
                        t => match parse_amount(t) {
                            Some(cents) => Some(cents.abs()),
                            None => {
                                let old = ctx.data.with_untracked(|d| d.budget_for(&id_change, &month));
                                event_target::<web_sys::HtmlInputElement>(&ev)
                                    .set_value(&old.map(budget_text).unwrap_or_default());
                                ctx.error.set(Some(t!("Invalid amount: {}", t)));
                                return;
                            }
                        },
                    };
                    apply(ctx, api::set_budget(id_change.clone(), month, amount));
                }
            />
        }
    };

    // A group's budget for its variable categories together (`names` in the tooltip).
    let group_cell = move |group: String, names: String, i: usize| {
        let kind = CategoryKind::Variable;
        let (g, g_change) = (group.clone(), group.clone());
        let label = format!("{} {} {}", group, kind_label(kind), i18n::month_short(i));
        view! {
            <input
                class="budget-cell"
                class:current=move || is_current(i)
                inputmode="decimal"
                placeholder="–"
                aria-label=label
                title=names
                prop:value=move || {
                    let month = month_at(year.get(), i);
                    ctx.data.with(|d| d.group_budget_for(&g, kind, &month)).map(budget_text).unwrap_or_default()
                }
                on:change=move |ev| {
                    let month = month_at(year.get_untracked(), i);
                    let text = event_target_value(&ev);
                    let amount = match text.trim() {
                        "" | "–" | "-" => None,
                        t => match parse_amount(t) {
                            Some(cents) => Some(cents.abs()),
                            None => {
                                let old = ctx.data.with_untracked(|d| d.group_budget_for(&g_change, kind, &month));
                                event_target::<web_sys::HtmlInputElement>(&ev)
                                    .set_value(&old.map(budget_text).unwrap_or_default());
                                ctx.error.set(Some(t!("Invalid amount: {}", t)));
                                return;
                            }
                        },
                    };
                    apply(ctx, api::set_group_budget(g_change.clone(), kind, month, amount));
                }
            />
        }
    };

    view! {
        <header class="page-head">
            <button class="round" aria-label=t!("Previous year") on:click=move |_| shift_year(-1)>"‹"</button>
            <h1 class="year">{t!("Budget")}" "{move || year.get()}</h1>
            <button class="round" aria-label=t!("Next year") on:click=move |_| shift_year(1)>"›"</button>
            <div class="spacer"></div>
                <button
                    title=t!("Calculate the budget from the average of recent months")
                    aria-expanded=move || avg_open.get().to_string()
                    on:click=move |_| avg_open.update(|v| *v = !*v)
                >
                    {t!("From average…")}
                </button>
                <button title=move || t!("Take over the budgets of {} where {} is still empty", year.get() - 1, year.get()) on:click=copy_prev>
                    {move || t!("Copy {}", year.get() - 1)}
                </button>
                <Show when=move || year.get() == this_year>
                    <button
                        class="primary"
                        title=t!("Copy the budget of {} to {}", i18n::month_name(this_idx), i18n::month_name((this_idx + 1) % 12))
                        on:click=move |_| copy_to_next(this_idx)
                    >
                        {format!("{} → {}", capitalize(i18n::month_short(this_idx)), i18n::month_short((this_idx + 1) % 12))}
                    </button>
                </Show>
        </header>
        <Show when=move || avg_open.get()>
            <div class="panel avg-panel">
                <label class="inline">{t!("Average over the last")}
                    <ComboBox
                        label=t!("Average over the last")
                        options={[3u32, 6, 12].into_iter().map(|n| ComboOption::new(n.to_string(), t!("{} months", n))).collect::<Vec<_>>()}
                        value=Signal::derive(move || avg_months.get().to_string())
                        on_change=move |v: String| avg_months.set(v.parse().unwrap_or(12))
                    />
                </label>
                <span class="muted">{move || t!("through {}", avg_last.with_value(|m| month_label(m)))}</span>
                <label class="check"><input type="radio" name="avg-mode" checked=move || !avg_overwrite.get() on:change=move |_| avg_overwrite.set(false)/>{t!("Only empty months")}</label>
                <label class="check"><input type="radio" name="avg-mode" checked=move || avg_overwrite.get() on:change=move |_| avg_overwrite.set(true)/>{t!("Overwrite everything")}</label>
                <p class="avg-preview">{avg_preview}</p>
                <div class="actions">
                    <button on:click=move |_| avg_open.set(false)>{t!("Cancel")}</button>
                    <button class="primary" disabled=move || avg_proposal.with(Vec::is_empty) on:click=run_average>
                        {move || t!("Fill in budget {}", year.get())}
                    </button>
                </div>
            </div>
        </Show>
        <p class="hint">
            {move || notice.get().unwrap_or_else(|| {
                t!("Amounts per month. Click a month heading to copy that month to the next; empty cells have no budget.").into()
            })}
        </p>
        <section class="panel budget">
            <div class="budget-row head">
                <span>{t!("Category")}</span>
                {(0..12).map(|i| view! {
                    <button
                        class="month-head"
                        class:current=move || is_current(i)
                        title=t!("Copy {} to {}", capitalize(i18n::month_name(i)), i18n::month_name((i + 1) % 12))
                        on:click=move |_| copy_to_next(i)
                    >
                        {i18n::month_short(i)}
                    </button>
                }).collect_view()}
            </div>
            <div class="budget-summary">
                {summary_row(t!("Fixed income"), |t, i| t[0][i], "income")}
                {summary_row(t!("Fixed costs"), |t, i| -t[1][i], "")}
                {summary_row(t!("Variable"), |t, i| -t[2][i], "")}
                <Show when=has_investments>{summary_row(t!("Investments"), |t, i| -t[3][i], "")}</Show>
                {summary_row(t!("Result"), |t, i| t[0][i] - t[1][i] - t[2][i] - t[3][i], "result")}
            </div>
            <For
                each=move || groups.get()
                key=|g| g.clone()
                children=move |(group, cats): (String, Vec<Category>)| {
                    // Every block holds one kind of one group (see budget_groups). Variable
                    // costs are budgeted per group: their block is the group budget's row
                    // alone. Income and fixed costs are budgeted per category.
                    let kind = cats.first().map(|c| c.kind);
                    let rows = if kind == Some(CategoryKind::Variable) {
                        let n = cats.len();
                        let names = cats.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(", ");
                        view! {
                            <div class=format!("budget-row group-budget {}", cat_tone(cats.first()))>
                                <span class="budget-name" title=format!("{}: {names}", tn!(n, "{} category", "{} categories", n))>
                                    {t!("Group budget")}
                                </span>
                                {(0..12).map(|i| group_cell(group.clone(), names.clone(), i)).collect_view()}
                            </div>
                        }
                        .into_any()
                    } else {
                        cats.iter()
                            .map(|c| view! {
                                <div class=format!("budget-row tinted {}", cat_tone(Some(c)))>
                                    <span class="budget-name" title=c.name.clone()>{cat_badge(Some(c))}<span>{c.name.clone()}</span></span>
                                    {(0..12).map(|i| cell(c, i)).collect_view()}
                                </div>
                            })
                            .collect_view()
                            .into_any()
                    };
                    let title = if group.is_empty() { t!("No group").to_string() } else { group };
                    view! {
                        <div class="budget-group">
                            {title}
                            {kind.map(|k| view! { <span class="muted">" · "{kind_label(k)}</span> })}
                        </div>
                        {rows}
                    }
                }
            />
        </section>
    }
}

/// An IBAN as typed or pasted, without any whitespace (also non-breaking spaces), upper case.
fn compact_iban(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_uppercase()
}

#[component]
fn Accounts() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let name = RwSignal::new(String::new());
    let iban = RwSignal::new(String::new());

    let submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let n = name.get_untracked();
        let i = Some(iban.get_untracked()).filter(|s| !s.trim().is_empty());
        name.set(String::new());
        iban.set(String::new());
        apply(ctx, async move { api::add_account(&n, i).await });
    };
    view! {
        <header class="page-head"><h1>{t!("Accounts")}</h1></header>
        <form class="inline-form panel" on:submit=submit>
            <label>{t!("Name")}<input required placeholder=t!("Current account") prop:value=move || name.get() on:input=move |ev| name.set(event_target_value(&ev))/></label>
            <label>{t!("IBAN (optional)")}<input placeholder="NL.." prop:value=move || iban.get() on:input=move |ev| iban.set(compact_iban(&event_target_value(&ev)))/></label>
            <button type="submit" class="dark">{t!("Add")}</button>
        </form>
        <section class="panel list">
            <For
                each=move || ctx.data.get().accounts
                key=|a| a.clone()
                children=move |a: Account| {
                    let bal_id = a.id.clone();
                    let bal = Memo::new(move |_| ctx.data.with(|d| d.account_balance(&bal_id)));
                    let (a1, a2, id) = (a.clone(), a.clone(), a.id.clone());
                    view! {
                        <div class="acc-row">
                            <input aria-label=t!("Name") value=a.name.clone() on:change=move |ev| {
                                let mut acc = a1.clone();
                                acc.name = event_target_value(&ev);
                                apply(ctx, api::update_account(acc));
                            }/>
                            <input aria-label="IBAN" placeholder="IBAN" value=a.iban.clone().unwrap_or_default() on:change=move |ev| {
                                let mut acc = a2.clone();
                                acc.iban = Some(compact_iban(&event_target_value(&ev)));
                                apply(ctx, api::update_account(acc));
                            }/>
                            <span class="right amount acc-balance">{move || euro(bal.get())}</span>
                            <button class="icon" aria-label=t!("Delete") on:click=move |_| {
                                let id = id.clone();
                                apply(ctx, async move { api::delete_account(&id).await })
                            }><TrashIcon/></button>
                        </div>
                    }
                }
            />
        </section>
        <YearEndBalances/>
    }
}

/// Year-end balances: per account the bank's balance on 31 December, a row per year
/// (last year, and earlier ones on request). The latest is where the account's
/// balance counts from; each year is checked against the previous year-end plus that
/// year's transactions. Clearing an amount removes it.
#[component]
fn YearEndBalances() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let last_year = today()[..4].parse::<i32>().unwrap_or(2026) - 1;
    let extra = RwSignal::new(0i32);
    // From the earliest year with a balance (or last year) to last year, newest first.
    let years = move || {
        let earliest = ctx.data.with(|d| {
            d.accounts.iter().flat_map(|a| &a.balances).filter_map(|b| b.date[..4].parse::<i32>().ok()).min()
        });
        let from = earliest.unwrap_or(last_year).min(last_year) - extra.get();
        (from..=last_year).rev().collect::<Vec<_>>()
    };
    let save = move |id: String, date: String, text: String| {
        let cents = if text.trim().is_empty() {
            None
        } else {
            match parse_amount(&text) {
                Some(c) => Some(c),
                None => return ctx.error.set(Some(t!("Invalid amount: {}", text))),
            }
        };
        apply(ctx, async move { api::set_account_balance(&id, &date, cents).await });
    };
    view! {
        <section class="panel list">
            <div class="section-head">
                <h2>{t!("Year-end balances")}</h2>
                <span class="muted">{t!("The bank's balance on 31 December")}</span>
            </div>
            {move || {
                let accounts = ctx.data.with(|d| d.accounts.clone());
                let cols = format!("90px repeat({}, minmax(120px, 1fr))", accounts.len().max(1));
                let heads = accounts.iter().map(|a| view! { <span class="ye-head" title=a.name.clone()>{a.name.clone()}</span> }).collect_view();
                let rows = years().into_iter().map(|y| {
                    let date = format!("{y:04}-12-31");
                    let cells = accounts.iter().map(|a| {
                        let value = a.balances.iter().find(|b| b.date == date).map(|b| i18n::cents(b.cents)).unwrap_or_default();
                        let check = ctx.data.with(|d| d.balance_checks(&a.id).into_iter().find(|c| c.date == date));
                        let (id, d2) = (a.id.clone(), date.clone());
                        view! {
                            <div class="ye-cell">
                                <input class="right" aria-label=format!("{} {}", a.name, short_date(&date)) placeholder="–" value=value
                                    on:change=move |ev| save(id.clone(), d2.clone(), event_target_value(&ev))/>
                                {check.map(|c| if c.difference() == 0 {
                                    view! { <small class="ok">{t!("Matches")}</small> }.into_any()
                                } else {
                                    view! { <small class="over" title=t!("Previous year-end plus this year's transactions")>{t!("{} off", euro(c.difference()))}</small> }.into_any()
                                })}
                            </div>
                        }
                    }).collect_view();
                    view! { <span class="ye-year">{short_date(&date)}</span>{cells} }
                }).collect_view();
                view! {
                    <div class="ye-grid" style:grid-template-columns=cols>
                        <span></span>{heads}
                        {rows}
                    </div>
                }
            }}
            <button class="link" on:click=move |_| extra.update(|e| *e += 1)>{t!("+ Earlier year")}</button>
        </section>
    }
}

#[component]
fn Categories() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let name = RwSignal::new(String::new());
    let group = RwSignal::new(String::new());
    let kind = RwSignal::new(CategoryKind::Variable);
    // A group that holds fixed or variable costs sets the kind (see kind_options); a new
    // group name leaves the choice free.
    let locked = move || group.with(|g| ctx.data.with(|d| group_cost_kind(d, g, None)));
    Effect::new(move |_| {
        let g = group.get();
        if let Some(k) = ctx.data.with_untracked(|d| group_cost_kind(d, &g, None)) {
            kind.set(k);
        }
    });
    let submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let (n, g, k) = (name.get_untracked(), group.get_untracked(), kind.get_untracked());
        name.set(String::new());
        apply(ctx, async move { api::add_category(&n, &g, k).await });
    };
    // Which category has its IBAN rules open; lives here so it survives re-renders.
    let rules_open = RwSignal::new(None::<String>);
    let query = RwSignal::new(String::new());
    let visible_groups = move || {
        let q = query.get().trim().to_lowercase();
        ctx.data.with(|d| {
            grouped(d)
                .into_iter()
                .filter_map(|(g, cats)| {
                    if q.is_empty() || g.to_lowercase().contains(&q) {
                        return Some((g, cats));
                    }
                    let cats: Vec<Category> = cats
                        .into_iter()
                        .filter(|c| {
                            c.name.to_lowercase().contains(&q)
                                || c.ibans.iter().any(|i| i.to_lowercase().contains(&q))
                                || [RuleKind::Text, RuleKind::TextIn, RuleKind::TextOut]
                                    .iter()
                                    .any(|k| c.rules(*k).iter().any(|p| p.contains(&q)))
                        })
                        .collect();
                    (!cats.is_empty()).then_some((g, cats))
                })
                .collect::<Vec<_>>()
        })
    };

    view! {
        <header class="page-head"><h1>{t!("Categories")}</h1></header>
        <form class="inline-form panel" on:submit=submit>
            <label>{t!("Name")}<input required placeholder=t!("New category") prop:value=move || name.get() on:input=move |ev| name.set(event_target_value(&ev))/></label>
            <label>{t!("Group")}
                // A new name, or one of the groups there are.
                <ComboBox
                    free_text=true
                    label=t!("Group")
                    placeholder=t!("E.g. Housing")
                    options=Signal::derive(move || ctx.data.with(|d| {
                        grouped(d).into_iter().map(|(g, _)| g).filter(|g| !g.is_empty()).map(|g| ComboOption::new(g.clone(), g)).collect::<Vec<_>>()
                    }))
                    value=group
                    on_change=move |v| group.set(v)
                />
            </label>
            <label>{t!("Kind")}
                <ComboBox
                    label=t!("Kind")
                    options=Signal::derive(move || kind_options(locked()))
                    value=Signal::derive(move || kind.get().key().to_string())
                    on_change=move |v: String| kind.set(CategoryKind::from_key(&v).unwrap_or_default())
                />
            </label>
            <button type="submit" class="dark">{t!("Add")}</button>
        </form>
        <div class="filters">
            <input
                type="search"
                class="search"
                aria-label=t!("Search categories")
                placeholder=t!("Search category, group or IBAN")
                prop:value=move || query.get()
                on:input=move |ev| query.set(event_target_value(&ev))
            />
        </div>
        <Show when=move || visible_groups().is_empty()>
            <p class="panel empty">{t!("No category found.")}</p>
        </Show>
        <For
            each=visible_groups
            key=|g| g.clone()
            children=move |(group, cats): (String, Vec<Category>)| {
                let title = if group.is_empty() { t!("No group").to_string() } else { group };
                view! {
                    <section class="panel list">
                        <h2 class="group-title">{title}</h2>
                        {cats.into_iter().map(|c| view! { <CategoryRow cat=c rules_open=rules_open/> }).collect_view()}
                    </section>
                }
            }
        />
    }
}

/// The kinds a category can be given. A group holds fixed or variable costs, never
/// both: in a group that already holds one (`locked`, see group_cost_kind) the other
/// isn't offered.
fn kind_options(locked: Option<CategoryKind>) -> Vec<ComboOption> {
    let other = match locked {
        Some(CategoryKind::Fixed) => Some(CategoryKind::Variable),
        Some(CategoryKind::Variable) => Some(CategoryKind::Fixed),
        _ => None,
    };
    CategoryKind::SELECTABLE
        .into_iter()
        .filter(|k| Some(*k) != other)
        .map(|k| ComboOption::new(k.key(), kind_label(k)))
        .collect()
}

/// Fixed or variable: the kind of costs a group's switched-on categories (other than
/// `except`) hold, if any. Categories without a group aren't a group.
fn group_cost_kind(ds: &Dataset, group: &str, except: Option<&str>) -> Option<CategoryKind> {
    let group = group.trim();
    if group.is_empty() {
        return None;
    }
    ds.categories
        .iter()
        .filter(|c| !c.disabled && c.group == group && Some(c.id.as_str()) != except)
        .find_map(|c| matches!(c.kind, CategoryKind::Fixed | CategoryKind::Variable).then_some(c.kind))
}

#[component]
fn CategoryRow(cat: Category, rules_open: RwSignal<Option<String>>) -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    // Every edit sends the category as rendered with one field changed.
    let edit = {
        let cat = cat.clone();
        move |change: &dyn Fn(&mut Category)| {
            let mut c = cat.clone();
            change(&mut c);
            apply(ctx, api::update_category(c));
        }
    };
    let (e1, e2, e3) = (edit.clone(), edit.clone(), edit);
    let disabled_now = cat.disabled;
    let use_id = cat.id.clone();
    let in_use = Memo::new(move |_| {
        ctx.data.with(|d| d.transactions.iter().filter(|t| t.category_id.as_deref() == Some(use_id.as_str())).count())
    });
    let id = cat.id.clone();
    let open_id = cat.id.clone();
    let is_open = Memo::new(move |_| rules_open.with(|o| o.as_deref() == Some(open_id.as_str())));
    let toggle_id = cat.id.clone();
    let rule_count = cat.rule_count();

    view! {
        <div class=format!("cat-edit-row tinted {}", cat_tone(Some(&cat))) class:disabled=cat.disabled>
            {if cat.system {
                view! {
                    <span class="cat-name">{cat_badge(Some(&cat))}{cat.name.clone()}<span class="tag">{t!("Standard")}</span></span>
                }.into_any()
            } else {
                view! {
                    <span class="cat-name">
                        {cat_badge(Some(&cat))}
                        <input aria-label=t!("Name") value=cat.name.clone() on:change=move |ev| {
                            let v = event_target_value(&ev);
                            e1(&|c| c.name = v.clone());
                        }/>
                    </span>
                }.into_any()
            }}
            {if cat.kind == CategoryKind::Transfer {
                view! {
                    <span class="fixed-kind" title=t!("Transfers between your own accounts count nowhere: not as income or spending, not in budgets or charts")>
                        {t!("Transfer")}
                    </span>
                }.into_any()
            } else if cat.system {
                // Standard categories keep the kind from the default list.
                view! { <span class="fixed-kind" title=t!("A standard category's kind is fixed")>{kind_label(cat.kind)}</span> }.into_any()
            } else {
                // Shows the new kind right away; the row redraws once it is saved. Only
                // the kinds its group allows.
                let kind = RwSignal::new(cat.kind.key().to_string());
                let (group, id) = (cat.group.clone(), cat.id.clone());
                let options = Signal::derive(move || kind_options(ctx.data.with(|d| group_cost_kind(d, &group, Some(&id)))));
                view! {
                    <ComboBox
                        label=t!("Kind")
                        options=options
                        value=kind
                        on_change=move |v: String| {
                            let k = CategoryKind::from_key(&v).unwrap_or_default();
                            kind.set(v);
                            e2(&|c| c.kind = k);
                        }
                    />
                }.into_any()
            }}
            <label class="check" title=move || match in_use.get() {
                0 => t!("Disabled: cannot be chosen for transactions and hidden in budgets").to_string(),
                n if !disabled_now => tn!(
                    n,
                    "In use by {} transaction; move it to another category first",
                    "In use by {} transactions; move them to another category first",
                    n
                ),
                _ => String::new(),
            }>
                <input type="checkbox" checked=cat.disabled disabled={move || !disabled_now && in_use.get() > 0} on:change=move |ev| {
                    let v = event_target_checked(&ev);
                    e3(&|c| c.disabled = v);
                }/>
                {t!("Disabled")}
            </label>
            <button class="small" aria-expanded=move || is_open.get().to_string() on:click=move |_| {
                let id = toggle_id.clone();
                rules_open.update(|o| *o = if o.as_deref() == Some(id.as_str()) { None } else { Some(id) });
            }>
                {if rule_count == 0 { t!("Rules").to_string() } else { t!("Rules ({})", rule_count) }}
            </button>
            {if cat.system {
                view! { <span></span> }.into_any()
            } else {
                view! {
                    <button class="icon" aria-label=t!("Delete") on:click=move |_| {
                        let id = id.clone();
                        apply(ctx, async move { api::delete_category(&id).await })
                    }><TrashIcon/></button>
                }.into_any()
            }}
        </div>
        <Show when=move || is_open.get()>
            <IbanRules cat=cat.clone()/>
        </Show>
    }
}

/// A category's rules: IBANs and "description contains" texts. One input: an IBAN becomes
/// an IBAN rule, anything else a text rule.
#[component]
fn IbanRules(cat: Category) -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let input = RwSignal::new(String::new());
    // For text rules: both directions, or only money in / only money out.
    let direction = RwSignal::new(RuleKind::Text);
    let note = RwSignal::new(None::<String>);
    let add_id = cat.id.clone();
    let add = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let (id, value) = (add_id.clone(), input.get_untracked());
        let kind = if normalize_iban(&value).is_some() { RuleKind::Iban } else { direction.get_untracked() };
        spawn_local(async move {
            match api::add_category_rule(&id, kind, &value).await {
                Ok(r) => {
                    ctx.data.set(r.data);
                    ctx.error.set(None);
                    offer_rule_for_year(ctx, &id, kind, &value);
                    input.set(String::new());
                    note.set(Some(match r.applied {
                        0 => t!("Rule added.").to_string(),
                        n => tn!(n, "Rule added, {} transaction categorised.", "Rule added, {} transactions categorised.", n),
                    }));
                }
                Err(e) => ctx.error.set(Some(e)),
            }
        });
    };
    let rule_item = move |cat_id: String, kind: RuleKind, value: String| {
        let label = rule_label(kind);
        let shown = value.clone();
        view! {
            <li>
                <span class="muted">{label}</span>
                <span class="rule-value">{shown}</span>
                <button class="icon" aria-label=t!("Remove rule {}", value) on:click=move |_| {
                    let (id, value) = (cat_id.clone(), value.clone());
                    apply(ctx, async move { api::remove_category_rule(&id, kind, &value).await })
                }><TrashIcon/></button>
            </li>
        }
    };

    view! {
        <div class="iban-rules">
            <p class="muted">
                {t!("Rules: transactions with this IBAN or with this text in the description always go to")}" "
                <strong>{cat.name.clone()}</strong>{t!(", on import and for transactions still to categorise.")}
            </p>
            <ul>
                {RuleKind::ALL.into_iter().flat_map(|k| {
                    cat.rules(k).iter().map(move |v| (k, v.clone())).collect::<Vec<_>>()
                }).map(|(k, v)| rule_item(cat.id.clone(), k, v)).collect_view()}
            </ul>
            <form class="iban-add" on:submit=add>
                <input
                    aria-label=t!("IBAN or text from the description")
                    placeholder=t!("IBAN, or text like albert heijn")
                    required
                    prop:value=move || input.get()
                    on:input=move |ev| input.set(event_target_value(&ev))
                />
                <ComboBox
                    label=t!("Direction (for text rules)")
                    options=vec![
                        ComboOption::new("both", t!("Money in and out")),
                        ComboOption::new("in", t!("Money in only")),
                        ComboOption::new("out", t!("Money out only")),
                    ]
                    value=Signal::derive(move || match direction.get() {
                        RuleKind::TextIn => "in",
                        RuleKind::TextOut => "out",
                        _ => "both",
                    }.to_string())
                    on_change=move |v: String| direction.set(match v.as_str() {
                        "in" => RuleKind::TextIn,
                        "out" => RuleKind::TextOut,
                        _ => RuleKind::Text,
                    })
                />
                <button type="submit">{t!("Add rule")}</button>
            </form>
            {move || note.get().map(|n| view! { <p class="muted">{n}</p> })}
        </div>
    }
}

/// `2026-09-30T14:29:00Z` in local time: `30-9-2026 16:29` or `30 Sep 2026 16:29`.
fn local_time(iso: &str) -> String {
    let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_str(iso));
    if d.get_time().is_nan() {
        return iso.to_string();
    }
    let day = i18n::date(d.get_full_year() as i32, d.get_month() + 1, d.get_date());
    format!("{day} {:02}:{:02}", d.get_hours(), d.get_minutes())
}

/// `2026-09-30` as `30-9-2026` or `30 Sep 2026`.
fn short_date(iso_date: &str) -> String {
    let p: Vec<&str> = iso_date.split('-').collect();
    match p.as_slice() {
        [y, m, d] => match (y.parse(), m.parse(), d.parse()) {
            (Ok(y), Ok(m), Ok(d)) => i18n::date(y, m, d),
            _ => iso_date.to_string(),
        },
        _ => iso_date.to_string(),
    }
}

/// A destructive action with a confirmation step inside the page: the first click asks,
/// the second does it.
#[component]
fn ConfirmButton(
    label: &'static str,
    question: String,
    on_confirm: impl Fn() + Clone + Send + Sync + 'static,
    #[prop(optional)] disabled: bool,
) -> impl IntoView {
    let armed = RwSignal::new(false);
    view! {
        <Show
            when=move || armed.get()
            fallback=move || view! { <button class="small" disabled=disabled on:click=move |_| armed.set(true)>{label}</button> }
        >
            <span class="confirm">
                <span>{question.clone()}</span>
                <button class="small danger" on:click={
                    let f = on_confirm.clone();
                    move |_| {
                        armed.set(false);
                        f();
                    }
                }>{t!("Yes")}</button>
                <button class="small" on:click=move |_| armed.set(false)>{t!("No")}</button>
            </span>
        </Show>
    }
}

#[component]
fn DataPage() -> impl IntoView {
    view! {
        <header class="page-head"><h1>{t!("Import & backup")}</h1></header>
        <ImportLog/>
        <Backups/>
    }
}

#[component]
fn ImportLog() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let account_name = move |id: &str| {
        ctx.data.with_untracked(|d| d.accounts.iter().find(|a| a.id == id).map(|a| a.name.clone())).unwrap_or_default()
    };
    view! {
        <section class="panel list">
            <h2 class="group-title">{t!("Import history")}</h2>
            <Show when=move || ctx.data.with(|d| d.imports.is_empty())>
                <p class="empty">{t!("No imports yet.")}</p>
            </Show>
            {move || ctx.data.get().imports.into_iter().map(|r| {
                let remaining = ctx.data.with_untracked(|d| {
                    d.transactions.iter().filter(|t| t.import_id.as_deref() == Some(r.id.as_str())).count()
                });
                let id = r.id.clone();
                let summary = t!("{} imported · {} duplicates · {} by rules", r.imported, r.skipped_duplicates, r.classified);
                view! {
                    <div class="log-row" class:undone=r.undone_at.is_some()>
                        <div class="log-main">
                            <strong>{local_time(&r.at)}</strong>
                            <span class="muted">{account_name(&r.account_id)}" · "{summary}</span>
                            <ul class="log-files">
                                {r.files.iter().map(|f| {
                                    let period = match (&f.date_from, &f.date_to) {
                                        (Some(a), Some(b)) if a == b => short_date(a),
                                        (Some(a), Some(b)) => t!("{} through {}", short_date(a), short_date(b)),
                                        _ => t!("no entries").to_string(),
                                    };
                                    view! {
                                        <li>
                                            <code>{f.name.clone()}</code>
                                            <span class="muted">
                                                {format!(" {period} · {}", t!("{} entries, {} new, {} duplicates", f.entries, f.imported, f.skipped_duplicates))}
                                            </span>
                                        </li>
                                    }
                                }).collect_view()}
                            </ul>
                        </div>
                        <div class="log-action">
                            {match &r.undone_at {
                                Some(at) => view! { <span class="muted">{t!("Undone")}" "{local_time(at)}</span> }.into_any(),
                                None => view! {
                                    <ConfirmButton
                                        label=t!("Undo")
                                        question=tn!(remaining, "Delete {} transaction?", "Delete {} transactions?", remaining)
                                        disabled=remaining == 0
                                        on_confirm=move || {
                                            let id = id.clone();
                                            apply(ctx, async move { api::undo_import(&id).await })
                                        }
                                    />
                                }.into_any(),
                            }}
                        </div>
                    </div>
                }
            }).collect_view()}
        </section>
    }
}

/// backup::BEFORE_RESTORE in the backend.
const BEFORE_RESTORE: &str = "voor-herstel";

fn reason_label(reason: &str) -> &'static str {
    match reason {
        // The reasons are part of the backup file names, so they stay Dutch.
        "dagelijks" => t!("Daily"),
        "handmatig" => t!("Manual"),
        "voor-import" => t!("Before import"),
        "voor-herstel" => t!("Before restore"),
        _ => t!("Other"),
    }
}

#[component]
fn Backups() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let backups = RwSignal::new(Vec::<fin_shared::BackupInfo>::new());
    let note = RwSignal::new(None::<String>);
    let picked = RwSignal::new(None::<(String, String)>);

    // Saves make daily and pre-import backups, so reload whenever the data changes.
    Effect::new(move |_| {
        ctx.data.track();
        spawn_local(async move {
            match api::list_backups().await {
                Ok(b) => backups.set(b),
                Err(e) => ctx.error.set(Some(e)),
            }
        });
    });

    let make = move |_| {
        spawn_local(async move {
            match api::create_backup().await {
                Ok(b) => {
                    backups.set(b);
                    note.set(Some(t!("Backup made.").into()));
                }
                Err(e) => ctx.error.set(Some(e)),
            }
        });
    };
    let export = move |_| {
        spawn_local(async move {
            match api::export_backup().await {
                Ok(path) => note.set(Some(t!("Copy saved: {}", path))),
                Err(e) => ctx.error.set(Some(e)),
            }
        });
    };
    let pick = move |ev: leptos::ev::Event| {
        let Some(input) = ev.target().and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok()) else { return };
        let Some(file) = input.files().and_then(|l| l.get(0)) else { return };
        input.set_value("");
        spawn_local(async move {
            match JsFuture::from(file.text()).await {
                Ok(v) => picked.set(Some((file.name(), v.as_string().unwrap_or_default()))),
                Err(_) => ctx.error.set(Some(t!("Cannot read {}", file.name()))),
            }
        });
    };
    let restored = move |what: String| {
        note.set(Some(t!("Restored: {}. The previous state is in the list as a backup \"{}\".", what, reason_label(BEFORE_RESTORE))));
    };

    view! {
        <section class="panel list">
            <div class="section-head">
                <h2>{t!("Backups")}</h2>
                <span class="actions">
                    <button on:click=make>{t!("Make a backup")}</button>
                    <button on:click=export>{t!("Copy to Downloads")}</button>
                    <label class="file-button">
                        {t!("Restore from file…")}
                        <input type="file" accept=".json,application/json" on:change=pick/>
                    </label>
                </span>
            </div>
            <p class="muted">
                {t!("Fin makes a backup every day at the first change, and also before every import and every restore. The last 30 automatic backups are kept, manual ones always.")}
            </p>
            {move || note.get().map(|n| view! { <p class="result">{n}</p> })}
            {move || picked.get().map(|(name, json)| view! {
                <div class="confirm-box">
                    <span>{t!("Replace all data with")}" "<code>{name.clone()}</code>"?"</span>
                    <button class="small danger" on:click={
                        let (name, json) = (name.clone(), json.clone());
                        move |_| {
                            let (name, json) = (name.clone(), json.clone());
                            picked.set(None);
                            spawn_local(async move {
                                match api::restore_backup_file(json).await {
                                    Ok(d) => {
                                        ctx.data.set(d);
                                        ctx.error.set(None);
                                        restored(name);
                                    }
                                    Err(e) => ctx.error.set(Some(e)),
                                }
                            });
                        }
                    }>{t!("Restore")}</button>
                    <button class="small" on:click=move |_| picked.set(None)>{t!("Cancel")}</button>
                </div>
            })}
            <Show when=move || backups.with(|b| b.is_empty())>
                <p class="empty">{t!("No backups yet. The first one comes with tomorrow's first change, or make one now.")}</p>
            </Show>
            {move || backups.get().into_iter().map(|b| {
                let name = b.name.clone();
                let when = local_time(&b.created);
                let when_q = when.clone();
                view! {
                    <div class="backup-row">
                        <span>{when}</span>
                        <span class="tag">{reason_label(&b.reason)}</span>
                        <span class="muted right">{format!("{} kB", b.size_bytes.div_ceil(1024))}</span>
                        <ConfirmButton
                            label=t!("Restore")
                            question=t!("Restore everything to {}?", when_q)
                            on_confirm=move || {
                                let (name, when) = (name.clone(), when_q.clone());
                                spawn_local(async move {
                                    match api::restore_backup(&name).await {
                                        Ok(d) => {
                                            ctx.data.set(d);
                                            ctx.error.set(None);
                                            restored(t!("backup of {}", when));
                                        }
                                        Err(e) => ctx.error.set(Some(e)),
                                    }
                                });
                            }
                        />
                    </div>
                }
            }).collect_view()}
        </section>
    }
}
