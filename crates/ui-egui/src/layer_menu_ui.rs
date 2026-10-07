//! Layers panel right-click menu in Photoshop's order. Items are command ids from the menu catalog;
//! unavailable ones are greyed using the same enablement as the main menus. Items marked as menu
//! invocations (`Value::Null` params) go through `menus::invoke`, so dialogs open like in the menu bar.

use photocraft_doc::{Layer, LayerContent};
use serde_json::{Value, json};

/// One entry: label, command id. `None` = separator.
pub type Entry = Option<(&'static str, &'static str)>;

/// The command behind "Add Layer Mask" (the panel button and this menu). Like Photoshop, an active
/// selection becomes the mask; ⌥ inverts it (Hide Selection, or Hide All without a selection).
pub fn add_mask_command(has_selection: bool, alt: bool) -> &'static str {
    match (has_selection, alt) {
        (true, false) => "layer.layerMask.revealSelection",
        (true, true) => "layer.layerMask.hideSelection",
        (false, false) => "layer.layerMask.revealAll",
        (false, true) => "layer.layerMask.hideAll",
    }
}

/// The context menu entries for a layer (Photoshop 2026 order, trimmed to the layer kind).
pub fn entries(l: &Layer, multi: bool, has_selection: bool) -> Vec<Entry> {
    let mut v: Vec<Entry> = vec![Some((tl!("Blending Options…"), "layer.layerStyle.blendingOptions"))];
    v.push(None);
    v.push(Some((if multi { tl_id!("ui-duplicate-layers-5a7a27c703148b52") } else { tl_id!("ui-duplicate-layer-14af5c3fa0d3f69b") }, "layer.duplicate")));
    v.push(Some((if multi { tl_id!("ui-delete-layers-c8252a79bc1e0b16") } else { tl_id!("ui-delete-layer-58b835e7900721e1") }, "layer.delete")));
    if !multi {
        v.push(Some((tl_id!("ui-group-from-layers-09ee0a05b31f9e60"), "layer.groupLayers")));
    }
    v.push(None);
    v.push(Some((tl_id!("ui-quick-export-as-png-1b9ab63313d7e58b"), "layer.quickExportAsPng")));
    v.push(Some((tl_id!("ui-export-as-e134d6c030fdf97d"), "layer.exportAs")));
    v.push(None);
    v.push(Some((tl_id!("ui-artboard-from-layers-c8c4d5e273dc9a46"), "layer.new.artboardFromLayers")));
    v.push(Some((tl_id!("ui-frame-from-layers-03291d8005885e2e"), "layer.new.frameFromLayers")));
    v.push(None);
    v.push(Some((tl_id!("ui-convert-to-smart-object-e3cae5800f8ef641"), "layer.smartObjects.convertToSmartObject")));
    match &l.content {
        LayerContent::Text(_) => v.push(Some(("Rasterize Type", "layer.rasterize.type"))),
        LayerContent::Shape(_) => v.push(Some(("Rasterize Layer", "layer.rasterize.shape"))),
        LayerContent::Smart(_) => v.push(Some(("Rasterize Layer", "layer.rasterize.smartObject"))),
        LayerContent::Fill(_) => v.push(Some(("Rasterize Layer", "layer.rasterize.fillContent"))),
        _ => v.push(Some(("Rasterize Layer", "layer.rasterize.layer"))),
    }
    v.push(None);
    if l.mask.is_some() {
        v.push(Some((tl_id!("ui-disable-layer-mask-3b0148ac5d668494"), "layer.layerMask.enabled")));
        v.push(Some((tl_id!("ui-apply-layer-mask-f52f96ef823c3c34"), "layer.layerMask.apply")));
        v.push(Some((tl_id!("ui-delete-layer-mask-0f007ecddce73047"), "layer.layerMask.delete")));
    } else {
        v.push(Some((tl_id!("ui-add-layer-mask-b657b676905bb6f5"), add_mask_command(has_selection, false))));
    }
    v.push(Some((
        if l.clipped { tl_id!("ui-release-clipping-mask-d9511a1a14a1c766") } else { tl_id!("ui-create-clipping-mask-dab2a1401549f8a1") },
        if l.clipped { "layer.releaseClippingMask" } else { "layer.createClippingMask" },
    )));
    v.push(None);
    v.push(Some((tl_id!("ui-link-layers-9c332d93a2dd3153"), "layer.linkLayers")));
    v.push(Some((tl_id!("ui-select-linked-layers-1372c4229a2852ea"), "layer.selectLinkedLayers")));
    v.push(None);
    v.push(Some((tl_id!("ui-copy-layer-style-e68cff0a57a03ef6"), "layer.layerStyle.copyLayerStyle")));
    v.push(Some((tl_id!("ui-paste-layer-style-156525152f887c82"), "layer.layerStyle.pasteLayerStyle")));
    v.push(Some((tl_id!("ui-clear-layer-style-2e46522acfabe670"), "layer.layerStyle.clear")));
    v.push(None);
    if multi {
        v.push(Some((tl_id!("ui-merge-layers-5abfd499e10d5fa9"), "layer.mergeLayers")));
    } else {
        v.push(Some((tl_id!("ui-merge-down-d6cf94561a2ce0db"), "layer.mergeDown")));
    }
    v.push(Some((tl_id!("ui-merge-visible-4c4ec6fdf576bb73"), "layer.mergeVisible")));
    v.push(Some((tl_id!("ui-flatten-image-f50ab81b0b01d49a"), "layer.flattenImage")));
    v
}

