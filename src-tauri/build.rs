// Declaring the app's commands makes them subject to capabilities, so each one
// must be allowed explicitly in capabilities/default.json.
const COMMANDS: &[&str] = &[
    "get_data",
    "set_language",
    "add_account",
    "update_account",
    "delete_account",
    "add_category",
    "update_category",
    "delete_category",
    "add_category_rule",
    "apply_category_rule",
    "remove_category_rule",
    "list_rules",
    "save_transaction",
    "delete_transaction",
    "set_counts_on",
    "set_splits",
    "set_account_balance",
    "save_contract",
    "delete_contract",
    "import_camt053",
    "undo_import",
    "list_backups",
    "create_backup",
    "restore_backup",
    "restore_backup_file",
    "export_backup",
    "export_csv",
    "set_budget",
    "set_group_budget",
    "copy_budgets_from_previous_year",
    "copy_budget_month_to_next",
    "budgets_from_average",
];

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to run tauri-build")
}
