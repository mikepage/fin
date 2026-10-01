//! Thin wrappers around Tauri's `invoke`. Every mutation returns the full dataset.

use fin_shared::{
    Account, BackupInfo, Category, CategoryKind, Contract, CopyResult, Dataset, ImportFile, ImportProgress, ImportResult, Lang,
    RuleInfo, RuleKind, RuleResult, TransactionInput,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"], catch)]
    async fn invoke(cmd: &str, args: JsValue) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"])]
    type Channel;
    #[wasm_bindgen(constructor, js_namespace = ["window", "__TAURI__", "core"])]
    fn new() -> Channel;
    #[wasm_bindgen(method, setter)]
    fn set_onmessage(this: &Channel, f: &js_sys::Function);
}

fn js_err(e: JsValue) -> String {
    e.as_string().unwrap_or_else(|| format!("{e:?}"))
}

async fn call_raw<R: DeserializeOwned>(cmd: &str, args: JsValue) -> Result<R, String> {
    let v = invoke(cmd, args).await.map_err(js_err)?;
    serde_wasm_bindgen::from_value(v).map_err(|e| e.to_string())
}

async fn call<A: Serialize, R: DeserializeOwned>(cmd: &str, args: &A) -> Result<R, String> {
    let args = serde_wasm_bindgen::to_value(args).map_err(|e| e.to_string())?;
    call_raw(cmd, args).await
}

#[derive(Serialize)]
struct NoArgs {}

#[derive(Serialize)]
struct Id<'a> {
    id: &'a str,
}

pub async fn get_data() -> Result<Dataset, String> {
    call("get_data", &NoArgs {}).await
}

pub async fn set_language(lang: Lang) -> Result<Dataset, String> {
    #[derive(Serialize)]
    struct A {
        lang: Lang,
    }
    call("set_language", &A { lang }).await
}

pub async fn add_account(name: &str, iban: Option<String>) -> Result<Dataset, String> {
    #[derive(Serialize)]
    struct A<'a> {
        name: &'a str,
        iban: Option<String>,
    }
    call("add_account", &A { name, iban }).await
}

pub async fn update_account(account: Account) -> Result<Dataset, String> {
    #[derive(Serialize)]
    struct A {
        account: Account,
    }
    call("update_account", &A { account }).await
}

pub async fn delete_account(id: &str) -> Result<Dataset, String> {
    call("delete_account", &Id { id }).await
}

pub async fn add_category(name: &str, group: &str, kind: CategoryKind) -> Result<Dataset, String> {
    #[derive(Serialize)]
    struct A<'a> {
        name: &'a str,
        group: &'a str,
        kind: CategoryKind,
    }
    call("add_category", &A { name, group, kind }).await
}

pub async fn update_category(category: Category) -> Result<Dataset, String> {
    #[derive(Serialize)]
    struct A {
        category: Category,
    }
    call("update_category", &A { category }).await
}

pub async fn delete_category(id: &str) -> Result<Dataset, String> {
    call("delete_category", &Id { id }).await
}

pub async fn save_transaction(input: TransactionInput) -> Result<Dataset, String> {
    #[derive(Serialize)]
    struct A {
        input: TransactionInput,
    }
    call("save_transaction", &A { input }).await
}

pub async fn delete_transaction(id: &str) -> Result<Dataset, String> {
    call("delete_transaction", &Id { id }).await
}

/// Splits a transaction into parts (amount without sign, category); empty removes it.
/// Sets the bank balance of an account at the end of `date`; `None` removes it.
pub async fn set_account_balance(id: &str, date: &str, cents: Option<i64>) -> Result<Dataset, String> {
    #[derive(Serialize)]
    struct A<'a> {
        id: &'a str,
        date: &'a str,
        cents: Option<i64>,
    }
    call("set_account_balance", &A { id, date, cents }).await
}

/// Adds a contract (empty id) or replaces it.
pub async fn save_contract(contract: Contract) -> Result<Dataset, String> {
    #[derive(Serialize)]
    struct A {
        contract: Contract,
    }
    call("save_contract", &A { contract }).await
}

pub async fn delete_contract(id: &str) -> Result<Dataset, String> {
    call("delete_contract", &Id { id }).await
}

pub async fn set_splits(id: &str, parts: Vec<(i64, String)>) -> Result<Dataset, String> {
    #[derive(Serialize)]
    struct A<'a> {
        id: &'a str,
        parts: Vec<(i64, String)>,
    }
    call("set_splits", &A { id, parts }).await
}

/// The day a transaction counts on; `None` goes back to its bank date.
pub async fn set_counts_on(id: &str, day: Option<&str>) -> Result<Dataset, String> {
    #[derive(Serialize)]
    struct A<'a> {
        id: &'a str,
        day: Option<&'a str>,
    }
    call("set_counts_on", &A { id, day }).await
}

pub async fn undo_import(id: &str) -> Result<Dataset, String> {
    call("undo_import", &Id { id }).await
}

