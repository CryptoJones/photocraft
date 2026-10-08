//! Coming-soon generative workflows: editable prompts and geometry, with local draft review.
//! Execution stays disabled regardless of account state. No provider or upload is hidden here.
use egui::RichText;
use photocraft_engine::generative_cmds::{self, Operation};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{PhotocraftApp, theme::Tokens, widgets};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GenerativeState {
    pub open: bool,
    pub operation: Operation,
    pub prompt: String,
    pub width: u32,
    pub height: u32,
    /// Review is transient: reopening a saved UI must validate the current document again.
    #[serde(skip)]
    pub review: Option<Value>,
}

impl Default for GenerativeState {
    fn default() -> Self {
        Self { open: false, operation: Operation::Generate, prompt: String::new(), width: 1024, height: 1024, review: None }
    }
}

impl GenerativeState {
    pub fn params(&self) -> Value {
        json!({"operation":self.operation,"prompt":self.prompt,"width":self.width,"height":self.height})
    }

    pub fn patched(&self, value: &Value) -> Result<Self, String> {
        let fields = value.as_object().ok_or("generative must be an object")?;
        if fields.keys().any(|k| !["open", "operation", "prompt", "width", "height"].contains(&k.as_str())) {
            return Err("generative accepts open, operation, prompt, width and height only".into());
        }
        let mut state = self.clone();
        if let Some(v) = fields.get("open") {
            state.open = v.as_bool().ok_or("generative.open must be a boolean")?;
        }
        if let Some(v) = fields.get("operation") {
            state.operation = serde_json::from_value(v.clone()).map_err(|_| "operation must be generate, extend, reframe or editSelection")?;
        }
        if let Some(v) = fields.get("prompt") {
            state.prompt = v.as_str().ok_or("generative.prompt must be text")?.into();
        }
        for (key, out) in [("width", &mut state.width), ("height", &mut state.height)] {
            if let Some(v) = fields.get(key) {
                *out = v.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or_else(|| format!("generative.{key} must be a positive whole number"))?;
            }
        }
        generative_cmds::validate_size(state.width, state.height).map_err(|e| e.to_string())?;
        generative_cmds::validate_prompt(&state.prompt).map_err(|e| e.to_string())?;
        state.review = None;
        Ok(state)
    }
}

pub fn open(app: &mut PhotocraftApp, operation: Operation) {
    let mut state = std::mem::take(&mut app.ui.generative);
    state.open = true;
    state.operation = operation;
    state.review = None;
    if let Some(st) = app.session.active() {
        state.width = st.doc.size.width;
        state.height = st.doc.size.height;
        if operation == Operation::Extend {
            state.width = state.width.saturating_add((state.width / 4).max(1)).min(generative_cmds::MAX_SIDE);
            state.height = state.height.saturating_add((state.height / 4).max(1)).min(generative_cmds::MAX_SIDE);
        }
    }
    app.ui.generative = state;
}

fn framed_size(width: u32, height: u32, aspect: [u32; 2]) -> Result<[u32; 2], String> {
    let [a, b] = aspect;
    if a == 0 || b == 0 {
        return Err("frame ratio must be positive".into());
    }
    let w = u64::from(width).max((u64::from(height) * u64::from(a)).div_ceil(u64::from(b)));
    let h = u64::from(height).max((u64::from(width) * u64::from(b)).div_ceil(u64::from(a)));
    let w = u32::try_from(w).map_err(|_| "frame is too large")?;
    let h = u32::try_from(h).map_err(|_| "frame is too large")?;
    generative_cmds::validate_size(w, h).map_err(|e| e.to_string())?;
    Ok([w, h])
}

