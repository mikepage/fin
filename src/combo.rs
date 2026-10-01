//! One dropdown for every choice in the app, in place of the native `<select>` (whose
//! macOS popup looks nothing like the rest). A button shows the choice and opens a
//! list under it: grouped options, an optional icon, a check on the current one, and
//! a search field once the list is long. Keyboard as a select: arrows, Home/End,
//! Enter, Escape, type a letter to jump. Enter and Escape on a closed box bubble up,
//! so a surrounding editor can still save or cancel with them.
//!
//! The list is `position: fixed`, placed from the button's rectangle through CSSOM
//! (`style:` props, allowed by the strict CSP), so no panel or dialog clips it.

use std::sync::atomic::{AtomicUsize, Ordering};

use leptos::{ev, prelude::*};
use wasm_bindgen::JsCast;

#[derive(Clone)]
pub struct ComboOption {
    pub value: String,
    pub label: String,
    /// Heading the option is listed under; consecutive options share one.
    pub group: Option<String>,
    pub icon: Option<ViewFn>,
}

impl ComboOption {
    pub fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        Self { value: value.into(), label: label.into(), group: None, icon: None }
    }

    pub fn in_group(mut self, group: impl Into<String>) -> Self {
        self.group = Some(group.into());
        self
    }

    pub fn with_icon(mut self, icon: impl Into<ViewFn>) -> Self {
        self.icon = Some(icon.into());
        self
    }
}

/// Longer lists get a search field.
const SEARCH_FROM: usize = 10;
/// Tallest the list gets, and the room below the button it wants before opening upwards.
const MAX_HEIGHT: f64 = 360.0;
const MIN_BELOW: f64 = 220.0;

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy, Default, PartialEq)]
struct Placement {
    left: Option<f64>,
    right: Option<f64>,
    top: Option<f64>,
    bottom: Option<f64>,
    min_width: f64,
    max_height: f64,
}

#[derive(Clone, Copy)]
enum Start {
    Selected,
    First,
    Last,
}

fn px(v: Option<f64>) -> String {
    v.map_or_else(|| "auto".to_string(), |v| format!("{v:.0}px"))
}

/// A key that types a character (not a shortcut).
fn typed_char(ev: &ev::KeyboardEvent) -> Option<String> {
    let k = ev.key();
    (k.chars().count() == 1 && !ev.ctrl_key() && !ev.meta_key() && !ev.alt_key()).then_some(k)
}

