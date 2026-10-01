//! Contracts: what a supplier charges, with an energy section for electricity and the
//! meter readings per period, so a period's cost can be estimated under net metering
//! (until it ends) and under the rules after it.
//!
//! Rates are euros per kWh (or per month / year) as the supplier's tariff sheet states
//! them, so they keep their five decimals; results are cents.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Contract {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub supplier: String,
    /// First and last day (`YYYY-MM-DD`); the end is empty for an open-ended contract.
    #[serde(default)]
    pub start: String,
    #[serde(default)]
    pub end: String,
    /// The monthly advance payment, in cents.
    #[serde(default)]
    pub monthly_cents: i64,
    #[serde(default)]
    pub notes: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub energy: Option<EnergyTerms>,
    /// Usage per period, oldest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub usage: Vec<UsagePeriod>,
    /// Year-end or final bills: what the supplier paid back (positive) or charged on
    /// top of the monthly payments (negative), on the day it was paid.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub settlements: Vec<Settlement>,
    /// When the next year-end bill is expected (`YYYY-MM-DD`), a year after the supply
    /// started. Contracts split at a change of terms share the date: the bill settles
    /// them together.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub bill_due: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Settlement {
    pub date: String,
    pub cents: i64,
    #[serde(default)]
    pub note: String,
}

/// An electricity contract's tariffs. "Incl." amounts include VAT; the feed-in
/// compensation is quoted without VAT, as suppliers do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EnergyTerms {
    pub utility: Utility,
    /// Supply rate per kWh (gas: per m³ in `supply_normal`), normal and low (off-peak),
    /// incl. VAT, without energy tax.
    pub supply_normal: f64,
    pub supply_low: f64,
    /// Energy tax per kWh incl. VAT.
    pub energy_tax: f64,
    /// Net metering (until 2027 in the Netherlands): a returned kWh offsets
    /// a used one at the full price, and only the surplus gets the compensation.
    pub netting: bool,
    /// Feed-in compensation per kWh excl. VAT: with net metering for the surplus beyond
    /// usage, without it for every kWh returned.
    pub feed_in: f64,
    /// Feed-in costs incl. VAT: per month (the supplier's scale, with net metering) and per
    /// kWh returned (without it).
    pub feed_in_cost_month: f64,
    pub feed_in_cost_kwh: f64,
    /// Fixed supply costs per month and network costs per year, incl. VAT.
    pub fixed_month: f64,
    pub network_year: f64,
    /// The energy tax reduction per year (one per dwelling), incl. VAT.
    pub tax_reduction_year: f64,
    /// VAT in percent, applied to the feed-in compensation.
    pub vat: f64,
}

/// What an energy contract supplies. Gas has one rate per m³ and no feed-in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Utility {
    #[default]
    Electricity,
    Gas,
}

impl Default for EnergyTerms {
    fn default() -> Self {
        Self {
            utility: Utility::Electricity,
            supply_normal: 0.0,
            supply_low: 0.0,
            energy_tax: 0.0,
            netting: false,
            feed_in: 0.0,
            feed_in_cost_month: 0.0,
            feed_in_cost_kwh: 0.0,
            fixed_month: 0.0,
            network_year: 0.0,
            tax_reduction_year: 0.0,
            vat: 21.0,
        }
    }
}

/// Meter readings for a period: kWh taken from and returned to the grid.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UsagePeriod {
    /// First and last day, both included.
    pub start: String,
    pub end: String,
    pub used_normal: f64,
    pub used_low: f64,
    pub returned_normal: f64,
    pub returned_low: f64,
}

impl UsagePeriod {
    pub fn used(&self) -> f64 {
        self.used_normal + self.used_low
    }

    pub fn returned(&self) -> f64 {
        self.returned_normal + self.returned_low
    }

    pub fn days(&self) -> i64 {
        match (day_number(&self.start), day_number(&self.end)) {
            (Some(a), Some(b)) if b >= a => b - a + 1,
            _ => 0,
        }
    }
}

