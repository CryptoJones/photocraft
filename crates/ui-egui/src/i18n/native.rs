//! Native language-pack reader and bounded background polling. Drawing never touches the disk.

use super::{
    config::{self, Manifest},
    runtime::{LanguagePack, LocaleSource},
};
use std::{
    collections::{BTreeMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    time::Duration,
};

/// Read only a bounded, regular UTF-8 file. Resource names are flat manifest basenames.
pub(crate) fn read_text(path: &Path, max: usize) -> Result<String, String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !metadata.file_type().is_file() || metadata.len() > max as u64 {
        return Err(format!("{}: expected a regular file of at most {max} bytes", path.display()));
    }
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut text = String::new();
    file.take((max as u64).saturating_add(1)).read_to_string(&mut text).map_err(|e| format!("{}: {e}", path.display()))?;
    if text.len() > max {
        return Err(format!("{}: file grew beyond {max} bytes", path.display()));
    }
    Ok(text)
}

pub(crate) struct Inputs {
    pub manifest: Manifest,
    pub files: BTreeMap<String, String>,
    fingerprint: u64,
}

pub(crate) fn read_inputs(dir: &Path) -> Result<Option<Inputs>, String> {
    let path = dir.join("manifest.json");
    match std::fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", path.display())),
        Ok(_) => {}
    }
    let text = read_text(&path, config::MAX_MANIFEST_BYTES)?;
    let manifest = Manifest::parse(&text)?;
    let mut fingerprint = DefaultHasher::new();
    text.hash(&mut fingerprint);
    let mut files = BTreeMap::new();
    let mut total = 0usize;
    for language in &manifest.languages {
        if let Some(name) = &language.catalog {
            let remaining = config::MAX_PACK_BYTES.saturating_sub(total);
            let text = read_text(&dir.join(name), config::MAX_CATALOG_BYTES.min(remaining))?;
            total = total.checked_add(text.len()).ok_or("language pack size overflow")?;
            name.hash(&mut fingerprint);
            text.hash(&mut fingerprint);
            files.insert(name.clone(), text);
        }
    }
    // Catch a manifest replaced while its files were being read; retry at the next tick.
    if read_text(&path, config::MAX_MANIFEST_BYTES)? != text {
        return Err("language manifest changed during loading; retrying".into());
    }
    Ok(Some(Inputs { manifest, files, fingerprint: fingerprint.finish() }))
}

type Update = Result<Option<Arc<LanguagePack>>, String>;

pub struct Watcher {
    updates: mpsc::Receiver<Update>,
    reloads: mpsc::SyncSender<()>,
}

impl LocaleSource for Watcher {
    fn poll(&mut self) -> Option<Update> {
        self.updates.try_recv().ok()
    }
    fn reload(&mut self) -> Result<(), String> {
        match self.reloads.try_send(()) {
            Ok(()) | Err(mpsc::TrySendError::Full(())) => Ok(()),
            Err(mpsc::TrySendError::Disconnected(())) => Err("language watcher stopped".into()),
        }
    }
}

#[derive(Eq, PartialEq)]
enum Fingerprint {
    Bundle,
    Files(u64),
    Error(String),
}

/// Start watching a directory. Changes to bytes are detected even when size and mtime match.
/// Dropping the service disconnects the request channel and ends its worker without a UI join.
pub fn watch(dir: PathBuf, ctx: egui::Context) -> Result<Watcher, String> {
    let (updates_tx, updates) = mpsc::sync_channel(1);
    let (reloads, reloads_rx) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("photocraft-locales".into())
        .spawn(move || {
            // An absent directory at startup already uses the bundle; don't overwrite a supplied
            // session pack with a redundant initial "restore bundle" event.
            let mut previous = Some(Fingerprint::Bundle);
            let mut interval = Duration::from_millis(1000);
            loop {
                let inputs = read_inputs(&dir);
                let fingerprint = match &inputs {
                    Ok(Some(inputs)) => Fingerprint::Files(inputs.fingerprint),
                    Ok(None) => Fingerprint::Bundle,
                    Err(error) => Fingerprint::Error(error.clone()),
                };
                if previous.as_ref() != Some(&fingerprint) {
                    let update = inputs.and_then(|inputs| match inputs {
                        Some(inputs) => {
                            let pack = LanguagePack::build(inputs.manifest, &inputs.files)?;
                            interval = Duration::from_millis(pack.manifest.reload_interval_ms);
                            Ok(Some(Arc::new(pack)))
                        }
                        None => Ok(None),
                    });
                    match updates_tx.try_send(update) {
                        Ok(()) => {
                            previous = Some(fingerprint);
                            ctx.request_repaint();
                        }
                        Err(mpsc::TrySendError::Full(_)) => {}
                        Err(mpsc::TrySendError::Disconnected(_)) => return,
                    }
                }
                match reloads_rx.recv_timeout(interval) {
                    Ok(()) => previous = None,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }
        })
        .map_err(|e| format!("start language watcher: {e}"))?;
    Ok(Watcher { updates, reloads })
}
