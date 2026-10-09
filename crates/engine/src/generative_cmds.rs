//! Local draft planning for future generative editing. No image provider is enabled, no pixels
//! are sampled or uploaded, and even a connected ChatGPT account cannot enable execution.
use photocraft_geom::Rect;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{EngineError, Result, Session, commands::CommandSpec};

pub const COMING_SOON: &str = "Coming soon: OpenAI's Sign in with ChatGPT preview does not support image generation. No images or prompts are sent.";
pub const MAX_SIDE: u32 = 32_768;
pub const MAX_PIXELS: u64 = 100_000_000;
pub const MAX_PROMPT_CHARS: usize = 2_000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Operation {
    #[default]
    Generate,
    Extend,
    Reframe,
    EditSelection,
}

impl Operation {
    pub fn label(self) -> &'static str {
        match self {
            Self::Generate => "Generate image",
            Self::Extend => "Extend image",
            Self::Reframe => "Generative reframe",
            Self::EditSelection => "Edit selected object",
        }
    }
}

fn bad(msg: impl Into<String>) -> EngineError {
    EngineError::BadParams { cmd: "generative.prepare".into(), msg: msg.into() }
}

pub fn validate_size(width: u32, height: u32) -> Result<()> {
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE || u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(bad("width and height must be 1..32768, with at most 100 million pixels"));
    }
    Ok(())
}

pub fn validate_prompt(prompt: &str) -> Result<()> {
    if prompt.len() > MAX_PROMPT_CHARS * 4 || prompt.chars().count() > MAX_PROMPT_CHARS {
        return Err(bad("prompt must contain at most 2000 characters"));
    }
    Ok(())
}

fn side(p: &Value, key: &str, fallback: u32) -> Result<u32> {
    match p.get(key) {
        None => Ok(fallback),
        Some(v) => v.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or_else(|| bad(format!("{key} must be a positive whole number"))),
    }
}

fn selection_bounds(s: &Session) -> Result<Rect> {
    let doc = &s.active().ok_or(EngineError::NoDocument)?.doc;
    let mask = doc.selection.as_ref().ok_or_else(|| bad("select the object or region to edit first"))?;
    let bounds = if mask.default_pixel().first().is_some_and(|n| *n > 0.0) { doc.bounds() } else { mask.content_bounds().intersect(&doc.bounds()) };
    if bounds.is_empty() {
        return Err(bad("select the object or region to edit first"));
    }
    Ok(bounds)
}