/// A period's estimated cost; positive is what you pay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PeriodCost {
    /// kWh taken from the grid at the full price.
    pub usage_cents: i64,
    /// What returned kWh are worth (net metering: the full price; otherwise the
    /// compensation minus the feed-in costs per kWh).
    pub return_cents: i64,
    /// Fixed supply, network and monthly feed-in costs, minus the tax reduction.
    pub fixed_cents: i64,
    pub total_cents: i64,
}

/// Usage and return while net metering lasts. Net metering offsets a kWh returned against a
/// kWh used over the settlement, so only the surplus (returned beyond used) gets the
/// low feed-in compensation instead of the full price.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct NettingPosition {
    pub used: f64,
    pub returned: f64,
    /// kWh returned beyond usage so far.
    pub surplus: f64,
    /// What the surplus loses against the full price, in cents.
    pub surplus_loss_cents: i64,
}

// The store rejects non-finite numbers, so equality is total.
impl Eq for Contract {}
impl Eq for EnergyTerms {}
impl Eq for UsagePeriod {}

fn cents(euros: f64) -> i64 {
    (euros * 100.0).round() as i64
}

impl EnergyTerms {
    /// The full price per kWh incl. energy tax and VAT.
    pub fn price_normal(&self) -> f64 {
        self.supply_normal + self.energy_tax
    }

    pub fn price_low(&self) -> f64 {
        self.supply_low + self.energy_tax
    }

    fn with_vat(&self, excl: f64) -> f64 {
        excl * (1.0 + self.vat / 100.0)
    }

    /// The compensation per kWh incl. VAT.
    pub fn feed_in_incl(&self) -> f64 {
        self.with_vat(self.feed_in)
    }

    /// What a returned kWh is worth without net metering: the compensation minus the
    /// feed-in costs per kWh.
    pub fn feed_in_net(&self) -> f64 {
        self.feed_in_incl() - self.feed_in_cost_kwh
    }

    /// A period's cost. With net metering every returned kWh counts at the full price, as
    /// the supplier's monthly report shows it; whether that holds is up to the
    /// settlement (see `netting_position`).
    pub fn period_cost(&self, p: &UsagePeriod) -> PeriodCost {
        let share = p.days() as f64 / 365.0;
        let usage = p.used_normal * self.price_normal() + p.used_low * self.price_low();
        let ret = if self.netting {
            p.returned_normal * self.price_normal() + p.returned_low * self.price_low()
        } else {
            p.returned() * self.feed_in_net()
        };
        let feed_in_fixed = if p.returned() > 0.0 { self.feed_in_cost_month * 12.0 * share } else { 0.0 };
        let fixed = (self.fixed_month * 12.0 + self.network_year - self.tax_reduction_year) * share + feed_in_fixed;
        let (usage_cents, return_cents, fixed_cents) = (cents(usage), cents(ret), cents(fixed));
        PeriodCost { usage_cents, return_cents, fixed_cents, total_cents: usage_cents - return_cents + fixed_cents }
    }

    /// Usage and return over all periods, for a contract with net metering.
    pub fn netting_position(&self, usage: &[UsagePeriod]) -> NettingPosition {
        let (mut used, mut returned, mut value) = (0.0, 0.0, 0.0);
        for p in usage {
            used += p.used();
            returned += p.returned();
            value += p.returned_normal * self.price_normal() + p.returned_low * self.price_low();
        }
        let surplus = (returned - used).max(0.0);
        let avg_price = if returned > 0.0 { value / returned } else { self.price_normal() };
        let loss = surplus * (avg_price - self.with_vat(self.feed_in));
        NettingPosition { used, returned, surplus, surplus_loss_cents: cents(loss) }
    }
}

/// The first day without net metering in the Netherlands.
pub const SALDEREN_ENDS: &str = "2027-01-01";

impl Contract {
    /// Net metering only exists for a contract that ends before it was abolished.
    pub fn netting_allowed(&self) -> bool {
        !self.end.is_empty() && self.end.as_str() < SALDEREN_ENDS
    }

