//! UI for the Eraser group's colour-aware tools: Magic Eraser (click → `paint.magicEraser`) and
//! Background Eraser (stroke → `paint.backgroundEraser`), with their options bars.

use serde_json::{Value, json};

use crate::PhotocraftApp;
use crate::state::Tool;
use crate::theme::Tokens;
use crate::{icons, widgets};

fn report(app: &mut PhotocraftApp, r: Result<Value, String>) {
    if let Err(e) = r {
        app.ui.status = e;
        app.ui.status_error = true;
    }
}

/// Magic Eraser click at document `(x, y)`. Returns false for other tools.
pub fn click(app: &mut PhotocraftApp, tool: Tool, x: f64, y: f64) -> bool {
    if tool != Tool::MagicEraser {
        return false;
    }
    let o = &app.ui.tool_options;
    let p = json!({
        "x": x.floor(),
        "y": y.floor(),
        "tolerance": o.tolerance,
        "antiAlias": o.anti_alias,
        "contiguous": o.contiguous,
        "sampleAllLayers": o.sample_all_layers,
        "opacity": o.magic_eraser_opacity,
    });
    let r = app.run("paint.magicEraser", p);
    report(app, r);
    true
}

/// Finishes a Background Eraser stroke. Returns false for other tools.
pub fn finish_stroke(app: &mut PhotocraftApp, tool: Tool, points: &[[f64; 3]]) -> bool {
    if tool != Tool::BackgroundEraser {
        return false;
    }
    let o = &app.ui.tool_options;
    let p = json!({
        "points": points,
        "sampling": o.bg_sampling,
        "limits": o.bg_limits,
        "tolerance": o.bg_tolerance,
        "protectForegroundColor": o.bg_protect_fg,
    });
    let r = app.run("paint.backgroundEraser", p);
    report(app, r);
    true
}

fn opt(ui: &mut egui::Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    ui.label(egui::RichText::new(tl!(&text)).color(t.text_dim).size(12.0));
}

/// Options bar for the Magic Eraser and Background Eraser. Returns false for other tools.
pub fn options_bar(app: &mut PhotocraftApp, ui: &mut egui::Ui, tool: Tool) -> bool {
    let o = &mut app.ui.tool_options;
    match tool {
        Tool::MagicEraser => {
            opt(ui, tl_id!("ui-tolerance-7039963b63e5c7cc"));
            widgets::value_field(ui, &mut o.tolerance, 0.0..=255.0, "", 58.0);
            widgets::checkbox(ui, &mut o.anti_alias, tl_id!("ui-anti-alias-b3b588f74fa6d366"));
            widgets::checkbox(ui, &mut o.contiguous, tl_id!("ui-contiguous-0db31a8029e64f7f"));
            widgets::checkbox(ui, &mut o.sample_all_layers, tl_id!("ui-sample-all-layers-fd67756ff1544318"));
            widgets::vline(ui, 22.0);
            opt(ui, tl_id!("ui-opacity-b3b2ce99002fac62"));
            widgets::value_field(ui, &mut o.magic_eraser_opacity, 1.0..=100.0, "%", 62.0);
        }
        Tool::BackgroundEraser => {
            // Sampling: Continuous / Once / Background Swatch, as icon toggles (Photoshop).
            ui.spacing_mut().item_spacing.x = 2.0;
            for (k, icon, tip) in [
                ("continuous", "pipette", tl_id!("ui-sampling-continuous-5dd49d6b6ae5d3f7")),
                ("once", "circle-dot", tl_id!("ui-sampling-once-e4bab686a4e23735")),
                ("backgroundSwatch", "square", tl_id!("ui-sampling-background-swatch-636e6c4891a4024c")),
            ] {
                if icons::button(ui, icon, 24.0, o.bg_sampling == k, tip).clicked() {
                    o.bg_sampling = k.into();
                }
            }
            ui.spacing_mut().item_spacing.x = 8.0;
            widgets::vline(ui, 22.0);
            opt(ui, tl_id!("ui-limits-d7e5ab8f6b938f73"));
            let limits = [
                ("discontiguous".to_string(), tl_id!("ui-discontiguous-d45dcce1dd7b6859")),
                ("contiguous".to_string(), tl_id!("ui-contiguous-0db31a8029e64f7f")),
                ("findEdges".to_string(), tl_id!("ui-find-edges-9d036a4a822f9306")),
            ];
            widgets::dropdown(ui, "bg-eraser-limits", &mut o.bg_limits, &limits, 110.0);
            opt(ui, tl_id!("ui-tolerance-7039963b63e5c7cc"));
            widgets::value_field(ui, &mut o.bg_tolerance, 0.0..=100.0, "%", 62.0);
            widgets::checkbox(ui, &mut o.bg_protect_fg, tl_id!("ui-protect-foreground-color-14aad13f97c41918"));
        }
        _ => return false,
    }
    true
}
