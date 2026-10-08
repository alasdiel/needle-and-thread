//! Spike 3: is Leptos + SVG fast enough for the network map (DESIGN §7)? A map of made-up
//! nodes and lines drawn like Look 4, where a node can be dragged and the map panned and
//! zoomed, timed in the real webview. Mounted instead of the app when the URL's hash is
//! `#network-spike` (or `#network-spike-500` for 500 nodes).

use std::cell::RefCell;
use std::rc::Rc;

use leptos::prelude::*;
use wasm_bindgen::JsCast;

const KINDS: [(&str, &str, &str); 4] = [
    ("character", "var(--swatch-print)", r#"<circle cx="12" cy="8" r="4"/><path d="M5 21v-1.5a5 5 0 0 1 5-5h4a5 5 0 0 1 5 5V21"/>"#),
    ("place", "#e0b26a", r#"<path d="M20 10c0 6-8 12-8 12s-8-6-8-12a8 8 0 0 1 16 0Z"/><circle cx="12" cy="10" r="3"/>"#),
    ("plot point", "#f08fb0", r#"<path d="M4 15s1-1 4-1 5 2 8 2 4-1 4-1V3s-1 1-4 1-5-2-8-2-4 1-4 1z"/><path d="M4 22v-7"/>"#),
    ("thread", "var(--thread)", r#"<path d="M6 3h12M6 21h12"/><path d="M8 3v18M16 3v18"/><path d="M8 7l8 3M8 11l8 3M8 15l8 3"/>"#),
];
const LABELS: [&str; 5] = ["trusts", "betrays", "causes", "mentor of", "owes"];

#[derive(Clone, Copy)]
struct Node {
    x: RwSignal<f64>,
    y: RwSignal<f64>,
}

#[derive(Clone, Copy)]
struct View {
    x: RwSignal<f64>,
    y: RwSignal<f64>,
    zoom: RwSignal<f64>,
}

#[derive(Clone, Copy)]
enum Grab {
    Node { index: usize, dx: f64, dy: f64 },
    Pan { from_x: f64, from_y: f64, x: f64, y: f64 },
}

/// A tiny repeatable random sequence, so every run draws the same map.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 33) as f64 / (1u64 << 31) as f64
    }
}

/// The node count, and whether to move the map as a composited layer (a CSS transform on the
/// `<svg>`) instead of an SVG transform: `#network-spike-500-layer`.
pub fn wanted() -> Option<(usize, bool)> {
    let hash = web_sys::window()?.location().hash().ok()?;
    let rest = hash.strip_prefix("#network-spike")?;
    let layer = rest.ends_with("-layer");
    let rest = rest.trim_end_matches("-layer");
    Some((rest.strip_prefix('-').and_then(|n| n.parse().ok()).unwrap_or(200), layer))
}

#[component]
pub fn NetworkSpike(count: usize, layer: bool) -> impl IntoView {
    let columns = (count as f64 * 2.0).sqrt().ceil() as usize;
    let mut random = Lcg(7);
    let nodes: Vec<Node> = (0..count)
        .map(|i| Node {
            x: RwSignal::new((i % columns) as f64 * 168.0 + random.next() * 60.0),
            y: RwSignal::new((i / columns) as f64 * 144.0 + random.next() * 50.0),
        })
        .collect();
    let mut edges = Vec::new();
    for i in 0..count {
        if (i + 1) % columns != 0 && i + 1 < count {
            edges.push((i, i + 1));
        }
        if i + columns < count && random.next() < 0.55 {
            edges.push((i, i + columns));
        }
    }
    let nodes = StoredValue::new(nodes);
    let view_at = View { x: RwSignal::new(40.0), y: RwSignal::new(40.0), zoom: RwSignal::new(0.5) };
    let grab = StoredValue::new(None::<Grab>);
    let report = RwSignal::new(String::from("Drag a node, drag the mat to pan, scroll to zoom."));

    let to_map = move |cx: f64, cy: f64| {
        let z = view_at.zoom.get_untracked();
        ((cx - view_at.x.get_untracked()) / z, (cy - view_at.y.get_untracked()) / z)
    };

    let on_down = move |ev: web_sys::PointerEvent| {
        let (cx, cy) = (ev.client_x() as f64, ev.client_y() as f64);
        let target = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok());
        if target.as_ref().is_some_and(|el| el.closest(".spike-panel").ok().flatten().is_some()) {
            return;
        }
        let hit = target
            .and_then(|el| el.closest("[data-node]").ok().flatten())
            .and_then(|el| el.get_attribute("data-node"))
            .and_then(|i| i.parse::<usize>().ok());
        let new = match hit {
            Some(index) => {
                let (mx, my) = to_map(cx, cy);
                let n = nodes.with_value(|n| n[index]);
                Grab::Node { index, dx: n.x.get_untracked() - mx, dy: n.y.get_untracked() - my }
            }
            None => Grab::Pan { from_x: cx, from_y: cy, x: view_at.x.get_untracked(), y: view_at.y.get_untracked() },
        };
        grab.set_value(Some(new));
        if let Some(el) = ev.current_target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) {
            let _ = el.set_pointer_capture(ev.pointer_id());
        }
    };
    let on_move = move |ev: web_sys::PointerEvent| {
        let (cx, cy) = (ev.client_x() as f64, ev.client_y() as f64);
        match grab.get_value() {
            Some(Grab::Node { index, dx, dy }) => {
                let (mx, my) = to_map(cx, cy);
                let n = nodes.with_value(|n| n[index]);
                n.x.set(mx + dx);
                n.y.set(my + dy);
            }
            Some(Grab::Pan { from_x, from_y, x, y }) => {
                view_at.x.set(x + cx - from_x);
                view_at.y.set(y + cy - from_y);
            }
            None => {}
        }
    };
    let on_up = move |_: web_sys::PointerEvent| grab.set_value(None);
    let on_wheel = move |ev: web_sys::WheelEvent| {
        ev.prevent_default();
        zoom_at(view_at, ev.client_x() as f64, ev.client_y() as f64, (-ev.delta_y() * 0.0015).exp());
    };

    let edge_views = edges
        .iter()
        .enumerate()
        .map(|(e, &(a, b))| {
            let (a, b) = nodes.with_value(|n| (n[a], n[b]));
            let relationship = e % 4 == 0;
            let label = relationship.then(|| {
                let text = LABELS[e % LABELS.len()];
                let mid_x = move || (a.x.get() + b.x.get()) / 2.0;
                let mid_y = move || (a.y.get() + b.y.get()) / 2.0;
                view! {
                    <g class="spike-label" transform=move || format!("translate({:.1} {:.1})", mid_x(), mid_y())>
                        <rect x=-34 y=-10 width=68 height=20 rx=4></rect>
                        <text y=4>{text}</text>
                    </g>
                }
            });
            view! {
                <line
                    class=if relationship { "spike-relationship" } else { "spike-auto" }
                    x1=move || a.x.get()
                    y1=move || a.y.get()
                    x2=move || b.x.get()
                    y2=move || b.y.get()
                ></line>
                {label}
            }
        })
        .collect_view();
    let node_views = (0..count)
        .map(|i| {
            let n = nodes.with_value(|n| n[i]);
            let (kind, colour, icon) = KINDS[i % KINDS.len()];
            let shape = if kind == "plot point" {
                view! { <path class="spike-tile" d="M0,-25 L25,0 L0,25 L-25,0 Z" style:stroke=colour></path> }.into_any()
            } else {
                view! { <rect class="spike-tile" x=-23 y=-23 width=46 height=46 rx=10 style:stroke=colour></rect> }.into_any()
            };
            view! {
                <g data-node=i transform=move || format!("translate({:.1} {:.1})", n.x.get(), n.y.get())>
                    {shape}
                    <g transform="translate(-11 -11) scale(0.9167)" class="spike-icon" style:stroke=colour inner_html=icon></g>
                    <text class="spike-title" y=42>{format!("Note {}", i + 1)}</text>
                    <text class="spike-kind" y=56>{kind}</text>
                </g>
            }
        })
        .collect_view();

    // An SVG transform repaints the map each frame; a CSS one on the <svg> can move it as a layer.
    let at = move || {
        let (x, y, z) = (view_at.x.get(), view_at.y.get(), view_at.zoom.get());
        if layer { format!("translate({x:.1}px, {y:.1}px) scale({z:.4})") } else { format!("translate({x:.1} {y:.1}) scale({z:.4})") }
    };
    let (width, height) = if layer {
        (format!("{}", columns as f64 * 168.0 + 120.0), format!("{}", (count / columns + 1) as f64 * 144.0 + 120.0))
    } else {
        ("100%".into(), "100%".into())
    };
    let run = move |test: Test| {
        report.set(format!("Running {}…", test.name()));
        let times = Rc::new(RefCell::new(Vec::with_capacity(FRAMES + 1)));
        step(test, 0, view_at, nodes, times, report);
    };

    view! {
        <div class="spike" on:pointerdown=on_down on:pointermove=on_move on:pointerup=on_up on:wheel=on_wheel>
            <style>{STYLE}</style>
            <svg
                class="spike-map"
                class:layer=layer
                width=width
                height=height
                style:transform=move || layer.then(at)
            >
                <g transform=move || (!layer).then(at)>
                    <g>{edge_views}</g>
                    <g>{node_views}</g>
                </g>
            </svg>
            <div class="spike-panel">
                <div>{format!("{count} nodes, {} lines, {}", edges.len(), if layer { "CSS layer" } else { "SVG transform" })}</div>
                <button on:click=move |_| run(Test::Drag)>"Time a drag"</button>
                <button on:click=move |_| run(Test::Pan)>"Time a pan"</button>
                <button on:click=move |_| run(Test::Zoom)>"Time a zoom"</button>
                <div>
                    {["200", "200-layer", "500", "500-layer"]
                        .map(|v| view! { <button on:click=move |_| switch_to(v)>{v}</button> })}
                </div>
                <pre>{move || report.get()}</pre>
            </div>
        </div>
    }
}