    /// Whether the contract runs on some day of `year`.
    pub fn in_year(&self, year: i32) -> bool {
        let (first, last) = (format!("{year:04}-01-01"), format!("{year:04}-12-31"));
        (self.start.is_empty() || self.start <= last) && (self.end.is_empty() || self.end >= first)
    }

    /// Whether the contract runs on `date`.
    pub fn runs_on(&self, date: &str) -> bool {
        (self.start.is_empty() || self.start.as_str() <= date) && (self.end.is_empty() || self.end.as_str() >= date)
    }
}

/// One month of energy: the cost per utility (actual, or estimated from the same month
/// a year earlier at the tariffs that apply now) and the monthly payments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct EnergyMonth {
    /// 1–12.
    pub month: u32,
    pub electricity_cents: i64,
    pub gas_cents: i64,
    /// Whether the cost is an estimate (no usage entered for that month).
    pub electricity_estimated: bool,
    pub gas_estimated: bool,
    /// Part of `electricity_cents`: the net metering surplus settled this month.
    pub settlement_cents: i64,
    pub payment_cents: i64,
    /// Year-end and final bills paid this month: back (positive) or on top (negative).
    pub bill_cents: i64,
    /// A year-end bill expected this month (`Contract::bill_due`): the monthly
    /// payments minus the cost of the contracts it settles, since they started.
    pub bill_expected_cents: i64,
}

impl EnergyMonth {
    pub fn cost_cents(&self) -> i64 {
        self.electricity_cents + self.gas_cents
    }

    /// Paid minus cost: positive is money back at the settlement.
    pub fn balance_cents(&self) -> i64 {
        self.payment_cents - self.cost_cents()
    }
}

/// Days `[a, b]` and `[c, d]` share (day numbers, both included).
fn overlap(a: i64, b: i64, c: i64, d: i64) -> i64 {
    (b.min(d) - a.max(c) + 1).max(0)
}

/// First and last day of a month as day numbers.
fn month_days(year: i32, month: u32) -> (i64, i64) {
    let first = day_number(&format!("{year:04}-{month:02}-01")).unwrap_or(0);
    let next = if month == 12 { format!("{:04}-01-01", year + 1) } else { format!("{year:04}-{:02}-01", month + 1) };
    (first, day_number(&next).unwrap_or(first + 31) - 1)
}

/// Per month of `year`: the energy cost and the monthly payments of the contracts.
/// A period's cost is spread over the months by days; a month with part of its days
/// covered is scaled up to the whole month. A month without usage is estimated from
/// the same month in the latest earlier year with usage. Tariffs and payments are
/// those of the contract running mid-month; a contract runs on after its end date
/// until a newer one starts (it continues, as a variable contract).
///
/// Net metering is settled per contract: the surplus (kWh returned beyond used, actual and
/// estimated, in this year) counted at the full price is taken back at the low
/// compensation, in the contract's last month (December if it runs on).
pub fn energy_months(contracts: &[Contract], year: i32) -> Vec<EnergyMonth> {
    let mut out = energy_months_base(contracts, contracts, year);
    // Expected year-end bills: per due date, the contracts it settles.
    let mut dues: Vec<&str> = contracts.iter().map(|c| c.bill_due.as_str()).filter(|d| d.len() == 10).collect();
    dues.sort();
    dues.dedup();
    for due in dues.into_iter().filter(|d| d[..4].parse::<i32>().is_ok_and(|y| y == year || y + 1 == year)) {
        let group: Vec<Contract> = contracts.iter().filter(|c| c.bill_due == due).cloned().collect();
        let first = group.iter().map(|c| c.start.as_str()).filter(|s| s.len() == 10).min().unwrap_or(due);
        // Whole months: a start after the 15th counts from the next month, a due date
        // after the 15th includes its month; the bill comes the month after.
        let ym = |d: &str| -> (i32, u32, u32) { (d[..4].parse().unwrap_or(year), d[5..7].parse().unwrap_or(1), d[8..10].parse().unwrap_or(1)) };
        let next = |(y, m): (i32, u32)| if m == 12 { (y + 1, 1) } else { (y, m + 1) };
        let (sy, sm, sd) = ym(first);
        let from = if sd > 15 { next((sy, sm)) } else { (sy, sm) };
        let (dy, dm, dd) = ym(due);
        let shown = if dd > 15 { next((dy, dm)) } else { (dy, dm) };
        if shown.0 != year {
            continue;
        }
        // Payments minus cost from the first month up to the month before the bill.
        let mut balance = 0;
        for y in from.0..=shown.0 {
            // The group's tariffs and payments, estimated from all earlier usage.
            for m in energy_months_base(&group, contracts, y) {
                if (y, m.month) >= from && (y, m.month) < shown {
                    balance += m.balance_cents();
                }
            }
        }
        out[shown.1 as usize - 1].bill_expected_cents += balance;
    }
    out
}