pub fn prepare(s: &Session, p: &Value) -> Result<Value> {
    let fields = p.as_object().ok_or_else(|| bad("parameters must be an object"))?;
    if fields.keys().any(|k| !["operation", "prompt", "width", "height"].contains(&k.as_str())) {
        return Err(bad("only operation, prompt, width and height are accepted"));
    }
    let operation: Operation = serde_json::from_value(p.get("operation").cloned().ok_or_else(|| bad("operation is required"))?)
        .map_err(|_| bad("operation must be generate, extend, reframe or editSelection"))?;
    let prompt = match p.get("prompt") {
        None => "",
        Some(v) => v.as_str().ok_or_else(|| bad("prompt must be text"))?,
    };
    validate_prompt(prompt)?;
    if matches!(operation, Operation::Generate | Operation::EditSelection) && prompt.trim().is_empty() {
        return Err(bad("describe the image or object change first"));
    }
    let source = s.active();
    if operation != Operation::Generate && source.is_none() {
        return Err(EngineError::NoDocument);
    }
    let size = source.map(|st| st.doc.size);
    let width = side(p, "width", size.map_or(1024, |z| z.width))?;
    let height = side(p, "height", size.map_or(1024, |z| z.height))?;
    validate_size(width, height)?;
    if let Some(size) = size {
        if matches!(operation, Operation::Extend | Operation::Reframe) && (width < size.width || height < size.height) {
            return Err(bad("extension and reframing drafts must keep the original image inside the proposed canvas"));
        }
        if operation == Operation::Extend && width == size.width && height == size.height {
            return Err(bad("increase the width or height to extend the image"));
        }
        if operation == Operation::EditSelection && (width != size.width || height != size.height) {
            return Err(bad("selected-object edits keep the current canvas size"));
        }
    }
    let selection = if operation == Operation::EditSelection { Some(selection_bounds(s)?) } else { None };
    let source_info = source.map(|st| json!({"document":st.doc.id, "revision":st.revision, "width":st.doc.size.width, "height":st.doc.size.height}));
    let origin = size.map(|z| [(i64::from(z.width) - i64::from(width)) / 2, (i64::from(z.height) - i64::from(height)) / 2]);
    Ok(json!({
        "operation":operation, "prompt":prompt.trim(), "output":{"width":width,"height":height},
        "source":source_info, "canvasOrigin":origin, "selectionBounds":selection,
        "available":false, "status":"comingSoon", "reason":COMING_SOON,
        "imageSampled":false, "requestSent":false
    }))
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "generative.capabilities",
            label: "Generative image capabilities",
            menu: &[],
            shortcut: None,
            params: "{} → {available:false,status:comingSoon,operations,reason}; no network",
            enabled: |_| Ok(()),
            run: |_, _| Ok(json!({"available":false,"status":"comingSoon","operations":["generate","extend","reframe","editSelection"],"reason":COMING_SOON})),
            journal: false,
        },
        CommandSpec {
            id: "generative.prepare",
            label: "Review generative edit draft",
            menu: &[],
            shortcut: None,
            params: "{operation:generate|extend|reframe|editSelection,prompt:text<=2000,width?:px,height?:px} → local metadata draft; no image sampling, network or document edit",
            enabled: |_| Ok(()),
            run: |s, p| prepare(s, p),
            journal: false,
        },
        CommandSpec {
            id: "generative.run",
            label: "Generate (coming soon)",
            menu: &[],
            shortcut: None,
            params: "unavailable; connecting ChatGPT cannot enable image generation",
            enabled: |_| Err(COMING_SOON.into()),
            run: |_, _| Err(EngineError::Other(COMING_SOON.into())),
            journal: false,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_four_drafts_preserve_pixels_selection_history_and_floating_content() {
        let mut s = Session::new();
        let empty = s.execute("generative.prepare", json!({"operation":"generate","prompt":"A blue bicycle","width":1024,"height":768})).unwrap();
        assert_eq!(empty["source"], Value::Null);
        assert!(s.docs.is_empty());
        s.execute("file.new", json!({"width":200,"height":160})).unwrap();
        s.execute("layer.new.layer", json!({"name":"Photo"})).unwrap();
        s.execute("edit.fill", json!({"color":"#4488cc"})).unwrap();
        s.execute("select.rect", json!({"x":40,"y":30,"width":80,"height":60})).unwrap();
        s.execute("select.float", json!({"cut":true})).unwrap();
        let doc = s.active().unwrap().doc.clone();
        let steps = s.active().unwrap().history.past_len();
        let revision = s.active().unwrap().revision;
        for (operation, width, height) in [("generate", 200, 160), ("extend", 260, 200), ("reframe", 320, 180), ("editSelection", 200, 160)] {
            let draft = s.execute("generative.prepare", json!({"operation":operation,"prompt":"Make it blue","width":width,"height":height})).unwrap();
            assert_eq!(draft["requestSent"], false);
            assert_eq!(draft["available"], false);
            assert_eq!(draft["status"], "comingSoon");
            assert_eq!(s.active().unwrap().doc, doc);
            assert_eq!(s.active().unwrap().history.past_len(), steps);
            assert_eq!(s.active().unwrap().revision, revision);
            assert!(s.active().unwrap().floating.is_some());
            assert!(s.execute("generative.run", draft).is_err());
        }
        assert!(!s.is_enabled("generative.run"));
        assert_eq!(s.execute("generative.capabilities", json!({})).unwrap()["available"], false);
    }

    #[test]
    fn drafts_reject_bad_inputs_empty_selections_and_destructive_geometry_without_edits() {
        let mut s = Session::new();
        for p in [
            json!(null),
            json!([]),
            json!({}),
            json!({"operation":"unknown"}),
            json!({"operation":"generate","prompt":12}),
            json!({"operation":"generate","prompt":""}),
            json!({"operation":"extend"}),
            json!({"operation":"generate","prompt":"x","width":0}),
            json!({"operation":"generate","prompt":"x","width":4294967296_u64}),
            json!({"operation":"generate","prompt":"x","height":1.5}),
            json!({"operation":"generate","prompt":"x","width":32768,"height":32768}),
            json!({"operation":"generate","prompt":"x".repeat(2001)}),
            json!({"operation":"generate","prompt":"x","endpoint":"https://example.invalid"}),
        ] {
            assert!(s.execute("generative.prepare", p).is_err());
            assert!(s.docs.is_empty());
        }
        s.execute("file.new", json!({"width":200,"height":160})).unwrap();
        let doc = s.active().unwrap().doc.clone();
        for p in [
            json!({"operation":"editSelection","prompt":"Change the object"}),
            json!({"operation":"extend"}),
            json!({"operation":"reframe","width":100}),
            json!({"operation":"extend","height":100}),
        ] {
            assert!(s.execute("generative.prepare", p).is_err());
            assert_eq!(s.active().unwrap().doc, doc);
        }
        assert!(validate_prompt(&"🚲".repeat(2000)).is_ok());
        assert!(validate_prompt(&"🚲".repeat(2001)).is_err());
    }
}
