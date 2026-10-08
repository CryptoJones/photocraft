//! The Color Picker (Foreground / Background Color) dialog: a 2D colour field and a slider
//! for the selected component (H, S, B, R, G, B, L, a or b radio), new/current swatches beside OK
//! and Cancel, and HSB, RGB, Lab, CMYK and hex fields. The colour lives in the dialog fields (`color` as `#rrggbb`, plus the HSB
//! floats so hue survives greys), so `ui.dialog.set` drives it; OK runs `tools.setColors`.
//! While it is the top dialog the image is its eyedropper, as in Photoshop: the pointer over the
//! canvas is a pipette, and a click or drag there samples into the new colour ([`sample_at`]).

use egui::{Color32, Mesh, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
use serde_json::{Map, Value, json};

use crate::PhotocraftApp;
use crate::state::DialogKind;
use crate::theme::Tokens;
use crate::widgets;

/// The colour model a radio component belongs to.
#[derive(Clone, Copy)]
enum Space {
    Hsv,
    Rgb,
    Lab,
}

/// A radio component: the slider sets component `z` of `space`, the field `x` across and `y` up.
struct Mode {
    key: &'static str,
    label: &'static str,
    space: Space,
    z: usize,
    x: usize,
    y: usize,
}

const fn mode(key: &'static str, label: &'static str, space: Space, z: usize, x: usize, y: usize) -> Mode {
    Mode { key, label, space, z, x, y }
}

/// Radio components in dialog order: H, S, B, R, G, B, then L, a, b.
const MODES: &[Mode] = &[
    mode("h", "H:", Space::Hsv, 0, 1, 2),
    mode("s", "S:", Space::Hsv, 1, 0, 2),
    mode("v", "B:", Space::Hsv, 2, 0, 1),
    mode("r", "R:", Space::Rgb, 0, 2, 1),
    mode("g", "G:", Space::Rgb, 1, 2, 0),
    mode("b", "B:", Space::Rgb, 2, 0, 1),
    mode("lab-l", "L:", Space::Lab, 0, 1, 2),
    mode("lab-a", "a:", Space::Lab, 1, 2, 0),
    mode("lab-b", "b:", Space::Lab, 2, 1, 0),
];

impl Space {
    /// Range of component `i`.
    fn range(self, i: usize) -> (f32, f32) {
        match (self, i) {
            (Self::Hsv, 0) => (0.0, 360.0),
            (Self::Lab, 0) => (0.0, 100.0),
            (Self::Lab, _) => (-128.0, 127.0),
            _ => (0.0, 1.0),
        }
    }

    /// Scale from a component to the number its field shows.
    fn shown(self, i: usize) -> f32 {
        match (self, i) {
            (Self::Hsv, 0) | (Self::Lab, _) => 1.0,
            (Self::Hsv, _) => 100.0,
            (Self::Rgb, _) => 255.0,
        }
    }

    fn get(self, now: &Components) -> [f32; 3] {
        match self {
            Self::Hsv => now.hsv,
            Self::Rgb => now.rgb,
            Self::Lab => now.lab,
        }
    }

    fn to_rgb(self, v: [f32; 3]) -> [f32; 3] {
        match self {
            Self::Hsv => hsv_to_rgb(v[0], v[1], v[2]),
            Self::Rgb => v,
            Self::Lab => photocraft_color::convert::lab_to_srgb(v).map(|c| c.clamp(0.0, 1.0)),
        }
    }

    /// The edit that sets the colour to `v`, keeping `v` so hue and out-of-gamut Lab survive.
    fn edit(self, v: [f32; 3]) -> ([f32; 3], Keep) {
        let keep = match self {
            Self::Hsv => Keep::Hsv(v),
            Self::Rgb => Keep::Nothing,
            Self::Lab => Keep::Lab(v),
        };
        (self.to_rgb(v), keep)
    }
}

impl Mode {
    /// `v` with component `i` at `t` (0..1) of its range.
    fn set(&self, mut v: [f32; 3], i: usize, t: f32) -> [f32; 3] {
        let (lo, hi) = self.space.range(i);
        v[i] = lo + t * (hi - lo);
        v
    }

    fn norm(&self, v: [f32; 3], i: usize) -> f32 {
        let (lo, hi) = self.space.range(i);
        (v[i] - lo) / (hi - lo)
    }

    /// Components at field position (x, y) in 0..1 (y down) with the slider at `z` (0..1).
    fn at(&self, v: [f32; 3], z: f32, x: f32, y: f32) -> [f32; 3] {
        self.set(self.set(self.set(v, self.z, z), self.x, x), self.y, 1.0 - y)
    }

    /// Field position and slider value of `v` (inverse of [`Mode::at`]).
    fn locate(&self, v: [f32; 3]) -> (f32, f32, f32) {
        (self.norm(v, self.x), 1.0 - self.norm(v, self.y), self.norm(v, self.z))
    }

    /// Slider colour at `z`: the hue slider shows pure hues, the others the current colour.
    fn slider_rgb(&self, v: [f32; 3], z: f32) -> [f32; 3] {
        if self.key == "h" { hsv_to_rgb(z * 360.0, 1.0, 1.0) } else { self.space.to_rgb(self.set(v, self.z, z)) }
    }
}

fn find_mode(key: &str) -> &'static Mode {
    MODES.iter().find(|m| m.key == key).unwrap_or(&MODES[0])
}

pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [f32; 3] {
    let h = (h.rem_euclid(360.0)) / 60.0;
    let c = v * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    [r + m, g + m, b + m]
}

pub fn rgb_to_hsv(c: [f32; 3]) -> [f32; 3] {
    let (mx, mn) = (c[0].max(c[1]).max(c[2]), c[0].min(c[1]).min(c[2]));
    let d = mx - mn;
    let h = if d <= 0.0 {
        0.0
    } else if mx == c[0] {
        60.0 * ((c[1] - c[2]) / d).rem_euclid(6.0)
    } else if mx == c[1] {
        60.0 * ((c[2] - c[0]) / d + 2.0)
    } else {
        60.0 * ((c[0] - c[1]) / d + 4.0)
    };
    [h, if mx <= 0.0 { 0.0 } else { d / mx }, mx]
}

pub fn hex(c: [f32; 3]) -> String {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", q(c[0]), q(c[1]), q(c[2]))
}

pub fn parse_hex(s: &str) -> Option<[f32; 3]> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    // `get`, not `[..]`: typed text may be multi-byte, and a byte range can split a character.
    let c = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok().map(|v| f32::from(v) / 255.0);
    Some([c(0)?, c(2)?, c(4)?])
}

fn c32(c: [f32; 3]) -> Color32 {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgb(q(c[0]), q(c[1]), q(c[2]))
}

/// Open the picker for the foreground or background colour.
pub fn open(app: &mut PhotocraftApp, target: &str) -> u64 {
    let c = if target == "background" { app.session.tools.background } else { app.session.tools.foreground };
    let rgb = [c[0], c[1], c[2]];
    let hsv = rgb_to_hsv(rgb);
    let mut f = Map::new();
    f.insert("__colorPicker".into(), json!(target));
    f.insert("__label".into(), json!(if target == "background" { "Color Picker (Background Color)" } else { "Color Picker (Foreground Color)" }));
    f.insert("color".into(), json!(hex(rgb)));
    f.insert("__orig".into(), json!(hex(rgb)));
    f.insert("__hsv".into(), json!(hsv));
    f.insert("__mode".into(), json!("h"));
    f.insert("__webOnly".into(), json!(false));
    app.ui.open_dialog(DialogKind::Command, f)
}

/// Open the picker on `rgb` for something other than the tool colours: OK runs `command` with
/// `params` plus `"color": "#rrggbb"` (e.g. a gradient stop's colour).
pub fn open_for_command(app: &mut PhotocraftApp, label: &str, rgb: [f32; 3], command: &str, params: Value) -> u64 {
    let hsv = rgb_to_hsv(rgb);
    let mut f = Map::new();
    f.insert("__colorPicker".into(), json!("command"));
    f.insert("__label".into(), json!(label));
    f.insert("__command".into(), json!(command));
    f.insert("__params".into(), params);
    f.insert("color".into(), json!(hex(rgb)));
    f.insert("__orig".into(), json!(hex(rgb)));
    f.insert("__hsv".into(), json!(hsv));
    f.insert("__mode".into(), json!("h"));
    f.insert("__webOnly".into(), json!(false));
    app.ui.open_dialog(DialogKind::Command, f)
}

