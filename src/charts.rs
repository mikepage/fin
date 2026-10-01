//! Report charts as plain SVG. Colors come from CSS classes (strict CSP: no inline
//! style attributes), geometry from a fixed viewBox that scales with the panel.

use fin_shared::MonthFlow;
use leptos::prelude::*;

use crate::i18n;

const W: f64 = 960.0;
const H: f64 = 260.0;
const LEFT: f64 = 72.0;
const RIGHT: f64 = 12.0;
const TOP: f64 = 16.0;
const BOTTOM: f64 = 32.0;

fn month_short(month: &str) -> String {
    let m: usize = month.get(5..7).and_then(|s| s.parse().ok()).unwrap_or(1);
    i18n::month_short(m.max(1) - 1).to_string()
}

/// Axis labels: whole euros, `€ 1.500` (`€ 1,500` in English).
fn axis_euro(cents: i64) -> String {
    format!("€ {}", i18n::whole(cents - cents % 100))
}

/// A tick step (in cents) giving about four intervals: 1, 2, 2.5 or 5 × 10ⁿ euros.
fn nice_step(range_cents: i64) -> i64 {
    let raw = (range_cents.max(100) as f64 / 100.0) / 4.0;
    let mag = 10f64.powf(raw.log10().floor());
    let step = [1.0, 2.0, 2.5, 5.0, 10.0].into_iter().map(|f| f * mag).find(|s| *s >= raw).unwrap_or(10.0 * mag);
    (step * 100.0).round() as i64
}

/// Value axis covering `lo..=hi` (lo <= 0 <= hi), extended to whole steps.
fn axis(lo: i64, hi: i64) -> (i64, i64, i64) {
    let step = nice_step(hi - lo);
    let lo = if lo < 0 { -((-lo + step - 1) / step) * step } else { 0 };
    let hi = ((hi.max(1) + step - 1) / step) * step;
    (lo, hi, step)
}

/// Column with a 4px rounded data end; square at the baseline. `up` = grows upward.
fn column(x: f64, base: f64, w: f64, len: f64, up: bool) -> String {
    let r = 4f64.min(len).min(w / 2.0);
    if up {
        let top = base - len;
        format!(
            "M{x},{base}V{}Q{x},{top} {},{top}H{}Q{},{top} {},{}V{base}Z",
            top + r,
            x + r,
            x + w - r,
            x + w,
            x + w,
            top + r
        )
    } else {
        let bot = base + len;
        format!(
            "M{x},{base}V{}Q{x},{bot} {},{bot}H{}Q{},{bot} {},{}V{base}Z",
            bot - r,
            x + r,
            x + w - r,
            x + w,
            x + w,
            bot - r
        )
    }
}

struct Frame {
    lo: i64,
    hi: i64,
    step: i64,
    band: f64,
}

impl Frame {
    fn new(lo: i64, hi: i64, n: usize) -> Self {
        let (lo, hi, step) = axis(lo, hi);
        Frame { lo, hi, step, band: (W - LEFT - RIGHT) / n.max(1) as f64 }
    }
    fn y(&self, cents: i64) -> f64 {
        TOP + (self.hi - cents) as f64 / (self.hi - self.lo) as f64 * (H - TOP - BOTTOM)
    }
    fn band_x(&self, i: usize) -> f64 {
        LEFT + i as f64 * self.band
    }
    /// Horizontal hairline grid with euro labels; the zero line is a step darker.
    fn grid(&self) -> impl IntoView {
        let mut ticks = Vec::new();
        let mut v = self.lo;
        while v <= self.hi {
            ticks.push(v);
            v += self.step;
        }
        ticks
            .into_iter()
            .map(|t| {
                let y = self.y(t);
                view! {
                    <line class=if t == 0 { "axis-zero" } else { "grid" } x1=LEFT x2=W - RIGHT y1=y y2=y></line>
                    <text class="tick" x=LEFT - 8.0 y=y + 4.0 text-anchor="end">{axis_euro(t)}</text>
                }
            })
            .collect_view()
    }
    /// Month names under the bands; the year is added at January and at the first band.
    fn months(&self, flows: &[MonthFlow]) -> impl IntoView {
        flows
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let label = if i == 0 || f.month.ends_with("-01") {
                    format!("{} '{}", month_short(&f.month), &f.month[2..4])
                } else {
                    month_short(&f.month)
                };
                view! {
                    <text class="tick" x=self.band_x(i) + self.band / 2.0 y=H - 10.0 text-anchor="middle">{label}</text>
                }
            })
            .collect_view()
    }
}