/// Render the menu. Pushes `(command, params)` actions; `Value::Null` params mean "invoke like the
/// menu item" (opens the command's dialog when it has one).
pub fn show(app: &crate::PhotocraftApp, ui: &mut egui::Ui, l: &Layer, on_set: bool, actions: &mut Vec<(String, Value)>) -> bool {
    ui.set_min_width(220.0);
    let mut rename = false;
    let mut last_sep = true;
    let has_selection = app.session.active().is_some_and(|s| s.doc.selection.is_some());
    for e in entries(l, on_set, has_selection) {
        match e {
            None => {
                if !last_sep {
                    ui.separator();
                }
                last_sep = true;
            }
            Some((label, id)) => {
                // Skip commands this build doesn't have rather than showing dead items.
                if photocraft_engine::commands::find(id).is_none() && !crate::menu_catalog::CATALOG.iter().any(|m| m.3 == id) {
                    continue;
                }
                last_sep = false;
                // Enablement is exact for the active layer (or the selection); another row is
                // selected first when clicked, so its items stay available.
                let is_active = app.session.active().is_some_and(|s| s.active_layer == Some(l.id));
                let enabled = if on_set || is_active { crate::menus::is_enabled(app, id) } else { true };
                if ui.add_enabled(enabled, egui::Button::new(tl!(&label))).clicked() {
                    if !on_set {
                        actions.push(("layer.select".into(), json!({"layer": l.id.0})));
                    }
                    actions.push((id.into(), Value::Null));
                    ui.close();
                }
            }
        }
    }
    ui.separator();
    if ui.button(tl_id!("ui-rename-layer-878203cf79b1de3c")).clicked() {
        rename = true;
        ui.close();
    }
    rename
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_follow_layer_state() {
        let mut s = photocraft_engine::Session::new();
        s.execute("file.new", json!({"width": 10, "height": 10})).unwrap();
        s.execute("layer.new.layer", json!({})).unwrap();
        let st = s.active().unwrap();
        let l = st.doc.layer(st.active_layer.unwrap()).unwrap().clone();
        let ids: Vec<&str> = entries(&l, false, false).into_iter().flatten().map(|e| e.1).collect();
        assert_eq!(ids.first(), Some(&"layer.layerStyle.blendingOptions"));
        assert!(ids.contains(&"layer.mergeDown") && !ids.contains(&"layer.mergeLayers"));
        assert!(ids.contains(&"layer.layerMask.revealAll"));
        let ids: Vec<&str> = entries(&l, false, true).into_iter().flatten().map(|e| e.1).collect();
        assert!(ids.contains(&"layer.layerMask.revealSelection") && !ids.contains(&"layer.layerMask.revealAll"));
        let ids: Vec<&str> = entries(&l, true, false).into_iter().flatten().map(|e| e.1).collect();
        assert!(ids.contains(&"layer.mergeLayers"));
    }

    #[test]
    fn add_mask_uses_the_selection() {
        assert_eq!(add_mask_command(false, false), "layer.layerMask.revealAll");
        assert_eq!(add_mask_command(false, true), "layer.layerMask.hideAll");
        assert_eq!(add_mask_command(true, true), "layer.layerMask.hideSelection");
        // With a selection the new mask is the selection: inside revealed, outside hidden.
        let mut s = photocraft_engine::Session::new();
        s.execute("file.new", json!({"width": 10, "height": 10})).unwrap();
        s.execute("layer.new.layer", json!({})).unwrap();
        s.execute("select.rect", json!({"x": 0, "y": 0, "width": 4, "height": 10})).unwrap();
        s.execute(add_mask_command(true, false), json!({})).unwrap();
        let st = s.active().unwrap();
        let mask = &st.doc.layer(st.active_layer.unwrap()).unwrap().mask.as_ref().unwrap().surface;
        assert!(mask.pixel(1, 5)[0] > 0.99 && mask.pixel(8, 5)[0] < 0.01);
    }

    #[test]
    fn every_entry_is_a_known_command() {
        let mut s = photocraft_engine::Session::new();
        s.execute("file.new", json!({"width": 10, "height": 10})).unwrap();
        let l = s.active().unwrap().doc.layers[0].clone();
        for (_, id) in entries(&l, false, true).into_iter().chain(entries(&l, false, false)).flatten() {
            assert!(crate::menu_catalog::CATALOG.iter().any(|m| m.3 == id) || photocraft_engine::commands::find(id).is_some(), "{id}");
        }
    }
}
