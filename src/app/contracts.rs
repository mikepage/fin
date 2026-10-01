//! Contracts: a supplier's terms and, for electricity, the usage per period with its
//! cost under net metering and after it ends.

use fin_shared::contract::{add_days, energy_months, EnergyMonth};
use fin_shared::{Contract, EnergyTerms, UsagePeriod, Utility};
use leptos::prelude::*;

use super::{apply, euro, today, Ctx, TrashIcon};
use crate::{api, i18n};

/// A number in the app's notation with `dec` decimals: `0,15730` or `0.15730`.
fn num(v: f64, dec: usize) -> String {
    let s = format!("{v:.dec$}");
    match i18n::lang() {
        fin_shared::Lang::Nl => s.replace('.', ","),
        fin_shared::Lang::En => s,
    }
}

/// Accepts `0,1573`, `0.1573` and `1.234,5`.
fn parse_num(s: &str) -> Option<f64> {
    let s = s.trim();
    let s = if s.contains(',') { s.replace('.', "").replace(',', ".") } else { s.to_string() };
    s.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// kWh without decimals when whole.
fn kwh(v: f64) -> String {
    if v.fract() == 0.0 { num(v, 0) } else { num(v, 1) }
}

#[component]
pub(super) fn ContractsPage() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let year = RwSignal::new(today()[..4].parse::<i32>().unwrap_or(2026));
    let add = move |_| {
        let y = year.get_untracked();
        let start = if today().starts_with(&y.to_string()) { today() } else { format!("{y:04}-01-01") };
        let c = Contract { name: t!("New contract").into(), start, ..Default::default() };
        apply(ctx, api::save_contract(c));
    };
    view! {
        <header class="page-head">
            <button class="round" aria-label=t!("Previous year") on:click=move |_| year.update(|y| *y -= 1)>"‹"</button>
            <h1 class="year">{t!("Contracts")}" "{move || year.get()}</h1>
            <button class="round" aria-label=t!("Next year") on:click=move |_| year.update(|y| *y += 1)>"›"</button>
            <button class="dark" on:click=add>{t!("New contract")}</button>
        </header>
        <EnergyYear year=year/>
        <Show when=move || ctx.data.with(|d| !d.contracts.iter().any(|c| c.in_year(year.get())))>
            <p class="panel muted empty-note">{t!("No contracts yet.")}</p>
        </Show>
        <For
            // The year's contracts, newest first.
            each=move || {
                let y = year.get();
                let mut cs: Vec<Contract> = ctx.data.get().contracts.into_iter().filter(|c| c.in_year(y)).collect();
                cs.sort_by(|a, b| b.start.cmp(&a.start).then_with(|| a.name.cmp(&b.name)));
                cs
            }
            // Any change re-renders the card, so its inputs show what was saved.
            key=|c| format!("{c:?}")
            children=move |c: Contract| view! { <ContractCard contract=c/> }
        />
    }
}