/// Transparent full-band hit areas; hover and keyboard focus show the tooltip.
fn hit_areas(frame: &Frame, n: usize, hover: RwSignal<Option<usize>>) -> impl IntoView {
    (0..n)
        .map(|i| {
            view! {
                <rect
                    class="hit"
                    x=frame.band_x(i)
                    y=TOP
                    width=frame.band
                    height=H - TOP - BOTTOM
                    tabindex="0"
                    on:mouseenter=move |_| hover.set(Some(i))
                    on:focus=move |_| hover.set(Some(i))
                    on:mouseleave=move |_| hover.set(None)
                    on:blur=move |_| hover.set(None)
                ></rect>
            }
        })
        .collect_view()
}

/// How a report's bars are colored: one series color, or blue/orange by sign.
#[derive(Clone, Copy, PartialEq)]
pub enum Tone {
    Expense,
    Income,
    Signed,
}

/// One value per month for a year; `None` marks months still to come (no bar).
#[derive(Clone, PartialEq)]
pub struct MonthPoint {
    pub month: String,
    pub cents: Option<i64>,
    /// Expected value for a month still to come (drawn lighter).
    pub forecast: Option<i64>,
    /// Expected value at the current pace of spending, for a month still to come.
    pub pace: Option<i64>,
}

impl MonthPoint {
    fn value(&self) -> Option<i64> {
        self.cents.or(self.forecast)
    }
}

/// The running total through the year, actual then forecast, per month.
/// The running total at the current pace: actual months, then the pace values.
fn pace_totals(pts: &[MonthPoint]) -> Vec<Option<i64>> {
    let mut sum = 0;
    pts.iter()
        .map(|p| {
            p.cents.or(p.pace).map(|v| {
                sum += v;
                sum
            })
        })
        .collect()
}

fn running_totals(pts: &[MonthPoint]) -> Vec<Option<i64>> {
    let mut sum = 0;
    pts.iter()
        .map(|p| {
            p.value().map(|v| {
                sum += v;
                sum
            })
        })
        .collect()
}