pub fn owns(f: &Map<String, Value>) -> bool {
    f.contains_key("__colorPicker")
}

/// The Color Picker, when it is the top dialog (another dialog opened over it takes the input).
pub fn top(app: &PhotocraftApp) -> Option<u64> {
    app.ui.dialogs.last().filter(|d| owns(&d.fields)).map(|d| d.id)
}

/// Eyedropper: the top Color Picker's new colour becomes the image's composite colour at document
/// point (x, y). Off the image or over transparency nothing changes.
pub fn sample_at(app: &mut PhotocraftApp, x: f64, y: f64) {
    let Some(id) = top(app) else { return };
    let Some(rgb) = crate::canvas::composite_color(app, x, y) else { return };
    if let Some(d) = app.ui.dialog_mut(id) {
        set_rgb(&mut d.fields, rgb, Keep::Nothing);
    }
}

/// The colour in every model the dialog shows.
pub struct Components {
    pub rgb: [f32; 3],
    pub hsv: [f32; 3],
    pub lab: [f32; 3],
    pub cmyk: [f32; 4],
}

/// Component values an edit stores next to the new `color`. sRGB alone loses them: greys have no
/// hue, and Lab or CMYK values outside sRGB are clamped. Typing "60" into L passes through L = 6
/// first; without the stored Lab the clamped colour would change `a` and `b` under the user.
#[derive(Clone, Copy)]
enum Keep {
    Nothing,
    Hsv([f32; 3]),
    Lab([f32; 3]),
    Cmyk([f32; 4]),
}

/// The stored `key` components, if they still produce `rgb`.
fn kept<const N: usize>(f: &Map<String, Value>, key: &str, rgb: [f32; 3], to_rgb: impl Fn([f32; N]) -> [f32; 3]) -> Option<[f32; N]>
where
    [f32; N]: serde::de::DeserializeOwned,
{
    let v: [f32; N] = f.get(key).and_then(|v| serde_json::from_value(v.clone()).ok())?;
    (hex(to_rgb(v)) == hex(rgb)).then_some(v)
}

/// Current colour, keeping the stored HSB, Lab or CMYK values while they still match `color`.
pub fn current(f: &Map<String, Value>) -> Components {
    let rgb = f.get("color").and_then(Value::as_str).and_then(parse_hex).unwrap_or([0.0; 3]);
    Components {
        rgb,
        hsv: kept(f, "__hsv", rgb, |h| hsv_to_rgb(h[0], h[1], h[2])).unwrap_or_else(|| rgb_to_hsv(rgb)),
        lab: kept(f, "__lab", rgb, photocraft_color::convert::lab_to_srgb).unwrap_or_else(|| photocraft_color::convert::srgb_to_lab(rgb)),
        cmyk: kept(f, "__cmyk", rgb, photocraft_color::convert::cmyk_to_rgb).unwrap_or_else(|| photocraft_color::convert::rgb_to_cmyk(rgb)),
    }
}

fn set_rgb(f: &mut Map<String, Value>, rgb: [f32; 3], keep: Keep) {
    let web = f.get("__webOnly").and_then(Value::as_bool).unwrap_or(false);
    let (rgb, keep) = if web { (rgb.map(|v| (v * 5.0).round() / 5.0), Keep::Nothing) } else { (rgb, keep) };
    f.insert("color".into(), json!(hex(rgb)));
    for key in ["__hsv", "__lab", "__cmyk"] {
        f.remove(key);
    }
    match keep {
        Keep::Nothing => {}
        Keep::Hsv(h) => {
            f.insert("__hsv".into(), json!(h));
        }
        Keep::Lab(l) => {
            f.insert("__lab".into(), json!(l));
        }
        Keep::Cmyk(k) => {
            f.insert("__cmyk".into(), json!(k));
        }
    }
}