pub async fn list_backups() -> Result<Vec<BackupInfo>, String> {
    call("list_backups", &NoArgs {}).await
}

pub async fn create_backup() -> Result<Vec<BackupInfo>, String> {
    call("create_backup", &NoArgs {}).await
}

pub async fn restore_backup(name: &str) -> Result<Dataset, String> {
    #[derive(Serialize)]
    struct A<'a> {
        name: &'a str,
    }
    call("restore_backup", &A { name }).await
}

pub async fn restore_backup_file(json: String) -> Result<Dataset, String> {
    #[derive(Serialize)]
    struct A {
        json: String,
    }
    call("restore_backup_file", &A { json }).await
}

/// Returns the path of the written copy.
pub async fn export_backup() -> Result<String, String> {
    call("export_backup", &NoArgs {}).await
}

/// Saves `content` as `name`.csv in Downloads; returns the path.
pub async fn export_csv(name: String, content: String) -> Result<String, String> {
    #[derive(Serialize)]
    struct A {
        name: String,
        content: String,
    }
    call("export_csv", &A { name, content }).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RuleArgs<'a> {
    category_id: &'a str,
    kind: RuleKind,
    value: &'a str,
}

/// Every rule, with whether it is a (locked) default.
pub async fn list_rules() -> Result<Vec<RuleInfo>, String> {
    call("list_rules", &NoArgs {}).await
}

/// Adds an IBAN or text rule; fills in matching transactions that are still to be sorted.
pub async fn add_category_rule(category_id: &str, kind: RuleKind, value: &str) -> Result<RuleResult, String> {
    call("add_category_rule", &RuleArgs { category_id, kind, value }).await
}

/// Recategorises every transaction the rule matches from `from` (`YYYY-MM-DD`) on.
pub async fn apply_category_rule(category_id: &str, kind: RuleKind, value: &str, from: &str) -> Result<RuleResult, String> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct A<'a> {
        category_id: &'a str,
        kind: RuleKind,
        value: &'a str,
        from: &'a str,
    }
    call("apply_category_rule", &A { category_id, kind, value, from }).await
}

pub async fn remove_category_rule(category_id: &str, kind: RuleKind, value: &str) -> Result<Dataset, String> {
    call("remove_category_rule", &RuleArgs { category_id, kind, value }).await
}

/// `None` removes the budget.
pub async fn set_budget(category_id: String, month: String, amount_cents: Option<i64>) -> Result<Dataset, String> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct A {
        category_id: String,
        month: String,
        amount_cents: Option<i64>,
    }
    call("set_budget", &A { category_id, month, amount_cents }).await
}

/// The budget of a group's fixed or variable categories together. `None` removes it.
pub async fn set_group_budget(group: String, kind: CategoryKind, month: String, amount_cents: Option<i64>) -> Result<Dataset, String> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct A {
        group: String,
        kind: CategoryKind,
        month: String,
        amount_cents: Option<i64>,
    }
    call("set_group_budget", &A { group, kind, month, amount_cents }).await
}

pub async fn copy_budgets_from_previous_year(year: i32) -> Result<CopyResult, String> {
    #[derive(Serialize)]
    struct A {
        year: i32,
    }
    call("copy_budgets_from_previous_year", &A { year }).await
}

pub async fn budgets_from_average(year: i32, last_month: &str, months: u32, overwrite: bool) -> Result<CopyResult, String> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct A<'a> {
        year: i32,
        last_month: &'a str,
        months: u32,
        overwrite: bool,
    }
    call("budgets_from_average", &A { year, last_month, months, overwrite }).await
}

pub async fn copy_budget_month_to_next(month: &str) -> Result<Dataset, String> {
    #[derive(Serialize)]
    struct A<'a> {
        month: &'a str,
    }
    call("copy_budget_month_to_next", &A { month }).await
}

/// Runs the import, calling `on_progress` for every progress message from the backend.
pub async fn import_camt053(
    account_id: &str,
    files: &[ImportFile],
    on_progress: impl Fn(ImportProgress) + 'static,
) -> Result<ImportResult, String> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct A<'a> {
        account_id: &'a str,
        files: &'a [ImportFile],
    }
    let args = serde_wasm_bindgen::to_value(&A { account_id, files }).map_err(|e| e.to_string())?;

    let channel = Channel::new();
    let cb = Closure::<dyn Fn(JsValue)>::new(move |msg: JsValue| {
        if let Ok(p) = serde_wasm_bindgen::from_value::<ImportProgress>(msg) {
            on_progress(p);
        }
    });
    channel.set_onmessage(cb.as_ref().unchecked_ref());
    js_sys::Reflect::set(&args, &"onProgress".into(), &channel).map_err(js_err)?;

    // Channel messages can arrive after the invoke resolves; a dropped closure would
    // throw then. One small closure per import is an acceptable leak.
    cb.forget();
    call_raw("import_camt053", args).await
}