/// Twelve monthly columns with a hairline at the average of the months that have a
/// value, for spotting trends. Months still to come can carry a forecast (lighter
/// bars); with `running` a line shows the running total through the year. Hover or
/// focus shows the month's value.
#[component]
pub fn MonthlyBarChart(
    points: Signal<Vec<MonthPoint>>,
    tone: Signal<Tone>,
    label: Signal<String>,
    #[prop(into, optional)] running: Signal<bool>,
    /// Bars only: no line at the average.
    #[prop(into, optional)] plain: Signal<bool>,
) -> impl IntoView {
    let hover = RwSignal::new(None::<usize>);
    let average = move || {
        points.with(|p| {
            let vals: Vec<i64> = p.iter().filter_map(|x| x.cents).collect();
            (!vals.is_empty()).then(|| vals.iter().sum::<i64>() / vals.len() as i64)
        })
    };
    let svg = move || {
        let pts = points.get();
        let t = tone.get();
        let run = running.get();
        let totals = if run { running_totals(&pts) } else { Vec::new() };
        let has_pace = run && pts.iter().any(|p| p.pace.is_some());
        let paced = if has_pace { pace_totals(&pts) } else { Vec::new() };
        let vals = pts.iter().filter_map(MonthPoint::value).chain(totals.iter().flatten().copied()).chain(paced.iter().flatten().copied());
        let lo = vals.clone().min().unwrap_or(0).min(0);
        let hi = vals.max().unwrap_or(0).max(0);
        let frame = Frame::new(lo, hi, pts.len());
        let bw = (frame.band * 0.5).min(24.0);
        let base = frame.y(0);
        let flows: Vec<MonthFlow> =
            pts.iter().map(|p| MonthFlow { month: p.month.clone(), income_cents: 0, expense_cents: 0 }).collect();
        let bars = pts
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let x = frame.band_x(i) + (frame.band - bw) / 2.0;
                let active = move || hover.get() == Some(i);
                let is_forecast = p.cents.is_none();
                let bar = p.value().filter(|c| *c != 0).map(|c| {
                    let len = (frame.y(0) - frame.y(c.abs())).abs();
                    let class = match t {
                        Tone::Expense => "bar expense",
                        Tone::Income => "bar income",
                        Tone::Signed if c > 0 => "bar pos",
                        Tone::Signed => "bar neg",
                    };
                    let class = if is_forecast { format!("{class} forecast") } else { class.to_string() };
                    view! { <path class=class d={column(x, base, bw, len, c > 0)}></path> }
                });
                view! { <g class="month" class:active=active>{bar}</g> }
            })
            .collect_view();
        // The running total: a line through the month centres, dashed where forecast.
        let run_line = run.then(|| {
            let pt = |i: usize, v: i64| format!("{:.1},{:.1}", frame.band_x(i) + frame.band / 2.0, frame.y(v));
            let actual: Vec<String> = totals.iter().enumerate().filter(|(i, _)| pts[*i].cents.is_some()).filter_map(|(i, v)| v.map(|v| pt(i, v))).collect();
            let last_actual = pts.iter().rposition(|p| p.cents.is_some());
            let ahead: Vec<String> = totals
                .iter()
                .enumerate()
                .filter(|(i, _)| last_actual.is_none_or(|l| *i >= l))
                .filter_map(|(i, v)| v.map(|v| pt(i, v)))
                .collect();
            let end = totals.iter().rposition(|v| v.is_some()).and_then(|i| totals[i].map(|v| (i, v)));
            // The same from the last actual month at the current pace of spending.
            let pace_ahead: Vec<String> = paced
                .iter()
                .enumerate()
                .filter(|(i, _)| last_actual.is_none_or(|l| *i >= l))
                .filter_map(|(i, v)| v.map(|v| pt(i, v)))
                .collect();
            let pace_end = paced.iter().rposition(|v| v.is_some()).and_then(|i| paced[i].map(|v| (i, v)));
            view! {
                <polyline class="running" points=actual.join(" ")></polyline>
                {(ahead.len() > 1).then(|| view! { <polyline class="running forecast" points=ahead.join(" ")></polyline> })}
                {(pace_ahead.len() > 1).then(|| view! { <polyline class="running pace" points=pace_ahead.join(" ")></polyline> })}
                {pace_end.filter(|(i, _)| pts[*i].cents.is_none()).map(|(i, v)| view! {
                    <text class="running-label pace" x=frame.band_x(i) + frame.band / 2.0 y=frame.y(v) + 18.0 text-anchor="end">
                        {format!("{} {}", t!("at pace"), axis_euro(v))}
                    </text>
                })}
                {end.map(|(i, v)| view! {
                    <text class="running-label" x=frame.band_x(i) + frame.band / 2.0 y=frame.y(v) - 8.0 text-anchor="end">
                        {format!("{} {}", if pts[i].cents.is_some() { t!("total") } else { t!("expected") }, axis_euro(v))}
                    </text>
                })}
            }
        });
        let avg_line = average().filter(|a| *a != 0 && !plain.get()).map(|a| {
            let y = frame.y(a);
            view! {
                <line class="avg" x1=LEFT x2=W - RIGHT y1=y y2=y></line>
                <text class="avg-label" x=W - RIGHT y=y - 6.0 text-anchor="end">{t!("avg. {}", axis_euro(a))}</text>
            }
        });
        view! {
            <svg class="chart" viewBox=format!("0 0 {W} {H}") role="img" aria-label=label.get()>
                {frame.grid()}
                {bars}
                {avg_line}
                {run_line}
                {frame.months(&flows)}
                {hit_areas(&frame, pts.len(), hover)}
            </svg>
        }
    };
    let tip = move || {
        let i = hover.get()?;
        let p = points.with(|p| p.get(i).cloned())?;
        let n = points.with(Vec::len).max(1);
        let left_pct = (LEFT + (i as f64 + 0.5) * (W - LEFT - RIGHT) / n as f64) / W * 100.0;
        let value = match (p.cents, p.forecast) {
            (Some(c), _) => i18n::euro(c),
            (None, Some(f)) => format!("{} {}", t!("expected"), i18n::euro(f)),
            (None, None) => t!("not yet").to_string(),
        };
        let total = running.get().then(|| points.with(|p| running_totals(p)[i])).flatten();
        let pace_total = (running.get() && p.pace.is_some()).then(|| points.with(|p| pace_totals(p)[i])).flatten();
        Some(view! {
            <div class="chart-tip" class:flip={left_pct > 70.0} style:left=format!("{left_pct:.1}%")>
                <strong>{crate::app::month_label(&p.month)}</strong>
                <span class="row">{label.get()}<b>{value}</b></span>
                {p.pace.map(|v| view! { <span class="row">{t!("At current pace")}<b>{i18n::euro(v)}</b></span> })}
                {total.map(|t| view! { <span class="row">{t!("Total through this month")}<b>{i18n::euro(t)}</b></span> })}
                {pace_total.map(|t| view! { <span class="row">{t!("Total at current pace")}<b>{i18n::euro(t)}</b></span> })}
            </div>
        })
    };
    view! {
        <div class="chart-wrap">
            {svg}
            {tip}
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axis_steps_are_round() {
        assert_eq!(nice_step(400_000), 100_000); // € 4.000 range → € 1.000 steps
        assert_eq!(nice_step(900_000), 250_000);
        assert_eq!(axis(-120_000, 300_000), (-200_000, 400_000, 200_000));
        assert_eq!(axis(0, 0).0, 0);
        assert_eq!(axis_euro(150_000), "€ 1.500");
    }
}