#[component]
pub fn ComboBox(
    #[prop(into)] options: Signal<Vec<ComboOption>>,
    /// The chosen option's value; shows the placeholder when no option has it.
    #[prop(into)]
    value: Signal<String>,
    /// Called with the new value, only when it changes.
    #[prop(into)]
    on_change: Callback<String>,
    /// Accessible name, for screen readers (the visible label is around it).
    #[prop(into)]
    label: String,
    #[prop(optional, into)] placeholder: Option<String>,
    #[prop(optional, into)] title: Option<String>,
    #[prop(optional, into)] disabled: MaybeProp<bool>,
    #[prop(optional)] autofocus: bool,
    /// A text field instead of a button: anything typed is the value, and the list
    /// suggests the options that match (as a <datalist> would).
    #[prop(optional)]
    free_text: bool,
) -> impl IntoView {
    let uid = StoredValue::new(format!("combo{}", NEXT_ID.fetch_add(1, Ordering::Relaxed)));
    let list_id = uid.with_value(|u| format!("{u}-list"));
    let opt_id = move |pos: usize| uid.with_value(|u| format!("{u}-{pos}"));

    let root = NodeRef::<leptos::html::Div>::new();
    let button = NodeRef::<leptos::html::Button>::new();
    let field = NodeRef::<leptos::html::Input>::new();
    let search = NodeRef::<leptos::html::Input>::new();
    let list = NodeRef::<leptos::html::Div>::new();

    let open = RwSignal::new(false);
    let query = RwSignal::new(String::new());
    // Position in `shown` of the highlighted option.
    let active = RwSignal::new(None::<usize>);
    let place = RwSignal::new(Placement::default());

    // A free-text field is its own search.
    let searchable = move || !free_text && options.with(|o| o.len() > SEARCH_FROM);
    // Indices into `options` of what the search leaves; a group name matches all its options.
    let shown = Memo::new(move |_| {
        let q = query.get().trim().to_lowercase();
        options.with(|o| {
            o.iter()
                .enumerate()
                .filter(|(_, x)| {
                    q.is_empty()
                        || x.label.to_lowercase().contains(&q)
                        || x.group.as_deref().is_some_and(|g| g.to_lowercase().contains(&q))
                })
                .map(|(i, _)| i)
                .collect::<Vec<_>>()
        })
    });
    let is_disabled = move || disabled.get().unwrap_or(false);

    let reveal = move |pos: usize| {
        if let Some(el) = document().get_element_by_id(&opt_id(pos)) {
            let opts = web_sys::ScrollIntoViewOptions::new();
            opts.set_block(web_sys::ScrollLogicalPosition::Nearest);
            el.scroll_into_view_with_scroll_into_view_options(&opts);
        }
    };
    let set_active = move |pos: Option<usize>| {
        active.set(pos);
        if let Some(p) = pos {
            reveal(p);
        }
    };
    // The button, or the text field.
    let focus_control = move || {
        if let Some(f) = field.get_untracked() {
            let _ = f.focus();
        } else if let Some(b) = button.get_untracked() {
            let _ = b.focus();
        }
    };

    // Under the box, or above it when there is more room there; right-aligned when
    // the box sits in the right part of the window.
    let measure = move || {
        let Some(b) = root.get_untracked() else { return };
        let r = b.get_bounding_client_rect();
        let w = window();
        let vw = w.inner_width().ok().and_then(|v| v.as_f64()).unwrap_or(1200.0);
        let vh = w.inner_height().ok().and_then(|v| v.as_f64()).unwrap_or(800.0);
        let (gap, margin) = (6.0, 8.0);
        let below = vh - r.bottom() - gap - margin;
        let above = r.top() - gap - margin;
        let up = below < MIN_BELOW && above > below;
        let right_side = r.left() > vw / 2.0;
        place.set(Placement {
            left: (!right_side).then(|| r.left()),
            right: right_side.then(|| vw - r.right()),
            top: (!up).then(|| r.bottom() + gap),
            bottom: up.then(|| vh - r.top() + gap),
            min_width: r.width(),
            max_height: if up { above } else { below }.clamp(120.0, MAX_HEIGHT),
        });
    };

    let close = move |refocus: bool| {
        open.set(false);
        query.set(String::new());
        active.set(None);
        if refocus {
            focus_control();
        }
    };
    let open_at = move |start: Start| {
        if is_disabled() || open.get_untracked() {
            return;
        }
        measure();
        if free_text {
            // Suggestions for what is typed; the option it equals, if any, highlighted.
            query.set(value.get_untracked());
            let exact = shown.with_untracked(|s| {
                options.with_untracked(|o| value.with_untracked(|v| s.iter().position(|&i| o[i].value == *v)))
            });
            active.set(exact);
        } else {
            query.set(String::new());
            let n = options.with_untracked(Vec::len);
            let selected = options.with_untracked(|o| value.with_untracked(|v| o.iter().position(|x| x.value == *v)));
            active.set(match start {
                _ if n == 0 => None,
                Start::Selected => selected.or(Some(0)),
                Start::First => Some(0),
                Start::Last => Some(n - 1),
            });
        }
        open.set(true);
        // WebKit does not focus a clicked button: focus it, so keys arrive. The search
        // field takes the focus once it is drawn (the effects below).
        focus_control();
    };
    // As soon as the list is drawn: the search field gets the focus, the current
    // option comes into view.
    Effect::new(move |_| {
        if let Some(s) = search.get() {
            let _ = s.focus();
        }
    });
    Effect::new(move |_| {
        if list.get().is_some() {
            if let Some(p) = active.get_untracked() {
                reveal(p);
            }
        }
    });
    let choose = move |pos: usize| {
        let Some(v) = shown.with_untracked(|s| s.get(pos).copied()).and_then(|i| options.with_untracked(|o| o.get(i).map(|x| x.value.clone())))
        else {
            return;
        };
        close(true);
        if value.with_untracked(|cur| *cur != v) {
            on_change.run(v);
        }
    };
    let step = move |delta: isize| {
        let n = shown.with_untracked(Vec::len);
        if n == 0 {
            return;
        }
        let next = match active.get_untracked() {
            None if delta > 0 => 0,
            None => n - 1,
            Some(a) => (a as isize + delta).clamp(0, n as isize - 1) as usize,
        };
        set_active(Some(next));
    };
    // Type a letter: the next option starting with it.
    let jump = move |ch: &str| {
        let ch = ch.to_lowercase();
        let n = shown.with_untracked(Vec::len);
        let from = active.get_untracked().map_or(0, |a| a + 1);
        let hit = (0..n).map(|k| (from + k) % n).find(|&p| {
            shown.with_untracked(|s| options.with_untracked(|o| o[s[p]].label.to_lowercase().starts_with(&ch)))
        });
        if hit.is_some() {
            set_active(hit);
        }
    };

    let on_key = move |ev: ev::KeyboardEvent, in_search: bool| {
        let key = ev.key();
        if !open.get_untracked() && free_text {
            // Everything else types, and Enter submits the form around it.
            if matches!(key.as_str(), "ArrowDown" | "ArrowUp") {
                open_at(Start::Selected);
                ev.prevent_default();
            }
            return;
        }
        if !open.get_untracked() {
            match key.as_str() {
                "ArrowDown" | "ArrowUp" | " " => open_at(Start::Selected),
                "Home" => open_at(Start::First),
                "End" => open_at(Start::Last),
                // No toggle on the button; the key still bubbles (an inline editor saves).
                "Enter" => {}
                _ => match typed_char(&ev) {
                    Some(c) if searchable() => {
                        open_at(Start::Selected);
                        query.set(c);
                        active.set(shown.with_untracked(|s| (!s.is_empty()).then_some(0)));
                    }
                    Some(c) => {
                        open_at(Start::Selected);
                        jump(&c);
                    }
                    None => return,
                },
            }
            ev.prevent_default();
            return;
        }
        match key.as_str() {
            "ArrowDown" => step(1),
            "ArrowUp" => step(-1),
            "PageDown" => step(10),
            "PageUp" => step(-10),
            "Home" if !in_search => set_active(Some(0)),
            "End" if !in_search => set_active(shown.with_untracked(|s| s.len().checked_sub(1))),
            "Enter" => match active.get_untracked() {
                Some(p) => choose(p),
                // Nothing highlighted: keep what is typed, and let the form submit.
                None if free_text => return close(false),
                None => close(true),
            },
            " " if !in_search => {
                if let Some(p) = active.get_untracked() {
                    choose(p);
                }
            }
            "Escape" => close(true),
            "Tab" => return close(false),
            _ => match typed_char(&ev) {
                // Typed before the search field has the focus: goes into the search.
                Some(c) if !in_search && searchable() => {
                    query.update(|q| q.push_str(&c));
                    active.set(shown.with_untracked(|s| (!s.is_empty()).then_some(0)));
                }
                Some(c) if !in_search => jump(&c),
                _ => return,
            },
        }
        // Handled here: not also by a surrounding editor or dialog.
        ev.prevent_default();
        ev.stop_propagation();
    };

    // Close on a press outside, and when the page scrolls or resizes (the list is fixed).
    let outside = window_event_listener(ev::pointerdown, move |ev| {
        if !open.get_untracked() {
            return;
        }
        let inside = root.get_untracked().zip(ev.target().and_then(|t| t.dyn_into::<web_sys::Node>().ok()));
        if !inside.is_some_and(|(r, t)| r.contains(Some(&t))) {
            close(false);
        }
    });
    let scrolled = window_event_listener(ev::scroll, move |_| {
        if open.get_untracked() {
            close(false);
        }
    });
    let resized = window_event_listener(ev::resize, move |_| {
        if open.get_untracked() {
            close(false);
        }
    });
    on_cleanup(move || {
        outside.remove();
        scrolled.remove();
        resized.remove();
    });

    if autofocus {
        Effect::new(move |_| {
            if button.get().is_some() || field.get().is_some() {
                focus_control();
            }
        });
    }

    let field_placeholder = placeholder.clone();
    let current = move || {
        let placeholder = placeholder.clone();
        options.with(|o| match value.with(|v| o.iter().find(|x| x.value == *v)) {
            Some(x) => view! {
                {x.icon.as_ref().map(ViewFn::run)}
                <span class="combo-value">{x.label.clone()}</span>
            }
            .into_any(),
            None => view! { <span class="combo-value placeholder">{placeholder.unwrap_or_default()}</span> }.into_any(),
        })
    };

    let option_view = move |x: &ComboOption, pos: usize, selected: bool| {
        view! {
            <div
                class="combo-option"
                id=opt_id(pos)
                role="option"
                aria-selected=selected.to_string()
                class:selected=selected
                class:active=move || active.get() == Some(pos)
                on:pointermove=move |_| {
                    if active.get_untracked() != Some(pos) {
                        active.set(Some(pos));
                    }
                }
                on:click=move |ev| {
                    // Inside a <label>, the click would also open the list again.
                    ev.prevent_default();
                    choose(pos);
                }
            >
                {x.icon.as_ref().map(ViewFn::run)}
                <span class="combo-label">{x.label.clone()}</span>
                <svg class="combo-check" width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                    <path d="M3 8.5 6.5 12 13 4.5"></path>
                </svg>
            </div>
        }
    };
    // Options in their groups, each group under its heading.
    let items = move || {
        options.with(|o| {
            shown.with(|s| {
                let mut groups: Vec<(Option<String>, Vec<(usize, usize)>)> = Vec::new();
                for (pos, &i) in s.iter().enumerate() {
                    match groups.last_mut() {
                        Some((g, list)) if *g == o[i].group => list.push((pos, i)),
                        _ => groups.push((o[i].group.clone(), vec![(pos, i)])),
                    }
                }
                value.with(|v| {
                    groups
                        .into_iter()
                        .enumerate()
                        .map(|(gi, (g, list))| {
                            let list = list.into_iter().map(|(pos, i)| option_view(&o[i], pos, o[i].value == *v)).collect_view();
                            match g {
                                Some(g) => {
                                    let hid = uid.with_value(|u| format!("{u}-g{gi}"));
                                    let labelled_by = hid.clone();
                                    view! {
                                        <div role="group" aria-labelledby=labelled_by>
                                            <div class="combo-group" id=hid>{g}</div>
                                            {list}
                                        </div>
                                    }
                                    .into_any()
                                }
                                None => list.into_any(),
                            }
                        })
                        .collect_view()
                })
            })
        })
    };

    let search_label = StoredValue::new(t!("Search {}", label.to_lowercase()));
    let list_label = StoredValue::new(label.clone());
    let controls = list_id.clone();
    let list_id = StoredValue::new(list_id);
    let control = if free_text {
        view! {
            <input
                class="combo-input"
                node_ref=field
                role="combobox"
                aria-autocomplete="list"
                aria-label=label
                aria-controls=controls
                aria-expanded=move || open.get().to_string()
                aria-activedescendant=move || open.get().then(|| active.get().map(opt_id)).flatten()
                autocomplete="off"
                placeholder=field_placeholder
                title=title
                disabled=is_disabled
                prop:value=move || value.get()
                on:input=move |ev| {
                    let text = event_target_value(&ev);
                    on_change.run(text.clone());
                    if open.get_untracked() {
                        query.set(text);
                    } else {
                        open_at(Start::Selected);
                    }
                    active.set(None);
                }
                on:keydown=move |ev| on_key(ev, true)
            />
            <span
                class="combo-arrow"
                aria-hidden="true"
                on:mousedown=|ev| ev.prevent_default()
                on:click=move |ev| {
                    ev.prevent_default();
                    if open.get_untracked() { close(true) } else { open_at(Start::Selected) }
                }
            >
                "▾"
            </span>
        }
        .into_any()
    } else {
        view! {
            <button
                type="button"
                class="combo-button"
                node_ref=button
                role="combobox"
                aria-haspopup="listbox"
                aria-label=label
                aria-controls=controls
                aria-expanded=move || open.get().to_string()
                aria-activedescendant=move || {
                    (open.get() && !searchable()).then(|| active.get().map(opt_id)).flatten()
                }
                title=title
                disabled=is_disabled
                on:click=move |_| if open.get_untracked() { close(true) } else { open_at(Start::Selected) }
                on:keydown=move |ev| on_key(ev, false)
            >
                {current}
                <span class="combo-arrow" aria-hidden="true">"▾"</span>
            </button>
        }
        .into_any()
    };
    view! {
        <div
            class="combo"
            class:free=free_text
            class:open=move || open.get()
            node_ref=root
            on:focusout=move |ev: ev::FocusEvent| {
                // Tabbing away; a click elsewhere is handled by the pointer listener.
                let to = ev.related_target().and_then(|t| t.dyn_into::<web_sys::Node>().ok());
                if open.get_untracked() && to.is_some_and(|t| !root.get_untracked().is_some_and(|r| r.contains(Some(&t)))) {
                    close(false);
                }
            }
        >
            {control}
            // A free-text field shows no list when nothing matches: what is typed is fine.
            <Show when=move || open.get() && !(free_text && shown.with(Vec::is_empty))>
                <div
                    class="combo-pop"
                    class:up=move || place.with(|p| p.bottom.is_some())
                    style:left=move || px(place.get().left)
                    style:right=move || px(place.get().right)
                    style:top=move || px(place.get().top)
                    style:bottom=move || px(place.get().bottom)
                    style:min-width=move || format!("{:.0}px", place.get().min_width)
                    style:max-height=move || format!("{:.0}px", place.get().max_height)
                    // Inside a <label>, a click on a heading would also click the button.
                    on:click=|ev| ev.prevent_default()
                >
                    <Show when=searchable>
                        <input
                            type="search"
                            class="combo-search"
                            node_ref=search
                            placeholder=t!("Search…")
                            role="combobox"
                            aria-label=search_label.get_value()
                            aria-autocomplete="list"
                            aria-expanded="true"
                            aria-controls=list_id.get_value()
                            aria-activedescendant=move || active.get().map(opt_id)
                            prop:value=move || query.get()
                            on:input=move |ev| {
                                query.set(event_target_value(&ev));
                                active.set(shown.with_untracked(|s| (!s.is_empty()).then_some(0)));
                            }
                            on:keydown=move |ev| on_key(ev, true)
                        />
                    </Show>
                    <div
                        class="combo-list"
                        id=list_id.get_value()
                        role="listbox"
                        node_ref=list
                        aria-label=list_label.get_value()
                        // Keep the focus on the button or the search field.
                        on:mousedown=|ev| ev.prevent_default()
                    >
                        {items}
                        <Show when=move || shown.with(Vec::is_empty)>
                            <div class="combo-empty">{t!("Nothing found")}</div>
                        </Show>
                    </div>
                </div>
            </Show>
        </div>
    }
}
