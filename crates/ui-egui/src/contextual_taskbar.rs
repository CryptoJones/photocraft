//! Viewport contextual actions. Drag the grip to move the bar; background removal uses the
//! existing engine command, with its model preference, background job, editable mask and undo.

use egui::{Area, Id, Order, Pos2, Rect, Sense, Stroke, Vec2, vec2};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{PhotocraftApp, icons, menus, theme::Tokens, widgets};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TaskbarState {
    pub visible: bool,
    /// Normalized within the viewport's movement range, independent of zoom and document
    /// pixels. Restored bars stay reachable after a window or dock resize.
    pub position: Option<[f32; 2]>,
}

impl Default for TaskbarState {
    fn default() -> Self {
        Self { visible: true, position: None }
    }
}

fn position(viewport: Rect, size: Vec2, saved: Option<[f32; 2]>) -> Pos2 {
    let room = (viewport.size() - size).max(Vec2::ZERO);
    let [x, y] = saved.filter(|p| p.iter().all(|v| v.is_finite())).unwrap_or([0.5, 0.94]);
    viewport.min + vec2(x.clamp(0.0, 1.0) * room.x, y.clamp(0.0, 1.0) * room.y)
}

fn normalized(viewport: Rect, size: Vec2, at: Pos2) -> [f32; 2] {
    let room = (viewport.size() - size).max(Vec2::ZERO);
    let relative = at - viewport.min;
    [if room.x > 0.0 { (relative.x / room.x).clamp(0.0, 1.0) } else { 0.0 }, if room.y > 0.0 { (relative.y / room.y).clamp(0.0, 1.0) } else { 0.0 }]
}