/// The year per month: electricity, gas, the monthly payments and what is left (paid
/// minus cost). Months without usage are estimated from a year earlier, in italics.
#[component]
fn EnergyYear(year: RwSignal<i32>) -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    move || {
        let y = year.get();
        let months = ctx.data.with(|d| energy_months(&d.contracts, y));
        if months.iter().all(|m| m.cost_cents() == 0 && m.payment_cents == 0) {
            return None;
        }
        let cell = |cents: i64, estimated: bool| view! { <td class="amount" class:estimated=estimated>{euro(cents)}</td> };
        let balance = |cents: i64| {
            let short = cents < 0;
            view! { <td class="amount" class:over=short class:ok=!short>{euro(cents)}</td> }
        };
        // Year-end and final bills get a column of their own, only in a year that has them.
        let bills = months.iter().any(|m| m.bill_cents != 0 || m.bill_expected_cents != 0);
        let bill = move |cents: i64, expected: bool| bills.then(|| {
            let (back, extra) = (cents > 0, cents < 0);
            let text = (cents != 0).then(|| format!("{}{}", if back { "+" } else { "" }, euro(cents)));
            view! { <td class="amount bill" class:ok=back class:over=extra class:estimated=expected>{text}</td> }
        });
        let rows = months.iter().map(|m| view! {
            <tr>
                <td>{super::capitalize(i18n::month_name(m.month as usize - 1))}</td>
                {cell(m.electricity_cents, m.electricity_estimated)}
                {cell(m.gas_cents, m.gas_estimated)}
                {cell(m.payment_cents, false)}
                {balance(m.balance_cents())}
                {if m.bill_cents == 0 && m.bill_expected_cents != 0 { bill(m.bill_expected_cents, true) } else { bill(m.bill_cents, false) }}
            </tr>
        }).collect_view();
        let sum = |f: fn(&EnergyMonth) -> i64| months.iter().map(f).sum::<i64>();
        let any_estimate = months.iter().any(|m| m.electricity_estimated || m.gas_estimated);
        Some(view! {
            <section class="panel ct-year">
                <table>
                    <thead><tr>
                        <th>{t!("Month")}</th><th>{t!("Electricity")}</th><th>{t!("Gas")}</th>
                        <th>{t!("Monthly payment")}</th><th>{t!("Balance")}</th>
                        {bills.then(|| view! { <th title=t!("Paid back (+) or charged on top (−) with a year-end or final bill")>{t!("Settlement")}</th> })}
                    </tr></thead>
                    <tbody>{rows}</tbody>
                    <tfoot><tr>
                        <td>{t!("Year")}</td>
                        {cell(sum(|m| m.electricity_cents), false)}
                        {cell(sum(|m| m.gas_cents), false)}
                        {cell(sum(|m| m.payment_cents), false)}
                        {balance(sum(EnergyMonth::balance_cents))}
                        {bill(sum(|m| if m.bill_cents == 0 { m.bill_expected_cents } else { m.bill_cents }), false)}
                    </tr></tfoot>
                </table>
                {any_estimate.then(|| view! { <p class="muted ct-note">{t!("In italics: estimated from earlier usage")}</p> })}
                {months.iter().filter(|m| m.settlement_cents != 0).map(|m| view! {
                    <p class="muted ct-note">{t!("{}: includes {} salderen surplus at the low compensation", super::capitalize(i18n::month_name(m.month as usize - 1)), euro(m.settlement_cents))}</p>
                }).collect_view()}
            </section>
        })
    }
}

#[component]
fn ContractCard(contract: Contract) -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let stored = StoredValue::new(contract.clone());
    // Saves the contract with one change.
    let edit = move |f: Box<dyn FnOnce(&mut Contract)>| {
        let mut c = stored.get_value();
        f(&mut c);
        apply(ctx, api::save_contract(c));
    };
    let text = move |label: &'static str, value: String, kind: &'static str, set: fn(&mut Contract, String)| {
        view! {
            <label>{label}<input type=kind value=value on:change=move |ev| {
                let v = event_target_value(&ev);
                edit(Box::new(move |c| set(c, v)))
            }/></label>
        }
    };
    let monthly = i18n::cents(contract.monthly_cents);
    let id = contract.id.clone();
    let energy = contract.energy.clone();
    let utility = energy.as_ref().map(|e| e.utility);
    let has_energy = utility.is_some();

    view! {
        <section class="panel contract">
            <div class="section-head">
                <h2>{contract.name.clone()}</h2>
                <button class="icon" aria-label=t!("Delete") on:click=move |_| {
                    let id = id.clone();
                    apply(ctx, async move { api::delete_contract(&id).await })
                }><TrashIcon/></button>
            </div>
            <div class="ct-grid">
                {text(t!("Name"), contract.name.clone(), "text", |c, v| c.name = v)}
                {text(t!("Supplier"), contract.supplier.clone(), "text", |c, v| c.supplier = v)}
                {text(t!("Start"), contract.start.clone(), "date", |c, v| c.start = v)}
                {text(t!("End"), contract.end.clone(), "date", |c, v| c.end = v)}
                {has_energy.then(|| text(t!("Expected year-end bill"), contract.bill_due.clone(), "date", |c, v| c.bill_due = v))}
                <label>{t!("Monthly payment")}<input class="right" value=monthly on:change=move |ev| {
                    let v = event_target_value(&ev);
                    match fin_shared::parse_amount(&v) {
                        Some(cents) => edit(Box::new(move |c| c.monthly_cents = cents)),
                        None => ctx.error.set(Some(t!("Invalid amount: {}", v))),
                    }
                }/></label>
                <label>{t!("Type")}
                    <select on:change=move |ev| {
                        let utility = match event_target_value(&ev).as_str() {
                            "electricity" => Some(Utility::Electricity),
                            "gas" => Some(Utility::Gas),
                            _ => None,
                        };
                        edit(Box::new(move |c| {
                            c.energy = utility.map(|u| EnergyTerms { utility: u, ..c.energy.clone().unwrap_or_default() })
                        }))
                    }>
                        <option value="" selected=utility.is_none()>{t!("Other")}</option>
                        <option value="electricity" selected=utility == Some(Utility::Electricity)>{t!("Electricity")}</option>
                        <option value="gas" selected=utility == Some(Utility::Gas)>{t!("Gas")}</option>
                    </select>
                </label>
            </div>
            {text(t!("Notes"), contract.notes.clone(), "text", |c, v| c.notes = v)}
            {energy.map(|e| view! { <EnergySection terms=e usage=contract.usage.clone() netting_allowed=contract.netting_allowed() edit=edit/> })}
        </section>
    }
}