fn grid_mesh(rect: Rect, n: usize, color: impl Fn(f32, f32) -> [f32; 3]) -> Mesh {
    let mut m = Mesh::default();
    for j in 0..=n {
        for i in 0..=n {
            let (x, y) = (i as f32 / n as f32, j as f32 / n as f32);
            m.colored_vertex(pos2(rect.left() + x * rect.width(), rect.top() + y * rect.height()), c32(color(x, y)));
        }
    }
    let w = (n + 1) as u32;
    for j in 0..n as u32 {
        for i in 0..n as u32 {
            let a = j * w + i;
            m.add_triangle(a, a + 1, a + w + 1);
            m.add_triangle(a, a + w + 1, a + w);
        }
    }
    m
}

/// The dialog's width: its body, with no room left over.
pub const WIDTH: f32 = 580.0;

/// The dialog body, with its own OK and Cancel stacked beside the swatches. Returns `Some(true)`
/// for OK and `Some(false)` for Cancel.
pub fn body(ui: &mut egui::Ui, f: &mut Map<String, Value>) -> Option<bool> {
    let m = find_mode(f.get("__mode").and_then(Value::as_str).unwrap_or("h"));
    let now = current(f);
    let v = m.space.get(&now);
    let mut edit = None;
    let mut outcome = None;
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            edit = color_field(ui, m, v, now.lab[0] > 60.0).or(edit);
            ui.add_space(16.0);
            let mut web = f.get("__webOnly").and_then(Value::as_bool).unwrap_or(false);
            if widgets::checkbox(ui, &mut web, tl!("Only Web Colors")).changed() {
                f.insert("__webOnly".into(), json!(web));
                edit = Some((now.rgb, Keep::Nothing));
            }
        });
        ui.add_space(12.0);
        edit = slider(ui, m, v).or(edit);
        ui.add_space(12.0);
        ui.vertical(|ui| {
            ui.horizontal_top(|ui| {
                edit = swatches(ui, f, now.rgb).or(edit);
                ui.add_space(28.0);
                ui.vertical(|ui| {
                    if widgets::primary_button(ui, tl!("OK"), 116.0).clicked() {
                        outcome = Some(true);
                    }
                    ui.add_space(4.0);
                    if widgets::secondary_button(ui, tl!("Cancel"), 116.0).clicked() {
                        outcome = Some(false);
                    }
                });
            });
            ui.add_space(22.0);
            edit = fields(ui, f, &now).or(edit);
        });
    });
    if let Some((c, keep)) = edit {
        set_rgb(f, c, keep);
    }
    outcome
}

/// The 2D colour field: the selected component's slider value, the other two across and up.
fn color_field(ui: &mut egui::Ui, m: &Mode, v: [f32; 3], light: bool) -> Edit {
    let t = Tokens::get(ui.ctx());
    let (fx, fy, fz) = m.locate(v);
    let (field, resp) = ui.allocate_exact_size(vec2(256.0, 256.0), Sense::click_and_drag());
    ui.painter().add(grid_mesh(field, 32, |x, y| m.space.to_rgb(m.at(v, fz, x, y))));
    ui.painter().rect_stroke(field, 0.0, Stroke::new(1.0, t.field_border), StrokeKind::Outside);
    let marker = pos2(field.left() + fx * field.width(), field.top() + fy * field.height());
    ui.painter().circle_stroke(marker, 5.0, Stroke::new(1.5, if light { Color32::BLACK } else { Color32::WHITE }));
    let p = resp.interact_pointer_pos().filter(|_| resp.dragged() || resp.clicked())?;
    let (x, y) = (((p.x - field.left()) / field.width()).clamp(0.0, 1.0), ((p.y - field.top()) / field.height()).clamp(0.0, 1.0));
    Some(m.space.edit(m.at(v, fz, x, y)))
}