/// `energy_months` without expected bills; months without usage are estimated from
/// the usage in `history` (all contracts, also earlier suppliers').
fn energy_months_base(contracts: &[Contract], history: &[Contract], year: i32) -> Vec<EnergyMonth> {
    let mut out: Vec<EnergyMonth> = (1..=12).map(|month| EnergyMonth { month, ..Default::default() }).collect();
    // Bills paid this year, in the month they were paid.
    for s in contracts.iter().flat_map(|c| &c.settlements) {
        if s.date.len() >= 7 && s.date[..4] == format!("{year:04}") {
            if let Ok(m) = s.date[5..7].parse::<usize>() {
                if (1..=12).contains(&m) {
                    out[m - 1].bill_cents += s.cents;
                }
            }
        }
    }
    for utility in [Utility::Electricity, Utility::Gas] {
        let of_kind: Vec<(&Contract, &EnergyTerms)> = contracts
            .iter()
            .filter_map(|c| c.energy.as_ref().filter(|e| e.utility == utility).map(|e| (c, e)))
            .collect();
        // Per contract the usage counted this year, for the net metering settlement.
        let mut counted = vec![UsagePeriod::default(); of_kind.len()];
        for m in out.iter_mut() {
            let (first, last) = month_days(year, m.month);
            let mid = format!("{year:04}-{:02}-15", m.month);
            m.payment_cents += governing(&of_kind, &mid).map_or(0, |i| of_kind[i].0.monthly_cents);
            // Actual: each period's cost by the days it shares with the month.
            let (mut cost, mut covered) = (0.0, 0i64);
            for (i, (c, e)) in of_kind.iter().enumerate() {
                for p in &c.usage {
                    let (Some(a), Some(b)) = (day_number(&p.start), day_number(&p.end)) else { continue };
                    let days = overlap(a, b, first, last);
                    if days > 0 {
                        let share = days as f64 / (b - a + 1) as f64;
                        cost += e.period_cost(p).total_cents as f64 * share;
                        covered += days;
                        add_usage(&mut counted[i], p, share);
                    }
                }
            }
            let (cents, estimated) = if covered > 0 {
                ((cost * (last - first + 1) as f64 / covered.min(last - first + 1) as f64).round() as i64, false)
            } else if let Some((i, usage)) = estimate(&of_kind, history, utility, year, m.month, &mid) {
                add_usage(&mut counted[i], &usage, 1.0);
                (of_kind[i].1.period_cost(&usage).total_cents, true)
            } else {
                (0, false)
            };
            match utility {
                Utility::Electricity => (m.electricity_cents, m.electricity_estimated) = (cents, estimated),
                Utility::Gas => (m.gas_cents, m.gas_estimated) = (cents, estimated),
            }
        }
        for (i, (c, e)) in of_kind.iter().enumerate() {
            if !e.netting {
                continue;
            }
            let loss = e.netting_position(std::slice::from_ref(&counted[i])).surplus_loss_cents;
            if loss > 0 {
                let month = if c.end.starts_with(&format!("{year:04}-")) { c.end[5..7].parse().unwrap_or(12) } else { 12 };
                let m = &mut out[month as usize - 1];
                m.electricity_cents += loss;
                m.settlement_cents += loss;
            }
        }
    }
    out
}

fn add_usage(to: &mut UsagePeriod, p: &UsagePeriod, share: f64) {
    to.used_normal += p.used_normal * share;
    to.used_low += p.used_low * share;
    to.returned_normal += p.returned_normal * share;
    to.returned_low += p.returned_low * share;
}