fn switch_to(variant: &str) {
    let location = web_sys::window().unwrap().location();
    let _ = location.set_hash(&format!("network-spike-{variant}"));
    let _ = location.reload();
}

fn zoom_at(view_at: View, cx: f64, cy: f64, factor: f64) {
    let z = view_at.zoom.get_untracked();
    let next = (z * factor).clamp(0.1, 4.0);
    let scale = next / z;
    view_at.x.set(cx - (cx - view_at.x.get_untracked()) * scale);
    view_at.y.set(cy - (cy - view_at.y.get_untracked()) * scale);
    view_at.zoom.set(next);
}

const FRAMES: usize = 180;

const STYLE: &str = "
.spike { position: fixed; inset: 0; background: var(--mat, #0e1820); }
.spike { touch-action: none; cursor: grab; overflow: hidden; }
.spike-map { display: block; overflow: visible; }
.spike-map:not(.layer) { width: 100%; height: 100%; }
.spike-map.layer { transform-origin: 0 0; will-change: transform; }
.spike-auto { stroke: #4e6573; stroke-width: 1.2; stroke-dasharray: 4 4; }
.spike-relationship { stroke: #e4eef0; stroke-width: 2; }
.spike-label rect { fill: #0a1218; stroke: rgb(143 166 178 / 0.3); }
.spike-label text { fill: #e4eef0; font: 13px 'Alegreya Sans', sans-serif; text-anchor: middle; }
.spike-tile { fill: #1a2833; stroke-width: 2; }
.spike-icon { fill: none; stroke-width: 1.8; stroke-linecap: round; stroke-linejoin: round; }
.spike-title { fill: #e4eef0; font: 500 14px 'Alegreya Sans', sans-serif; text-anchor: middle; }
.spike-kind { fill: #8fa6b2; font: 500 11px 'Alegreya SC', serif; text-anchor: middle; letter-spacing: 0.07em; }
[data-node] { cursor: move; }
.spike-panel { position: fixed; right: 16px; bottom: 16px; display: grid; gap: 6px; padding: 12px; background: #0a1218; color: #e4eef0; border: 1px solid #333; font: 14px 'Alegreya Sans', sans-serif; }
.spike-panel pre { margin: 0; font: 13px monospace; white-space: pre-wrap; width: 360px; height: 5em; }
";

#[derive(Clone, Copy)]
enum Test {
    Drag,
    Pan,
    Zoom,
}

impl Test {
    fn name(self) -> &'static str {
        match self {
            Self::Drag => "drag",
            Self::Pan => "pan",
            Self::Zoom => "zoom",
        }
    }
}

/// One frame of a test: note the time, move something, and ask for the next frame. The gaps
/// between frames are what the webview took to lay out and paint the change.
fn step(test: Test, i: usize, view_at: View, nodes: StoredValue<Vec<Node>>, times: Rc<RefCell<Vec<f64>>>, report: RwSignal<String>) {
    request_animation_frame(move || {
        times.borrow_mut().push(js_sys::Date::now());
        if i == FRAMES {
            report.set(summary(test, &times.borrow()));
            return;
        }
        let t = i as f64 / FRAMES as f64 * std::f64::consts::TAU;
        match test {
            Test::Drag => {
                let n = nodes.with_value(|n| n[n.len() / 2 + 3]);
                n.x.update(|x| *x += 6.0 * t.cos());
                n.y.update(|y| *y += 6.0 * t.sin());
            }
            Test::Pan => {
                view_at.x.update(|x| *x += 8.0 * t.cos());
                view_at.y.update(|y| *y += 8.0 * t.sin());
            }
            Test::Zoom => zoom_at(view_at, 600.0, 400.0, if i < FRAMES / 2 { 1.012 } else { 1.0 / 1.012 }),
        }
        step(test, i + 1, view_at, nodes, times, report);
    });
}

fn summary(test: Test, times: &[f64]) -> String {
    let mut gaps: Vec<f64> = times.windows(2).map(|w| w[1] - w[0]).collect();
    let total = times.last().unwrap() - times.first().unwrap();
    gaps.sort_by(f64::total_cmp);
    let at = |p: f64| gaps[((gaps.len() - 1) as f64 * p).round() as usize];
    format!(
        "{}: {} frames in {:.0} ms, {:.0} fps\nframe gap median {:.1} ms, p95 {:.1} ms, worst {:.1} ms",
        test.name(),
        gaps.len(),
        total,
        gaps.len() as f64 * 1000.0 / total,
        at(0.5),
        at(0.95),
        gaps.last().unwrap(),
    )
}