type Edit = Box<dyn FnOnce(&mut Contract)>;

#[component]
fn EnergySection(
    terms: EnergyTerms,
    usage: Vec<UsagePeriod>,
    netting_allowed: bool,
    edit: impl Fn(Edit) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    // A tariff input; `dec` decimals as the supplier quotes them.
    let rate = move |label: &'static str, value: f64, dec: usize, set: fn(&mut EnergyTerms, f64)| {
        view! {
            <label>{label}<input class="right" value=num(value, dec) on:change=move |ev| {
                let v = event_target_value(&ev);
                match parse_num(&v) {
                    Some(n) => edit(Box::new(move |c| if let Some(e) = c.energy.as_mut() { set(e, n) })),
                    None => ctx.error.set(Some(t!("Invalid number: {}", v))),
                }
            }/></label>
        }
    };
    // Gas: one rate per m³, no feed-in and no tax reduction.
    let elec = terms.utility == Utility::Electricity;
    let netting = elec && netting_allowed && terms.netting;
    let pos = terms.netting_position(&usage);
    let costs: Vec<_> = usage.iter().map(|p| terms.period_cost(p)).collect();
    // What a returned kWh is worth, against the price of a kWh taken.
    let value = if netting { terms.price_normal() } else { terms.feed_in_net() };
    let share = if terms.price_normal() > 0.0 { (value / terms.price_normal() * 100.0).round() } else { 0.0 };
    let surplus_rate = num(terms.feed_in_incl(), 4);

    let rows = usage.iter().enumerate().map(|(i, p)| {
        let cost = costs[i];
        let cell = move |value: f64, set: fn(&mut UsagePeriod, f64)| view! {
            <td><input class="right" value=kwh(value) on:change=move |ev| {
                let v = event_target_value(&ev);
                match parse_num(&v) {
                    Some(n) if n >= 0.0 => edit(Box::new(move |c| if let Some(p) = c.usage.get_mut(i) { set(p, n) })),
                    _ => ctx.error.set(Some(t!("Invalid number: {}", v))),
                }
            }/></td>
        };
        let date = move |value: String, set: fn(&mut UsagePeriod, String)| view! {
            <td><input type="date" value=value on:change=move |ev| {
                let v = event_target_value(&ev);
                edit(Box::new(move |c| if let Some(p) = c.usage.get_mut(i) { set(p, v) }))
            }/></td>
        };
        view! {
            <tr>
                {date(p.start.clone(), |p, v| p.start = v)}
                {date(p.end.clone(), |p, v| p.end = v)}
                {cell(p.used_normal, |p, v| p.used_normal = v)}
                {elec.then(|| view! {
                    {cell(p.used_low, |p, v| p.used_low = v)}
                    {cell(p.returned_normal, |p, v| p.returned_normal = v)}
                    {cell(p.returned_low, |p, v| p.returned_low = v)}
                })}
                <td class="amount">{euro(cost.total_cents)}</td>
                <td><button class="icon" aria-label=t!("Delete") on:click=move |_| edit(Box::new(move |c| { c.usage.remove(i); }))><TrashIcon/></button></td>
            </tr>
        }
    }).collect_view();
    let next_start = usage.last().map(|p| add_days(&p.end, 1)).unwrap_or_else(today);

    view! {
        <h3 class="ct-head">{if elec { t!("Per kWh") } else { t!("Per m³") }}</h3>
        <div class="ct-grid">
            {if elec {
                view! {
                    {rate(t!("Supply normal"), terms.supply_normal, 5, |e, v| e.supply_normal = v)}
                    {rate(t!("Supply low"), terms.supply_low, 5, |e, v| e.supply_low = v)}
                }.into_any()
            } else {
                rate(t!("Gas rate"), terms.supply_normal, 5, |e, v| e.supply_normal = v).into_any()
            }}
            {rate(t!("Energy tax"), terms.energy_tax, 5, |e, v| e.energy_tax = v)}
            <div class="ct-sum">
                {if elec {
                    view! { <span>{t!("Price normal / low")}</span><strong>{format!("€ {} / {}", num(terms.price_normal(), 5), num(terms.price_low(), 5))}</strong> }.into_any()
                } else {
                    view! { <span>{t!("Price")}</span><strong>{format!("€ {}", num(terms.price_normal(), 5))}</strong> }.into_any()
                }}
            </div>
        </div>
        <Show when=move || elec>
        <h3 class="ct-head">{t!("Feed-in")}</h3>
        <div class="ct-grid">
            // Net metering ended with 2026, so only a contract ending before can have it.
            {netting_allowed.then(|| view! {
                <label class="check ct-energy-toggle">
                    <input type="checkbox" prop:checked=netting on:change=move |ev| {
                        let on = event_target_checked(&ev);
                        edit(Box::new(move |c| if let Some(e) = c.energy.as_mut() { e.netting = on }))
                    }/>
                    {t!("Salderen")}
                </label>
            })}
            {rate(t!("Compensation (excl. VAT)"), terms.feed_in, 5, |e, v| e.feed_in = v)}
            {rate(t!("Feed-in costs per month"), terms.feed_in_cost_month, 2, |e, v| e.feed_in_cost_month = v)}
            {rate(t!("Feed-in costs per kWh"), terms.feed_in_cost_kwh, 5, |e, v| e.feed_in_cost_kwh = v)}
            <div class="ct-sum">
                <span>{t!("Value per kWh returned")}</span>
                <strong>{format!("€ {} ({share}%)", num(value, 5))}</strong>
            </div>
        </div>
        </Show>
        <h3 class="ct-head">{t!("Fixed")}</h3>
        <div class="ct-grid">
            {rate(t!("Fixed supply costs per month"), terms.fixed_month, 2, |e, v| e.fixed_month = v)}
            {rate(t!("Network costs per year"), terms.network_year, 2, |e, v| e.network_year = v)}
            {elec.then(|| rate(t!("Energy tax reduction per year"), terms.tax_reduction_year, 2, |e, v| e.tax_reduction_year = v))}
            {elec.then(|| rate(t!("VAT %"), terms.vat, 0, |e, v| e.vat = v))}
        </div>

        <h3 class="ct-head">{t!("Usage")}</h3>
        <div class="ct-usage">
            <table>
                <thead><tr>
                    <th>{t!("From")}</th><th>{t!("To")}</th>
                    {if elec {
                        view! {
                            <th>{t!("Used normal")}</th><th>{t!("Used low")}</th>
                            <th>{t!("Returned normal")}</th><th>{t!("Returned low")}</th>
                        }.into_any()
                    } else {
                        view! { <th>{t!("Used (m³)")}</th> }.into_any()
                    }}
                    <th>{t!("Cost")}</th><th></th>
                </tr></thead>
                <tbody>{rows}</tbody>
            </table>
            <button class="link" on:click=move |_| {
                let start = next_start.clone();
                edit(Box::new(move |c| c.usage.push(UsagePeriod { end: start.clone(), start, ..Default::default() })))
            }>{t!("+ Period")}</button>
        </div>

        // Net metering: kWh returned beyond usage only get the low compensation.
        {(netting && pos.surplus > 0.0).then(|| view! {
            <p class="over ct-note">{t!("{} kWh surplus at € {}: {} less", kwh(pos.surplus), surplus_rate.clone(), euro(pos.surplus_loss_cents))}</p>
        })}
    }
}