/// The contract (index) that applies on `mid`: the one running then, else the latest
/// that started before, as a contract runs on until a newer one replaces it.
fn governing(of_kind: &[(&Contract, &EnergyTerms)], mid: &str) -> Option<usize> {
    of_kind.iter().position(|(c, _)| c.runs_on(mid)).or_else(|| {
        (0..of_kind.len()).filter(|&i| of_kind[i].0.start.as_str() <= mid).max_by(|&a, &b| of_kind[a].0.start.cmp(&of_kind[b].0.start))
    })
}

/// A month's usage from the same month in the latest earlier year with usage (up to
/// five years back), with the contract (index) whose tariffs apply.
fn estimate(
    of_kind: &[(&Contract, &EnergyTerms)],
    history: &[Contract],
    utility: Utility,
    year: i32,
    month: u32,
    mid: &str,
) -> Option<(usize, UsagePeriod)> {
    let (mut usage, mut covered, mut pf, mut pl) = (UsagePeriod::default(), 0i64, 0, 0);
    let past = history.iter().filter(|c| c.energy.as_ref().is_some_and(|e| e.utility == utility));
    for back in 1..=5 {
        (pf, pl) = month_days(year - back, month);
        for c in past.clone() {
            for p in &c.usage {
                let (Some(a), Some(b)) = (day_number(&p.start), day_number(&p.end)) else { continue };
                let days = overlap(a, b, pf, pl);
                if days > 0 {
                    add_usage(&mut usage, p, days as f64 / (b - a + 1) as f64);
                    covered += days;
                }
            }
        }
        if covered > 0 {
            break;
        }
    }
    if covered == 0 {
        return None;
    }
    let (first, last) = month_days(year, month);
    let scale = (pl - pf + 1) as f64 / covered.min(pl - pf + 1) as f64;
    usage.used_normal *= scale;
    usage.used_low *= scale;
    usage.returned_normal *= scale;
    usage.returned_low *= scale;
    usage.start = add_days(&format!("{year:04}-01-01"), first - day_number(&format!("{year:04}-01-01"))?);
    usage.end = add_days(&usage.start, last - first);
    Some((governing(of_kind, mid)?, usage))
}

/// Days since an epoch for a `YYYY-MM-DD` date (proleptic Gregorian).
pub fn day_number(date: &str) -> Option<i64> {
    if date.len() != 10 {
        return None;
    }
    let y: i64 = date[..4].parse().ok()?;
    let m: i64 = date[5..7].parse().ok()?;
    let d: i64 = date[8..10].parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let (y, m) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * m + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe)
}

