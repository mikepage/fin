//! The standard categories: stable ids, their group and kind. Names are not here; they
//! come from a locale mapping (src-tauri/locales/nl-NL.toml), so the ids never depend
//! on a language and stay the same in every data file (sync-friendly).

use crate::CategoryKind::{self, Fixed as F, Income as I, Transfer as T, Variable as V};

/// Group keys. A locale maps each to a display name.
pub mod groups {
    pub const INCOME: &str = "income";
    pub const HOUSING: &str = "housing";
    pub const HOUSEHOLD: &str = "household";
    pub const MEDICAL: &str = "medical";
    pub const INSURANCE: &str = "insurance";
    pub const FINANCES: &str = "finances";
    pub const TELECOM: &str = "telecom";
    pub const SUBSCRIPTIONS: &str = "subscriptions";
    pub const TRANSPORT: &str = "transport";
    pub const EDUCATION: &str = "education";
    pub const CLOTHING: &str = "clothing";
    pub const LEISURE: &str = "leisure";
    pub const OTHER_EXPENSES: &str = "other_expenses";
    pub const UNCATEGORISED: &str = "uncategorised";
}

/// Category ids: `sys-` plus the key a locale maps to a display name.
pub mod ids {
    // Income
    pub const SALARY: &str = "sys-salary";
    pub const BENEFITS: &str = "sys-benefits";
    pub const PENSION: &str = "sys-pension";
    pub const STUDY_ALLOWANCE: &str = "sys-study_allowance";
    pub const SIDE_INCOME: &str = "sys-side_income";
    pub const HOLIDAY_PAY: &str = "sys-holiday_pay";
    pub const BONUSES: &str = "sys-bonuses";
    pub const TAX_REFUND: &str = "sys-tax_refund";
    pub const CHILD_BENEFIT: &str = "sys-child_benefit";
    pub const ALIMONY_RECEIVED: &str = "sys-alimony_received";
    pub const TAX_ALLOWANCES: &str = "sys-tax_allowances";
    pub const EXPENSE_CLAIMS: &str = "sys-expense_claims";
    pub const INTEREST_RECEIVED: &str = "sys-interest_received";
    pub const INVESTMENT_INCOME: &str = "sys-investment_income";
    pub const OTHER_INCOME: &str = "sys-other_income";
    /// Year-end settlements and refunds, kept out of the cost categories they came from.
    pub const REFUNDS: &str = "sys-refunds";
    // Housing
    pub const RENT_MORTGAGE: &str = "sys-rent_mortgage";
    pub const SERVICE_COSTS: &str = "sys-service_costs";
    pub const UTILITIES: &str = "sys-utilities";
    pub const HOME_IMPROVEMENT: &str = "sys-home_improvement";
    pub const MUNICIPAL_TAXES: &str = "sys-municipal_taxes";
    // Household
    pub const GROCERIES: &str = "sys-groceries";
    pub const CASH_WITHDRAWALS: &str = "sys-cash_withdrawals";
    pub const CHILDREN: &str = "sys-children";
    pub const HOUSEHOLD_OTHER: &str = "sys-household_other";
    pub const PERSONAL_CARE: &str = "sys-personal_care";
    pub const CHARITY: &str = "sys-charity";
    pub const SHOPPING: &str = "sys-shopping";
    // Medical
    pub const DEDUCTIBLE: &str = "sys-deductible";
    pub const MEDICAL_OTHER: &str = "sys-medical_other";
    // Insurance
    pub const INSURANCE: &str = "sys-insurance";
    pub const HEALTH_INSURANCE: &str = "sys-health_insurance";
    pub const CAR_INSURANCE: &str = "sys-car_insurance";
    pub const MOPED_BIKE_INSURANCE: &str = "sys-moped_bike_insurance";
    // Finances
    pub const ROAD_TAX: &str = "sys-road_tax";
    pub const CAR_PURCHASE_LEASE: &str = "sys-car_purchase_lease";
    pub const DEBT_REPAYMENT: &str = "sys-debt_repayment";
    pub const BANK_FEES: &str = "sys-bank_fees";
    pub const ALIMONY_PAID: &str = "sys-alimony_paid";
    // Telecom
    pub const INTERNET: &str = "sys-internet";
    pub const LANDLINE: &str = "sys-landline";
    pub const MOBILE: &str = "sys-mobile";
    // Subscriptions (streaming, apps, hosting)
    pub const SUBSCRIPTIONS: &str = "sys-subscriptions";
    // Transport
    pub const FUEL: &str = "sys-fuel";
    pub const VEHICLE_MAINTENANCE: &str = "sys-vehicle_maintenance";
    pub const PARKING_TOLLS: &str = "sys-parking_tolls";
    pub const PUBLIC_TRANSPORT: &str = "sys-public_transport";
    // Education
    pub const SCHOOL_COSTS: &str = "sys-school_costs";
    pub const COURSES: &str = "sys-courses";
    // Clothing
    pub const CLOTHES: &str = "sys-clothes";
    // Leisure
    pub const HOLIDAYS: &str = "sys-holidays";
    pub const OUTINGS: &str = "sys-outings";
    pub const GIFTS: &str = "sys-gifts";
    pub const SPORT: &str = "sys-sport";
    // Other expenses
    pub const INTERNAL_TRANSFERS: &str = "sys-internal_transfers";
    pub const SAVINGS_INVESTMENTS: &str = "sys-savings_investments";
    pub const FINES: &str = "sys-fines";
    pub const OVERDRAFT_INTEREST: &str = "sys-overdraft_interest";
    pub const CREDIT_CARD: &str = "sys-credit_card";
    pub const TAXES: &str = "sys-taxes";
    pub const OTHER_TRANSFERS: &str = "sys-other_transfers";
    pub const PAYMENT_REQUESTS: &str = "sys-payment_requests";
    pub const OTHER_EXPENSES: &str = "sys-other_expenses";
    // Uncategorised
    pub const UNSORTED: &str = "sys-unsorted";
}