pub fn show(app: &mut PhotocraftApp, ctx: &egui::Context) {
    if !app.ui.contextual_taskbar.visible || app.ui.view.hides_chrome() || app.jobs.focus.is_some() || app.ui.generative.open {
        return;
    }
    let Some(st) = app.session.active() else { return };
    let Some(layer) = st.active_layer.and_then(|id| st.doc.layer(id)) else { return };
    if !matches!(layer.content, photocraft_doc::LayerContent::Raster(_) | photocraft_doc::LayerContent::Smart(_)) {
        return;
    }
    let has_selection = st.doc.selection.is_some();
    let enabled = menus::is_enabled(app, "layer.removeBackground");
    let viewport = app.last_canvas_rect.intersect(ctx.content_rect()).shrink(10.0);
    if viewport.width() < 220.0 || viewport.height() < 60.0 {
        return;
    }
    let remembered = ctx.data(|d| d.get_temp::<Vec2>(Id::new("contextual-taskbar-size")));
    let size = remembered.unwrap_or(vec2(220.0, 44.0));
    let at = position(viewport, size, app.ui.contextual_taskbar.position);
    let t = Tokens::get(ctx);
    let mut remove = false;
    let mut movement = Vec2::ZERO;
    let mut generative = None;
    let response = Area::new(Id::new("contextual-taskbar")).order(Order::Foreground).fixed_pos(at).movable(false).constrain_to(viewport).show(ctx, |ui| {
        egui::Frame::new().fill(t.card).stroke(Stroke::new(1.0, t.card_border)).corner_radius(t.radius).inner_margin(8).show(ui, |ui| {
            ui.horizontal(|ui| {
                let (grip, drag) = ui.allocate_exact_size(vec2(18.0, 26.0), Sense::drag());
                icons::paint(ui, grip, "move", 14.0, t.text_dim);
                drag.clone().on_hover_text("Drag contextual task bar");
                drag.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Drag contextual task bar"));
                if drag.dragged() {
                    movement = drag.drag_delta();
                }
                remove = ui
                    .add_enabled_ui(enabled, |ui| widgets::secondary_button(ui, "Remove Background", 140.0))
                    .inner
                    .on_hover_text("Hide the background with an editable layer mask. Uses the subject model selected in Preferences › Integrations.")
                    .clicked();
                ui.menu_button("…", |ui| {
                    use photocraft_engine::generative_cmds::Operation;
                    for operation in [Operation::Generate, Operation::Extend, Operation::Reframe, Operation::EditSelection] {
                        if ui
                            .add_enabled(
                                operation != Operation::EditSelection || has_selection,
                                egui::Button::new(format!("{} · Coming soon", operation.label())),
                            )
                            .clicked()
                        {
                            generative = Some(operation);
                            ui.close();
                        }
                    }
                    ui.separator();
                    if ui.button("Reset position").clicked() {
                        app.ui.contextual_taskbar.position = None;
                        ui.close();
                    }
                    if ui.button("Hide contextual task bar").clicked() {
                        app.ui.contextual_taskbar.visible = false;
                        ui.close();
                    }
                })
                .response
                .on_hover_text("Contextual task bar options");
            });
            if has_selection && widgets::secondary_button(ui, "Edit object · Coming soon", 200.0).clicked() {
                generative = Some(photocraft_engine::generative_cmds::Operation::EditSelection);
            }
        });
    });
    let rect = response.response.rect;
    ctx.data_mut(|d| d.insert_temp(Id::new("contextual-taskbar-size"), rect.size()));
    if movement != Vec2::ZERO {
        app.ui.contextual_taskbar.position = Some(normalized(viewport, rect.size(), rect.min + movement));
        ctx.request_repaint();
    }
    if remove && let Err(error) = menus::invoke(app, ctx, "layer.removeBackground", json!({})) {
        app.ui.status = error;
        app.ui.status_error = true;
    }
    if let Some(operation) = generative
        && let Err(error) = menus::invoke(app, ctx, "window.generativeAI", json!({"operation":operation}))
    {
        app.ui.status = error;
        app.ui.status_error = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;
    use egui_kittest::{Harness, kittest::Queryable};

    #[test]
    fn restored_positions_and_small_viewports_keep_the_bar_reachable() {
        let viewport = Rect::from_min_size(pos2(40.0, 60.0), vec2(300.0, 200.0));
        let size = vec2(160.0, 60.0);
        let p = position(viewport, size, Some([f32::NAN, -100.0]));
        assert!(viewport.contains_rect(Rect::from_min_size(p, size)));
        assert_eq!(position(viewport, size, Some([20.0, -2.0])), pos2(180.0, 60.0));
        assert_eq!(normalized(viewport, size, pos2(-400.0, 600.0)), [0.0, 1.0]);
        let smaller = Rect::from_min_size(pos2(20.0, 30.0), vec2(180.0, 70.0));
        assert!(smaller.contains_rect(Rect::from_min_size(position(smaller, size, Some([1.0, 1.0])), size)));
    }

    #[test]
    fn pixel_and_placed_smart_layers_show_actions_and_text_layers_do_not() {
        let mut session = photocraft_engine::Session::new();
        session.execute("file.new", json!({"width":400,"height":300})).unwrap();
        let mut h = Harness::builder().with_size(vec2(1200.0, 800.0)).with_max_steps(64).build_eframe(move |cc| {
            PhotocraftApp::setup_context(&cc.egui_ctx, Default::default());
            PhotocraftApp::new(session, crate::Services::default())
        });
        h.run_steps(8);
        assert!(h.query_by_label("Remove Background").is_some());
        h.state_mut().session.execute("select.rect", json!({"x":10,"y":20,"width":80,"height":60})).unwrap();
        h.run_steps(8);
        assert!(h.query_by_label("Remove Background").is_some());
        h.get_by_label("Edit object · Coming soon").click();
        h.run_steps(8);
        assert_eq!(h.state().ui.generative.operation, photocraft_engine::generative_cmds::Operation::EditSelection);
        assert!(h.query_by_label("Generate · Coming soon").is_some());
        assert!(h.query_by_label("Drag contextual task bar").is_none(), "the bar must not cover the prompt form's controls");
        h.state_mut().ui.generative.open = false;
        h.run_steps(4);
        assert!(h.query_by_label("Drag contextual task bar").is_some());
        h.state_mut().session.execute("layer.smartObjects.convertToSmartObject", json!({})).unwrap();
        h.run_steps(8);
        assert!(h.query_by_label("Remove Background").is_some());
        h.state_mut().session.execute("type.create", json!({"text":"Title","size":24,"x":20,"y":40})).unwrap();
        h.run_steps(8);
        assert!(h.query_by_label("Remove Background").is_none());
    }
    fn photo() -> Vec<u8> {
        let mut bytes = b"P6\n200 160\n255\n".to_vec();
        for y in 0_i32..160 {
            for x in 0_i32..200 {
                let inside = (x - 100).pow(2) + (y - 80).pow(2) < 45 * 45;
                let noise = ((x * 13 + y * 7) % 17) as u8;
                bytes.extend_from_slice(&if inside { [210 + noise, 50 + noise, 30 + noise] } else { [60 + noise, 100 + noise, 170 + noise] });
            }
        }
        bytes
    }

    #[test]
    fn placed_photo_button_click_creates_one_editable_mask_and_drag_does_not_paint() {
        let mut session = photocraft_engine::Session::new();
        session.execute("file.new", json!({"width":200,"height":160})).unwrap();
        photocraft_engine::file_cmds::place_bytes(&mut session, "dropped-photo.ppm", photo(), None, &json!({})).unwrap();
        let mut h = Harness::builder().with_size(vec2(1200.0, 800.0)).with_max_steps(64).build_eframe(move |cc| {
            PhotocraftApp::setup_context(&cc.egui_ctx, Default::default());
            PhotocraftApp::new(session, crate::Services::default())
        });
        h.run_steps(8);
        let layer = h.state().session.active().unwrap().active_layer.unwrap();
        let before = h.state().session.active().unwrap().doc.layer(layer).unwrap().content.clone();
        let steps = h.state().session.active().unwrap().history.past_len();
        let start = h.get_by_label("Drag contextual task bar").rect().center();
        let end = start + vec2(-120.0, -100.0);
        h.event(egui::Event::PointerMoved(start));
        h.run_steps(1);
        h.event(egui::Event::PointerButton { pos: start, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE });
        h.run_steps(1);
        for k in 1..=6 {
            h.event(egui::Event::PointerMoved(start + (end - start) * (k as f32 / 6.0)));
            h.run_steps(1);
        }
        h.event(egui::Event::PointerButton { pos: end, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE });
        h.run_steps(4);
        assert!(h.state().ui.contextual_taskbar.position.is_some());
        assert_eq!(h.state().session.active().unwrap().history.past_len(), steps, "dragging the bar must not paint");
        assert_eq!(h.state().session.active().unwrap().doc.layer(layer).unwrap().content, before);
        h.get_by_label("Remove Background").click();
        h.run_steps(8);
        let state = h.state().session.active().unwrap();
        assert!(state.doc.layer(layer).unwrap().mask.is_some(), "floating action should dispatch the existing command: {}", h.state().ui.status);
        assert_eq!(state.history.past_len(), steps + 1);
        assert_eq!(state.doc.layer(layer).unwrap().content, before);
        h.state_mut().session.execute("edit.undo", json!({})).unwrap();
        assert!(h.state().session.active().unwrap().doc.layer(layer).unwrap().mask.is_none());
    }
}