/// The selected component's slider, top to bottom from the high end (hue runs 360° down to 0°).
fn slider(ui: &mut egui::Ui, m: &Mode, v: [f32; 3]) -> Edit {
    let t = Tokens::get(ui.ctx());
    let (strip, resp) = ui.allocate_exact_size(vec2(20.0, 256.0), Sense::click_and_drag());
    ui.painter().add(grid_mesh(strip, 32, |_, y| m.slider_rgb(v, 1.0 - y)));
    let sy = strip.top() + (1.0 - m.norm(v, m.z)) * strip.height();
    for (x, dir) in [(strip.left() - 1.0, 1.0f32), (strip.right() + 1.0, -1.0)] {
        let tri = vec![pos2(x, sy), pos2(x - 7.0 * dir, sy - 5.0), pos2(x - 7.0 * dir, sy + 5.0)];
        ui.painter().add(egui::Shape::convex_polygon(tri, t.text, Stroke::new(1.0, t.field_border)));
    }
    let p = resp.interact_pointer_pos().filter(|_| resp.dragged() || resp.clicked())?;
    let z = 1.0 - ((p.y - strip.top()) / strip.height()).clamp(0.0, 1.0);
    Some(m.space.edit(m.set(v, m.z, z)))
}

/// The new colour over the current one; clicking the current one restores it.
fn swatches(ui: &mut egui::Ui, f: &Map<String, Value>, rgb: [f32; 3]) -> Edit {
    let t = Tokens::get(ui.ctx());
    let label = |ui: &mut egui::Ui, text: &str| ui.add_sized(vec2(62.0, 16.0), egui::Label::new(egui::RichText::new(text).color(t.text_dim)));
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        label(ui, tl!("new"));
        let (sw, _) = ui.allocate_exact_size(vec2(62.0, 70.0), Sense::hover());
        let orig = f.get("__orig").and_then(Value::as_str).and_then(parse_hex).unwrap_or(rgb);
        let (new, cur) = sw.split_top_bottom_at_fraction(0.5);
        ui.painter().rect_filled(new, 0.0, c32(rgb));
        ui.painter().rect_filled(cur, 0.0, c32(orig));
        ui.painter().rect_stroke(sw, 0.0, Stroke::new(1.0, t.field_border), StrokeKind::Outside);
        label(ui, tl!("current"));
        let restore = ui.interact(cur, ui.id().with("cp-current"), Sense::click());
        restore.on_hover_text(tl!("Click to restore the current colour")).clicked().then_some((orig, Keep::Nothing))
    })
    .inner
}

type Edit = Option<([f32; 3], Keep)>;

/// A field row: radio (or room for one), label, `field` and unit, at fixed widths so the rows line up.
fn row<R>(ui: &mut egui::Ui, radio: Option<bool>, label: &str, unit: &str, field: impl FnOnce(&mut egui::Ui) -> R) -> (bool, R) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let (r, resp) = ui.allocate_exact_size(vec2(14.0, 14.0), if radio.is_some() { Sense::click() } else { Sense::hover() });
        if let Some(on) = radio {
            ui.painter().circle_stroke(r.center(), 5.5, Stroke::new(1.2, if on { t.accent } else { t.text_faint }));
            if on {
                ui.painter().circle_filled(r.center(), 3.0, t.accent);
            }
        }
        let text = egui::RichText::new(label).color(t.text_dim);
        ui.add_sized(vec2(18.0, 24.0), egui::Label::new(text).halign(egui::Align::Max));
        let inner = field(ui);
        ui.add_sized(vec2(12.0, 24.0), egui::Label::new(egui::RichText::new(unit).color(t.text_faint)).halign(egui::Align::Min));
        (resp.clicked(), inner)
    })
    .inner
}