pub fn show(app: &mut PhotocraftApp, ctx: &egui::Context) {
    if !app.ui.generative.open || app.ui.view.hides_chrome() {
        return;
    }
    let mut state = std::mem::take(&mut app.ui.generative);
    let source = app.session.active().map(|st| (st.doc.id, st.revision, st.doc.size));
    let has_selection = app.session.active().is_some_and(|st| st.doc.selection.is_some());
    if let Some(review) = &state.review {
        let expected = source.map(|(id, revision, _)| json!({"document":id,"revision":revision}));
        let actual = review.get("source").filter(|v| !v.is_null()).map(|v| json!({"document":v.get("document"),"revision":v.get("revision")}));
        if actual != expected {
            state.review = None;
        }
    }
    let t = Tokens::get(ctx);
    let mut visible = state.open;
    let mut review = false;
    let mut choose = None;
    let mut account = false;
    let mut local_error = None;
    let viewport = if app.last_canvas_rect.width() > 300.0 && app.last_canvas_rect.height() > 250.0 {
        app.last_canvas_rect.intersect(ctx.content_rect()).shrink(10.0)
    } else {
        ctx.content_rect().shrink(20.0)
    };
    let width = (viewport.width() - 24.0).clamp(240.0, 420.0);
    egui::Window::new(format!("{} · Coming soon", state.operation.label()))
        .id(egui::Id::new("generative-draft"))
        .open(&mut visible)
        .collapsible(false)
        .resizable(false)
        .constrain_to(viewport)
        .max_height((viewport.height() - 30.0).max(120.0))
        .vscroll(true)
        .default_pos(viewport.center() - egui::vec2(width / 2.0, 160.0))
        .default_width(width)
        .default_height(480.0)
        .frame(egui::Frame::new().fill(t.card).stroke(egui::Stroke::new(1.0, t.card_border)).corner_radius(t.radius).inner_margin(12))
        .show(ctx, |ui| {
            ui.set_max_width(width);
            ui.label(RichText::new("Coming soon").font(crate::theme::semibold(15.0)).color(t.text));
            ui.label(RichText::new(generative_cmds::COMING_SOON).color(t.text_dim));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label("Workflow");
                egui::ComboBox::from_id_salt("generative-operation").selected_text(state.operation.label()).show_ui(ui, |ui| {
                    for operation in [Operation::Generate, Operation::Extend, Operation::Reframe, Operation::EditSelection] {
                        let enabled = operation == Operation::Generate || (source.is_some() && (operation != Operation::EditSelection || has_selection));
                        if ui.add_enabled(enabled, egui::Button::selectable(state.operation == operation, operation.label())).clicked() {
                            choose = Some(operation);
                        }
                    }
                });
            });
            ui.add_space(6.0);
            ui.label(match state.operation {
                Operation::Generate => "Describe an image for a new layer or an empty background.",
                Operation::Extend => "Describe the scene beyond the image edges (optional).",
                Operation::Reframe => "Describe the expanded scene for the new frame (optional).",
                Operation::EditSelection => "Describe how to replace or modify the selected object.",
            });
            let mut changed = ui
                .add(egui::TextEdit::multiline(&mut state.prompt).desired_width(f32::INFINITY).desired_rows(3).char_limit(generative_cmds::MAX_PROMPT_CHARS))
                .changed();
            if state.operation == Operation::EditSelection
                && let Some((_, _, size)) = source
            {
                state.width = size.width;
                state.height = size.height;
            }
            ui.add_enabled_ui(state.operation != Operation::EditSelection, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Width");
                    changed |= ui.add(egui::DragValue::new(&mut state.width).range(1..=generative_cmds::MAX_SIDE).suffix(" px")).changed();
                    ui.label("Height");
                    changed |= ui.add(egui::DragValue::new(&mut state.height).range(1..=generative_cmds::MAX_SIDE).suffix(" px")).changed();
                });
            });
            if state.operation == Operation::Reframe
                && let Some((_, _, size)) = source
            {
                ui.horizontal(|ui| {
                    ui.label("Frame");
                    for (label, aspect) in [("1:1", [1, 1]), ("4:5", [4, 5]), ("16:9", [16, 9]), ("9:16", [9, 16])] {
                        if ui.button(label).clicked() {
                            match framed_size(size.width, size.height, aspect) {
                                Ok([w, h]) => {
                                    state.width = w;
                                    state.height = h;
                                    changed = true;
                                }
                                Err(e) => local_error = Some(e),
                            }
                        }
                    }
                });
            }
            if matches!(state.operation, Operation::Extend | Operation::Reframe) {
                ui.label(RichText::new("The draft keeps the original image centred inside the new canvas.").color(t.text_dim));
            }
            if state.operation == Operation::EditSelection && !has_selection {
                ui.label("Select the object or region to edit first.");
            }
            if changed {
                state.review = None;
            }
            if let Some(draft) = &state.review {
                let output = draft.get("output");
                ui.label(format!(
                    "Local draft ready: {} × {} px. No request sent.",
                    output.and_then(|v| v.get("width")).and_then(Value::as_u64).unwrap_or(0),
                    output.and_then(|v| v.get("height")).and_then(Value::as_u64).unwrap_or(0)
                ));
            }
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                review = widgets::secondary_button(ui, "Review draft", 120.0)
                    .on_hover_text("Validate the prompt and proposed frame locally. This does not generate or upload anything.")
                    .clicked();
                ui.add_enabled_ui(false, |ui| {
                    widgets::primary_button(ui, "Generate · Coming soon", 190.0).on_hover_text(generative_cmds::COMING_SOON);
                });
            });
            if widgets::secondary_button(ui, "ChatGPT account settings…", 230.0).clicked() {
                account = true;
            }
        });
    state.open = visible;
    if review {
        match app.run("generative.prepare", state.params()) {
            Ok(value) => state.review = Some(value),
            Err(error) => local_error = Some(error),
        }
    }
    app.ui.generative = state;
    if let Some(operation) = choose {
        open(app, operation);
    }
    if account {
        app.ui.chatgpt_account_open = true;
    }
    if let Some(error) = local_error {
        app.ui.status = error;
        app.ui.status_error = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::{Harness, kittest::Queryable};

    #[test]
    fn coming_soon_workflows_review_locally_and_never_generate_or_change_history() {
        let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).with_max_steps(64).build_eframe(|cc| {
            PhotocraftApp::setup_context(&cc.egui_ctx, Default::default());
            PhotocraftApp::new(photocraft_engine::Session::new(), crate::Services::default())
        });
        open(h.state_mut(), Operation::Generate);
        h.state_mut().ui.generative.prompt = "A blue bicycle".into();
        h.run_steps(8);
        assert!(h.query_by_label("Coming soon").is_some());
        h.get_by_label("Review draft").click();
        h.run_steps(6);
        assert!(h.state().ui.generative.review.is_some());
        assert!(h.state().session.documents().is_empty());
        h.get_by_label("Generate · Coming soon").click();
        h.run_steps(4);
        assert!(h.state().session.documents().is_empty());
        h.state_mut().session.execute("file.new", json!({"width":200,"height":160})).unwrap();
        h.state_mut().session.execute("select.rect", json!({"x":40,"y":30,"width":80,"height":60})).unwrap();
        let doc = h.state().session.active().unwrap().doc.clone();
        let steps = h.state().session.active().unwrap().history.past_len();
        for operation in [Operation::Generate, Operation::Extend, Operation::Reframe, Operation::EditSelection] {
            open(h.state_mut(), operation);
            h.run_steps(8);
            h.get_by_label("Review draft").click();
            h.run_steps(6);
            assert!(h.state().ui.generative.review.is_some(), "{}", h.state().ui.status);
            assert_eq!(h.state().session.active().unwrap().doc, doc);
            assert_eq!(h.state().session.active().unwrap().history.past_len(), steps);
        }
        assert!(!h.state().session.is_enabled("generative.run"));
        h.state_mut().session.execute("select.deselect", json!({})).unwrap();
        h.run_steps(6);
        assert!(h.state().ui.generative.review.is_none(), "document changes invalidate a reviewed draft");
    }

    #[test]
    fn reframing_preserves_the_image_and_hostile_ui_state_fails_atomically() {
        assert_eq!(framed_size(400, 320, [16, 9]).unwrap(), [569, 320]);
        assert_eq!(framed_size(400, 320, [9, 16]).unwrap(), [400, 712]);
        assert!(framed_size(u32::MAX, u32::MAX, [16, 9]).is_err());
        assert!(framed_size(400, 320, [0, 1]).is_err());
        let state = GenerativeState::default();
        for p in [
            json!(null),
            json!({"open":1}),
            json!({"operation":"wrong"}),
            json!({"prompt":2}),
            json!({"width":0}),
            json!({"width":4294967296_u64}),
            json!({"width":32768,"height":32768}),
            json!({"prompt":"x".repeat(2001)}),
            json!({"endpoint":"https://example.invalid"}),
        ] {
            assert!(state.patched(&p).is_err());
            assert_eq!(state, GenerativeState::default());
        }
        assert_eq!(state.patched(&json!({"operation":"extend","open":true})).unwrap().operation, Operation::Extend);
    }
}