/// The `YYYY-MM-DD` date `days` after `date` (the same date if it doesn't parse).
pub fn add_days(date: &str, days: i64) -> String {
    let Some(n) = day_number(date) else { return date.to_string() };
    let z = n + days;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixed-rate contract with net metering in 2026 and without it from 2027;
    /// the readings for 30-08 to 29-09-2026.
    fn terms() -> EnergyTerms {
        EnergyTerms {
            supply_normal: 0.15730,
            supply_low: 0.15730,
            energy_tax: 0.11085,
            netting: true,
            feed_in: 0.01,
            feed_in_cost_month: 14.0,
            fixed_month: 7.99,
            network_year: 476.76,
            tax_reduction_year: 628.96,
            ..Default::default()
        }
    }

    fn terms_2027() -> EnergyTerms {
        EnergyTerms { netting: false, feed_in: 0.065, feed_in_cost_month: 0.0, feed_in_cost_kwh: 0.0475, ..terms() }
    }

    fn september() -> UsagePeriod {
        UsagePeriod {
            start: "2026-08-30".into(),
            end: "2026-09-29".into(),
            used_normal: 66.0,
            used_low: 60.0,
            returned_normal: 132.0,
            returned_low: 51.0,
        }
    }

    #[test]
    fn matches_the_suppliers_report() {
        let p = september();
        assert_eq!(p.days(), 31);
        let c = terms().period_cost(&p);
        assert_eq!(c.usage_cents, 3379); // the report: 33,83 at a slightly different average rate
        assert_eq!(c.return_cents, 4907);
        // 8,14 + 40,49 - 53,42 + 14,27 feed-in costs (14 a month), rounded once
        assert_eq!(c.fixed_cents, 949);
        assert_eq!(c.total_cents, c.usage_cents - c.return_cents + c.fixed_cents);
    }

    #[test]
    fn after_salderen_returns_are_worth_little() {
        let t = terms_2027();
        let c = t.period_cost(&september());
        // 183 kWh × (0,065 × 1,21 − 0,0475) = 183 × 0,03115
        assert!((t.feed_in_net() - 0.03115).abs() < 1e-9);
        assert_eq!(c.return_cents, 570);
        assert_eq!(c.fixed_cents, -478); // 8,14 + 40,49 - 53,42, rounded once
    }

    #[test]
    fn surplus_gets_the_low_compensation() {
        let t = terms();
        let pos = t.netting_position(&[september()]);
        assert_eq!(pos.surplus, 57.0);
        // 57 × (0,26815 − 0,0121)
        assert_eq!(pos.surplus_loss_cents, 1459);
    }

    #[test]
    fn gas_matches_the_suppliers_report() {
        let gas = EnergyTerms {
            utility: Utility::Gas,
            supply_normal: 0.6655,
            energy_tax: 0.7268,
            fixed_month: 8.99,
            network_year: 274.44,
            ..Default::default()
        };
        let p = UsagePeriod { start: "2026-08-30".into(), end: "2026-09-29".into(), used_normal: 26.0, ..Default::default() };
        let c = gas.period_cost(&p);
        assert_eq!(c.usage_cents, 3620); // 26 m³ × 1,3923
        assert_eq!(c.fixed_cents, 916 + 2331); // the report: 9,16 and 23,30
    }

    #[test]
    fn months_spread_scale_and_estimate() {
        let gas = EnergyTerms { utility: Utility::Gas, supply_normal: 1.0, ..Default::default() };
        let period = |s: &str, e: &str, m3: f64| UsagePeriod { start: s.into(), end: e.into(), used_normal: m3, ..Default::default() };
        let last_year = Contract {
            start: "2025-01-01".into(),
            end: "2025-12-31".into(),
            energy: Some(gas.clone()),
            usage: vec![period("2025-10-01", "2025-10-31", 50.0)],
            ..Default::default()
        };
        let this_year = Contract {
            start: "2026-08-30".into(),
            monthly_cents: 14_000,
            energy: Some(EnergyTerms { supply_normal: 2.0, ..gas }),
            // 30 Aug – 29 Sep: 2 days in August, 29 in September.
            usage: vec![period("2026-08-30", "2026-09-29", 31.0)],
            ..Default::default()
        };
        let months = energy_months(&[last_year, this_year], 2026);
        // August: 2 of 31 days covered, scaled to the month: 2 m³ × €2 × 31/2.
        assert_eq!(months[7].gas_cents, 6200);
        // September: 29 of 30 days covered, scaled: 58 × 30/29.
        assert_eq!(months[8].gas_cents, 6000);
        assert!(!months[8].gas_estimated);
        // October: none, so last October's 50 m³ at this year's €2.
        assert_eq!(months[9].gas_cents, 10_000);
        assert!(months[9].gas_estimated);
        assert_eq!(months[9].payment_cents, 14_000);
        assert_eq!(months[9].balance_cents(), 4_000);
        // November: no data last year either.
        assert_eq!(months[10].gas_cents, 0);
        assert!(!months[10].gas_estimated);
    }

    #[test]
    fn bills_land_in_the_month_paid() {
        let c = Contract {
            settlements: vec![
                Settlement { date: "2026-01-16".into(), cents: 52_759, note: "jaarnota".into() },
                Settlement { date: "2026-07-27".into(), cents: -12_596, note: "eindnota".into() },
                Settlement { date: "2025-01-10".into(), cents: 1, note: String::new() },
            ],
            ..Default::default()
        };
        let months = energy_months(&[c], 2026);
        assert_eq!((months[0].bill_cents, months[6].bill_cents), (52_759, -12_596));
        assert_eq!(months.iter().map(|m| m.bill_cents).sum::<i64>(), 52_759 - 12_596);
    }

    #[test]
    fn expected_bill_settles_split_contracts_together() {
        let gas = EnergyTerms { utility: Utility::Gas, supply_normal: 1.0, ..Default::default() };
        let period = |s: &str, e: &str, m3: f64| UsagePeriod { start: s.into(), end: e.into(), used_normal: m3, ..Default::default() };
        let a = Contract {
            start: "2026-07-01".into(),
            end: "2026-12-31".into(),
            monthly_cents: 10_000,
            bill_due: "2027-07-01".into(),
            energy: Some(gas.clone()),
            usage: vec![period("2026-07-01", "2026-07-31", 60.0)],
            ..Default::default()
        };
        let b = Contract { start: "2027-01-01".into(), end: "2027-06-30".into(), usage: vec![], ..a.clone() };
        let months = energy_months(&[a, b], 2027);
        // July 2026 to June 2027: 12 × € 100 paid. July 2026 cost € 60; the other
        // months have no usage, and the months without it a year earlier stay 0.
        assert_eq!(months[6].bill_expected_cents, 12 * 10_000 - 6_000);
        assert!(months.iter().enumerate().all(|(i, m)| i == 6 || m.bill_expected_cents == 0));
    }

    #[test]
    fn contracts_run_on_and_estimates_look_further_back() {
        let gas = EnergyTerms { utility: Utility::Gas, supply_normal: 1.0, ..Default::default() };
        let c = Contract {
            start: "2025-06-30".into(),
            end: "2026-06-30".into(),
            monthly_cents: 14_000,
            energy: Some(gas),
            usage: vec![UsagePeriod { start: "2025-10-01".into(), end: "2025-10-31".into(), used_normal: 50.0, ..Default::default() }],
            ..Default::default()
        };
        let months = energy_months(&[c], 2027);
        // After its end the contract runs on: payments and tariffs continue.
        assert_eq!(months[9].payment_cents, 14_000);
        // October 2026 has no usage either, so October 2025 is used.
        assert_eq!(months[9].gas_cents, 5_000);
        assert!(months[9].gas_estimated);
    }

    #[test]
    fn surplus_settled_in_the_last_month() {
        let mut t = terms();
        (t.fixed_month, t.network_year, t.tax_reduction_year, t.feed_in_cost_month) = (0.0, 0.0, 0.0, 0.0);
        let c = Contract {
            start: "2026-08-01".into(),
            end: "2026-12-31".into(),
            energy: Some(t),
            usage: vec![UsagePeriod { start: "2026-08-01".into(), end: "2026-08-31".into(), used_normal: 100.0, returned_normal: 157.0, ..Default::default() }],
            ..Default::default()
        };
        let months = energy_months(&[c], 2026);
        // August: 57 kWh net return at the full price.
        assert_eq!(months[7].electricity_cents, -1528);
        assert_eq!(months[7].settlement_cents, 0);
        // December: those 57 kWh back at 0,0121 instead of 0,26815.
        assert_eq!(months[11].settlement_cents, 1459);
        assert_eq!(months[11].electricity_cents, 1459);
    }

    #[test]
    fn day_numbers() {
        assert_eq!(day_number("2026-03-01").unwrap() - day_number("2026-02-28").unwrap(), 1);
        assert_eq!(day_number("2024-03-01").unwrap() - day_number("2024-02-28").unwrap(), 2);
        assert_eq!(day_number("2027-01-01").unwrap() - day_number("2026-01-01").unwrap(), 365);
        assert!(day_number("2026-13-01").is_none());
        assert_eq!(add_days("2026-09-29", 1), "2026-09-30");
        assert_eq!(add_days("2026-12-31", 1), "2027-01-01");
        assert_eq!(add_days("2024-02-28", 1), "2024-02-29");
        assert_eq!(add_days("2026-03-01", -1), "2026-02-28");
    }
}