/// A radio component row; clicking the radio makes it the field's component.
fn component_row(ui: &mut egui::Ui, f: &mut Map<String, Value>, m: &Mode, now: &Components, width: f32) -> Edit {
    let on = f.get("__mode").and_then(Value::as_str).unwrap_or("h") == m.key;
    let unit = match m.key {
        "h" => "°",
        "s" | "v" => "%",
        _ => "",
    };
    let (scale, (lo, hi)) = (m.space.shown(m.z), m.space.range(m.z));
    let mut v = m.space.get(now);
    let mut x = (v[m.z] * scale).round();
    let (clicked, resp) = row(ui, Some(on), tl!(m.label), unit, |ui| widgets::value_field(ui, &mut x, lo * scale..=hi * scale, "", width));
    if clicked {
        f.insert("__mode".into(), json!(m.key));
    }
    v[m.z] = x / scale;
    resp.changed().then(|| m.space.edit(v))
}

/// Component fields: H S B, R G B and hex on the left; L a b and C M Y K on the right.
fn fields(ui: &mut egui::Ui, f: &mut Map<String, Value>, now: &Components) -> Edit {
    let mut edit = None;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing = vec2(10.0, 0.0);
        ui.vertical(|ui| {
            for (i, m) in MODES[..6].iter().enumerate() {
                if i == 3 {
                    ui.add_space(6.0);
                }
                edit = component_row(ui, f, m, now, 44.0).or(edit);
            }
            ui.add_space(6.0);
            edit = row(ui, None, "#", "", |ui| hex_field(ui, now.rgb)).1.or(edit);
        });
        ui.vertical(|ui| {
            for m in &MODES[6..] {
                edit = component_row(ui, f, m, now, 44.0).or(edit);
            }
            ui.add_space(6.0);
            for (i, label) in ["C:", "M:", "Y:", "K:"].into_iter().enumerate() {
                let mut x = (now.cmyk[i] * 100.0).round();
                if row(ui, None, tl!(label), "%", |ui| widgets::value_field(ui, &mut x, 0.0..=100.0, "", 44.0)).1.changed() {
                    let mut k = now.cmyk;
                    k[i] = x / 100.0;
                    edit = Some((photocraft_color::convert::cmyk_to_rgb(k).map(|c| c.clamp(0.0, 1.0)), Keep::Cmyk(k)));
                }
            }
        });
    });
    edit
}

/// The hex field. While it has focus it shows what is being typed, not the colour: rebuilding it
/// from the colour every frame would throw away every keystroke until six valid digits were in.
fn hex_field(ui: &mut egui::Ui, rgb: [f32; 3]) -> Edit {
    let id = ui.id().with("cp-hex");
    let typing = if ui.memory(|m| m.has_focus(id)) { ui.data(|d| d.get_temp::<String>(id)) } else { None };
    let mut h = typing.unwrap_or_else(|| hex(rgb).trim_start_matches('#').to_string());
    let edit = egui::TextEdit::singleline(&mut h).id(id).char_limit(6).font(crate::theme::mono(12.0)).vertical_align(egui::Align::Center);
    let r = ui.add_sized(vec2(84.0, 24.0), edit);
    if r.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, h.clone()));
    } else {
        ui.data_mut(|d| d.remove::<String>(id));
    }
    if r.changed() { parse_hex(&h).map(|c| (c, Keep::Nothing)) } else { None }
}

