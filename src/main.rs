// First, so `t!` and `tn!` are in scope in the other modules.
#[macro_use]
mod i18n;
mod api;
mod app;
mod charts;
mod combo;

use app::*;
use leptos::prelude::*;

fn main() {
    console_error_panic_hook::set_once();
    mount_to_body(|| {
        view! {
            <App/>
        }
    })
}