use groups as g;
use ids::*;

/// Every standard category in display order: (id, group key, kind).
pub const CATALOG: &[(&str, &str, CategoryKind)] = &[
    (SALARY, g::INCOME, I),
    (BENEFITS, g::INCOME, I),
    (PENSION, g::INCOME, I),
    (STUDY_ALLOWANCE, g::INCOME, I),
    (SIDE_INCOME, g::INCOME, I),
    // Often spent on something one-off (a holiday, an investment): not budgeted.
    (HOLIDAY_PAY, g::INCOME, CategoryKind::IrregularIncome),
    (BONUSES, g::INCOME, I),
    (TAX_REFUND, g::INCOME, I),
    (CHILD_BENEFIT, g::INCOME, I),
    (ALIMONY_RECEIVED, g::INCOME, I),
    (TAX_ALLOWANCES, g::INCOME, I),
    (EXPENSE_CLAIMS, g::INCOME, I),
    // Income you can't plan: counted, but not budgeted.
    (INTEREST_RECEIVED, g::INCOME, CategoryKind::IrregularIncome),
    (INVESTMENT_INCOME, g::INCOME, CategoryKind::IrregularIncome),
    (OTHER_INCOME, g::INCOME, CategoryKind::IrregularIncome),
    (REFUNDS, g::INCOME, CategoryKind::IrregularIncome),
    (RENT_MORTGAGE, g::HOUSING, F),
    (SERVICE_COSTS, g::HOUSING, F),
    (UTILITIES, g::HOUSING, F),
    (HOME_IMPROVEMENT, g::HOUSING, CategoryKind::Investment),
    (MUNICIPAL_TAXES, g::HOUSING, F),
    (GROCERIES, g::HOUSEHOLD, V),
    (CASH_WITHDRAWALS, g::HOUSEHOLD, V),
    (CHILDREN, g::HOUSEHOLD, V),
    (HOUSEHOLD_OTHER, g::HOUSEHOLD, V),
    (PERSONAL_CARE, g::HOUSEHOLD, V),
    (CHARITY, g::HOUSEHOLD, V),
    (SHOPPING, g::HOUSEHOLD, V),
    (DEDUCTIBLE, g::MEDICAL, V),
    (MEDICAL_OTHER, g::MEDICAL, V),
    (INSURANCE, g::INSURANCE, F),
    (HEALTH_INSURANCE, g::INSURANCE, F),
    (CAR_INSURANCE, g::INSURANCE, F),
    (MOPED_BIKE_INSURANCE, g::INSURANCE, F),
    (ROAD_TAX, g::FINANCES, F),
    (CAR_PURCHASE_LEASE, g::FINANCES, F),
    (DEBT_REPAYMENT, g::FINANCES, F),
    (BANK_FEES, g::FINANCES, F),
    (ALIMONY_PAID, g::FINANCES, F),
    (INTERNET, g::TELECOM, F),
    (LANDLINE, g::TELECOM, F),
    (MOBILE, g::TELECOM, F),
    (SUBSCRIPTIONS, g::SUBSCRIPTIONS, F),
    (FUEL, g::TRANSPORT, V),
    (VEHICLE_MAINTENANCE, g::TRANSPORT, V),
    (PARKING_TOLLS, g::TRANSPORT, V),
    (PUBLIC_TRANSPORT, g::TRANSPORT, V),
    (SCHOOL_COSTS, g::EDUCATION, F),
    (COURSES, g::EDUCATION, F),
    (CLOTHES, g::CLOTHING, V),
    (HOLIDAYS, g::LEISURE, V),
    (OUTINGS, g::LEISURE, V),
    (GIFTS, g::LEISURE, V),
    (SPORT, g::LEISURE, V),
    (INTERNAL_TRANSFERS, g::OTHER_EXPENSES, T),
    (SAVINGS_INVESTMENTS, g::OTHER_EXPENSES, V),
    (FINES, g::OTHER_EXPENSES, V),
    (OVERDRAFT_INTEREST, g::OTHER_EXPENSES, V),
    (CREDIT_CARD, g::OTHER_EXPENSES, V),
    (TAXES, g::OTHER_EXPENSES, V),
    (OTHER_TRANSFERS, g::OTHER_EXPENSES, V),
    (PAYMENT_REQUESTS, g::OTHER_EXPENSES, V),
    (OTHER_EXPENSES, g::OTHER_EXPENSES, V),
    (UNSORTED, g::UNCATEGORISED, V),
];

/// The key a locale uses for a category id (`sys-groceries` → `groceries`).
/// The group key of a standard category (None for the user's own categories).
pub fn group_of(id: &str) -> Option<&'static str> {
    CATALOG.iter().find(|(c, _, _)| *c == id).map(|(_, g, _)| *g)
}

pub fn key(id: &str) -> &str {
    id.strip_prefix("sys-").unwrap_or(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fixed costs are budgeted per category and variable costs per group, so a group
    /// holds one or the other, never both.
    #[test]
    fn no_group_mixes_fixed_and_variable() {
        for &(_, group, _) in CATALOG {
            let has = |k: CategoryKind| CATALOG.iter().any(|&(_, g, kind)| g == group && kind == k);
            assert!(!(has(F) && has(V)), "group {group} has fixed and variable categories");
        }
    }
}