/// OK: set the foreground or background colour.
pub fn confirm(app: &mut PhotocraftApp, f: &Map<String, Value>) -> Result<Value, String> {
    let target = f.get("__colorPicker").and_then(Value::as_str).unwrap_or("foreground");
    let color = f.get("color").and_then(Value::as_str).unwrap_or("#000000");
    if let Some(cmd) = f.get("__command").and_then(Value::as_str) {
        let mut p = f.get("__params").cloned().filter(Value::is_object).unwrap_or_else(|| json!({}));
        p["color"] = json!(color);
        return app.run(cmd, p);
    }
    let r = app.run("tools.setColors", json!({ target: color }))?;
    if target == "foreground" {
        crate::type_tool::foreground_changed(app);
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use egui::accesskit::Role;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    use super::*;

    fn picker(color: &str) -> Harness<'static, Map<String, Value>> {
        let mut f = Map::new();
        f.insert("__colorPicker".into(), json!("foreground"));
        f.insert("color".into(), json!(color));
        let mut h = Harness::builder().with_size(vec2(700.0, 500.0)).build_ui_state(
            |ui, f: &mut Map<String, Value>| {
                if ui.ctx().fonts(|fonts| fonts.families().contains(&egui::FontFamily::Name("medium".into()))) {
                    body(ui, f);
                }
            },
            f,
        );
        PhotocraftApp::setup_context(&h.ctx, crate::theme::ThemeKind::ALL[0]);
        h.run_steps(2);
        h
    }

    /// Type `text` into the focused field one key per frame, like a person typing.
    fn type_keys(h: &mut Harness<'static, Map<String, Value>>, text: &str) {
        for ch in text.chars() {
            h.event(egui::Event::Text(ch.to_string()));
            h.run_steps(1);
        }
    }

    fn color(h: &Harness<'static, Map<String, Value>>) -> String {
        h.state().get("color").and_then(Value::as_str).unwrap().to_string()
    }

    #[test]
    fn hex_field_takes_typed_digits() {
        let mut h = picker("#000000");
        h.get_by_role(Role::TextInput).click();
        h.run_steps(1);
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        h.run_steps(1);
        type_keys(&mut h, "ff88");
        // Partial input stays in the field instead of snapping back to the colour.
        assert_eq!(h.get_by_role(Role::TextInput).value().as_deref(), Some("ff88"));
        assert_eq!(color(&h), "#000000");
        type_keys(&mut h, "00");
        assert_eq!(color(&h), "#ff8800");
        h.key_press(egui::Key::Tab);
        h.run_steps(2);
        assert_eq!(color(&h), "#ff8800");
    }

    #[test]
    fn lab_and_cmyk_fields_take_multi_digit_values() {
        let base = parse_hex("#336699").unwrap();
        let lab = photocraft_color::convert::srgb_to_lab(base);
        let cmyk = photocraft_color::convert::rgb_to_cmyk(base);
        // Fields in order: H S B R G B, L a b, C M Y K.
        for (field, typed, want) in [
            (6, "60", photocraft_color::convert::lab_to_srgb([60.0, lab[1], lab[2]])),
            (7, "30", photocraft_color::convert::lab_to_srgb([lab[0], 30.0, lab[2]])),
            (9, "40", photocraft_color::convert::cmyk_to_rgb([0.4, cmyk[1], cmyk[2], cmyk[3]])),
        ] {
            let mut h = picker("#336699");
            h.query_all_by_role(Role::SpinButton).nth(field).unwrap().click();
            h.run_steps(1);
            type_keys(&mut h, typed);
            h.key_press(egui::Key::Tab);
            h.run_steps(2);
            assert_eq!(color(&h), hex(want), "field {field} <- {typed}");
        }
    }

    #[test]
    fn parse_hex_rejects_multibyte_text() {
        // Six bytes, three characters: the byte range 0..2 splits "é", which used to panic.
        assert_eq!(parse_hex("aé€"), None);
        assert_eq!(parse_hex("#ff880"), None);
    }

    #[test]
    fn hsv_round_trips() {
        for c in [[1.0, 0.0, 0.0], [0.2, 0.6, 0.4], [0.5, 0.5, 0.5], [0.0, 0.0, 1.0], [0.9, 0.8, 0.1]] {
            let h = rgb_to_hsv(c);
            let back = hsv_to_rgb(h[0], h[1], h[2]);
            for i in 0..3 {
                assert!((back[i] - c[i]).abs() < 1e-5, "{c:?} -> {h:?} -> {back:?}");
            }
        }
        assert_eq!(hex([1.0, 0.5, 0.0]), "#ff8000");
        assert_eq!(parse_hex("#FF8000").map(hex).as_deref(), Some("#ff8000"));
    }

    #[test]
    fn field_and_locate_are_inverse_in_every_mode() {
        let mut f = Map::new();
        f.insert("color".into(), json!("#cc4d33"));
        let now = current(&f);
        for m in MODES {
            let v = m.space.get(&now);
            let (x, y, z) = m.locate(v);
            assert_eq!(hex(m.space.to_rgb(m.at(v, z, x, y))), "#cc4d33", "mode {}", m.key);
            assert_eq!(hex(m.space.to_rgb(m.set(v, m.z, z))), "#cc4d33", "slider {}", m.key);
        }
    }

    #[test]
    fn the_l_field_spans_a_across_and_b_up() {
        let m = find_mode("lab-l");
        let lab = photocraft_color::convert::srgb_to_lab(parse_hex("#336699").unwrap());
        let (_, _, z) = m.locate(lab);
        assert_eq!(m.at(lab, z, 0.0, 0.0)[1..], [-128.0, 127.0]);
        assert_eq!(m.at(lab, z, 1.0, 1.0)[1..], [127.0, -128.0]);
        assert_eq!(find_mode("nonsense").key, "h");
    }

    #[test]
    fn hue_survives_grey_and_ok_sets_the_colour() {
        let mut f = Map::new();
        f.insert("color".into(), json!("#808080"));
        f.insert("__hsv".into(), json!([200.0, 0.0, 128.0 / 255.0]));
        assert_eq!(current(&f).hsv[0], 200.0);
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), Default::default());
        let id = open(&mut app, "background");
        let d = app.ui.dialog_mut(id).unwrap();
        d.fields.insert("color".into(), json!("#3366cc"));
        let fields = d.fields.clone();
        confirm(&mut app, &fields).unwrap();
        assert_eq!(hex([app.session.tools.background[0], app.session.tools.background[1], app.session.tools.background[2]]), "#3366cc");
    }

    fn dialog_color(app: &PhotocraftApp, id: u64) -> &str {
        app.ui.dialogs.iter().find(|d| d.id == id).and_then(|d| d.fields.get("color")).and_then(Value::as_str).unwrap()
    }

    #[test]
    fn the_top_picker_samples_the_image() {
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), Default::default());
        app.run("file.new", json!({"width": 40, "height": 20, "background": "transparent"})).unwrap();
        app.run("shape.create", json!({"kind": "rect", "rect": [0, 0, 20, 20], "fill": "#ff0000"})).unwrap();
        app.run("shape.create", json!({"kind": "rect", "rect": [20, 0, 10, 20], "fill": "#00ff00"})).unwrap();
        let id = open(&mut app, "foreground");
        assert_eq!(top(&app), Some(id));
        sample_at(&mut app, 25.5, 5.5);
        assert_eq!(dialog_color(&app, id), "#00ff00");
        assert_eq!(app.ui.dialog_mut(id).unwrap().fields["__orig"], json!("#000000"), "the current colour stays");
        // Off the image or over transparency nothing changes.
        for (x, y) in [(35.0, 5.0), (-3.0, 5.0), (5.0, 20.0), (1e12, -1e12), (f64::NAN, f64::INFINITY)] {
            sample_at(&mut app, x, y);
            assert_eq!(dialog_color(&app, id), "#00ff00", "({x}, {y})");
        }

        // Agents: `ui.pointer` samples into the picker instead of driving the tool.
        app.ui.tool = crate::state::Tool::Brush;
        let rev = app.session.active().unwrap().revision;
        let ctx = egui::Context::default();
        let events = json!([{"kind": "down", "x": 5, "y": 5}, {"kind": "up", "x": 5, "y": 5}]);
        let (req, _rx) = crate::control::ControlRequest::new("ui.pointer", json!({"events": events}));
        let _ = crate::control::handle(&mut app, &ctx, &req);
        assert_eq!(dialog_color(&app, id), "#ff0000");
        assert_eq!(app.session.active().unwrap().revision, rev, "the Brush must not paint");
        assert_eq!(app.session.tools.foreground, [0.0, 0.0, 0.0, 1.0], "only OK sets the foreground");

        // A dialog opened over the picker takes the input.
        let about = app.ui.open_dialog(DialogKind::About, Map::new());
        assert_eq!(top(&app), None);
        sample_at(&mut app, 25.0, 5.0);
        assert_eq!(dialog_color(&app, id), "#ff0000");
        app.ui.close_dialog(about);
        sample_at(&mut app, 25.0, 5.0);
        assert_eq!(dialog_color(&app, id), "#00ff00");
    }
}
