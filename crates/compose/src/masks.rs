//! A layer's effective mask (pixel mask × vector mask, each feathered) as one surface, for
//! backends that sample masks from textures (the GPU compositor) and for feathered masks on the
//! CPU. Rasterising a vector mask costs a path coverage pass and feathering a blur, so results
//! are cached per mask state; unchanged masks return the same tiles (cheap to clone, and the
//! GPU sees no change). Retention is bounded by both entry count and accounted pixel bytes;
//! [] drops cached masks and prevents older builds from refilling the cache.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, OnceLock, Weak};

use photocraft_color::{ColorMode, PixelFormat, SampleType};
use photocraft_doc::Layer;
use photocraft_geom::Rect;
use photocraft_raster::Surface;

const FORMAT: PixelFormat = PixelFormat { mode: ColorMode::Grayscale, sample: SampleType::F32, alpha: false };

struct CacheEntry {
    surface: Surface,
    tick: u64,
    bytes: usize,
    // The key contains input tile addresses. Keep those tiles alive until eviction:
    // edits must copy them, and the allocator cannot reuse their addresses for new tiles.
    _pixel_input: Option<Surface>,
}

#[derive(Default)]
struct Cache {
    map: HashMap<u64, CacheEntry>,
    in_flight: HashMap<u64, Weak<OnceLock<Surface>>>,
    tick: u64,
    bytes: usize,
    generation: u64,
}

impl Cache {
    fn get(&mut self, key: u64) -> Option<Surface> {
        self.tick = self.tick.saturating_add(1);
        let entry = self.map.get_mut(&key)?;
        entry.tick = self.tick;
        Some(entry.surface.clone())
    }

    fn insert(&mut self, key: u64, surface: Surface, bytes: usize, budget: usize, generation: u64, _pixel_input: Option<Surface>) {
        if bytes > budget || generation != self.generation {
            return;
        }
        if let Some(previous) = self.map.remove(&key) {
            self.bytes = self.bytes.saturating_sub(previous.bytes);
        }
        while self.map.len() >= CAPACITY || self.bytes > budget.saturating_sub(bytes) {
            let Some(old) = self.map.iter().min_by_key(|(_, entry)| entry.tick).map(|(&key, _)| key) else { break };
            if let Some(entry) = self.map.remove(&old) {
                self.bytes = self.bytes.saturating_sub(entry.bytes);
            }
        }
        self.bytes = self.bytes.saturating_add(bytes);
        self.tick = self.tick.saturating_add(1);
        self.map.insert(key, CacheEntry { surface, tick: self.tick, bytes, _pixel_input });
    }

    fn clear(&mut self) -> usize {
        let bytes = self.bytes;
        self.map.clear();
        self.in_flight.clear();
        self.bytes = 0;
        self.tick = 0;
        self.generation = self.generation.wrapping_add(1);
        bytes
    }
}

fn cache() -> &'static Mutex<Cache> {
    static C: OnceLock<Mutex<Cache>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(Cache::default()))
}

/// Entries kept (least recently used dropped first).
const CAPACITY: usize = 64;

/// Accounted pixel-byte limit of the combined-mask cache. One 36 MP float input and result
/// fit; larger individual results remain usable but are not retained in the cache.
pub const CACHE_BUDGET: usize = 512 << 20;

fn surface_bytes(surface: &Surface) -> usize {
    surface.tiles().fold(surface.default_bytes().len(), |bytes, (_, tile)| bytes.saturating_add(tile.bytes().len()))
}

fn entry_bytes(surface: &Surface, pixel_input: Option<&Surface>) -> usize {
    surface_bytes(surface).saturating_add(pixel_input.map_or(0, surface_bytes))
}

/// Pixel bytes accounted for by the combined-mask cache. This conservatively includes source
/// tiles and counts tiles shared across entries more than once; it is not process memory use.
pub fn cache_bytes() -> usize {
    cache().lock().unwrap_or_else(|e| e.into_inner()).bytes
}

/// Drop every cached combined mask (Edit › Purge › All), returning its accounted pixel bytes.
/// Callers may still hold returned surfaces, so this is not a measure of freed process memory.
pub fn purge_cache() -> usize {
    cache().lock().unwrap_or_else(|e| e.into_inner()).clear()
}

enum CachedMask {
    Hit(Surface),
    Pending(Arc<OnceLock<Surface>>),
}

fn lookup(c: &Mutex<Cache>, k: u64) -> (CachedMask, u64) {
    let mut c = c.lock().unwrap_or_else(|e| e.into_inner());
    let generation = c.generation;
    if let Some(surface) = c.get(k) {
        return (CachedMask::Hit(surface), generation);
    }
    c.in_flight.retain(|_, slot| slot.strong_count() > 0);
    if let Some(slot) = c.in_flight.get(&k).and_then(Weak::upgrade) {
        return (CachedMask::Pending(slot), generation);
    }
    let slot = Arc::new(OnceLock::new());
    c.in_flight.insert(k, Arc::downgrade(&slot));
    (CachedMask::Pending(slot), generation)
}

fn finish_mask(c: &Mutex<Cache>, k: u64, slot: &Arc<OnceLock<Surface>>, pixel_input: Option<Surface>, generation: u64, build: impl FnOnce() -> Surface) -> Surface {
    slot.get_or_init(|| {
        let surface = build();
        let mut c = c.lock().unwrap_or_else(|e| e.into_inner());
        if c.in_flight.get(&k).and_then(Weak::upgrade).is_some_and(|active| Arc::ptr_eq(&active, slot)) {
            let bytes = entry_bytes(&surface, pixel_input.as_ref());
            c.insert(k, surface.clone(), bytes, CACHE_BUDGET, generation, pixel_input);
        }
        surface
    })
    .clone()
}

fn cached_mask(c: &Mutex<Cache>, k: u64, pixel_input: Option<Surface>, build: impl FnOnce() -> Surface) -> Surface {
    let (status, generation) = lookup(c, k);
    match status {
        CachedMask::Hit(surface) => surface,
        CachedMask::Pending(slot) => finish_mask(c, k, &slot, pixel_input, generation, build),
    }
}

fn key(layer: &Layer, canvas: Rect) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    layer.id.0.hash(&mut h);
    (canvas.x0, canvas.y0, canvas.x1, canvas.y1).hash(&mut h);
    format!("{:?}", layer.vector_mask).hash(&mut h);
    if let Some(m) = &layer.mask {
        (m.enabled, m.density.to_bits(), m.feather.to_bits()).hash(&mut h);
        format!("{:?}", m.surface.default_pixel()).hash(&mut h);
        for (c, t) in m.surface.tiles() {
            (c.tx, c.ty, Arc::as_ptr(t) as usize).hash(&mut h);
        }
    }
    h.finish()
}

pub fn feather_sigma(px: f32) -> f32 {
    if px.is_finite() { (px * FEATHER_SIGMA).clamp(0.0, MAX_FEATHER_SIGMA) } else { 0.0 }
}

const FEATHER_SIGMA: f32 = 1.0;
pub const MAX_FEATHER_SIGMA: f32 = 1000.0 * FEATHER_SIGMA;

pub fn has_feather(layer: &Layer) -> bool {
    layer.mask.as_ref().is_some_and(|m| m.enabled && feather_sigma(m.feather) > 0.0)
        || layer.vector_mask.as_ref().is_some_and(|v| v.enabled && feather_sigma(v.feather) > 0.0)
}

fn gaussian(v: &mut [f32], w: usize, h: usize, sigma: f32) {
    if sigma <= 0.0 || w == 0 || h == 0 {
        return;
    }
    let ideal = (12.0 * sigma * sigma / 3.0 + 1.0).sqrt();
    let r = (((ideal.floor() as usize) | 1).max(1) - 1) / 2;
    if r == 0 {
        return;
    }
    let mut line = Vec::new();
    let mut pass = |v: &mut [f32], len: usize, count: usize, at: &dyn Fn(usize, usize) -> usize| {
        for k in 0..count {
            for _ in 0..3 {
                line.clear();
                line.extend((0..len).map(|i| v[at(k, i)]));
                let get = |i: isize| line[i.clamp(0, len as isize - 1) as usize];
                let mut acc: f32 = (-(r as isize)..=r as isize).map(get).sum();
                let n = (2 * r + 1) as f32;
                for i in 0..len {
                    v[at(k, i)] = acc / n;
                    acc += get(i as isize + r as isize + 1) - get(i as isize - r as isize);
                }
            }
        }
    };
    pass(v, w, h, &|k, i| k * w + i);
    pass(v, h, w, &|k, i| i * w + k);
}

pub fn combined_mask(layer: &Layer, canvas: Rect) -> Option<Surface> {
    if !layer.vector_mask.as_ref().is_some_and(|v| v.enabled) && !has_feather(layer) {
        return None;
    }
    let pixel_input = layer.mask.as_ref().map(|m| m.surface.clone());
    Some(cached_mask(cache(), key(layer, canvas), pixel_input, || build_mask(layer, canvas)))
}

fn build_mask(layer: &Layer, canvas: Rect) -> Surface {
    let vm = layer.vector_mask.as_ref().filter(|v| v.enabled);
    let pixel = layer.mask.as_ref().filter(|m| m.enabled);
    let (sv, sp) = (vm.map_or(0.0, |v| feather_sigma(v.feather)), pixel.map_or(0.0, |m| feather_sigma(m.feather)));
    let compiled = vm.map(photocraft_vector::CompiledVectorMask::new);
    let far = Rect::from_xywh(canvas.x0 - 1_000_000, canvas.y0 - 1_000_000, 1, 1);
    let v_out = compiled.as_ref().map_or(1.0, |vm| vm.render_sequential(far)[0]);
    let p_def = pixel.map_or(1.0, |m| {
        let d = m.surface.default_pixel().first().copied().unwrap_or(1.0);
        1.0 - m.density * (1.0 - d)
    });
    let grow = |r: Rect, sigma: f32| {
        let m = (sigma * 3.0).ceil() as i32 + 2;
        if r.is_empty() { r } else { Rect::new(r.x0.saturating_sub(m), r.y0.saturating_sub(m), r.x1.saturating_add(m), r.y1.saturating_add(m)) }
    };
    let mut area = match vm.and_then(|vm| vm.path.control_bounds()) {
        Some((x0, y0, x1, y1)) => grow(Rect::new(x0.floor() as i32 - 2, y0.floor() as i32 - 2, x1.ceil() as i32 + 2, y1.ceil() as i32 + 2), sv),
        None => Rect::EMPTY,
    };
    if let Some(m) = pixel
        && v_out > 0.0
    {
        let b = grow(m.surface.content_bounds(), sp);
        area = if area.is_empty() {
            b
        } else if b.is_empty() {
            area
        } else {
            area.union(&b)
        };
    }
    let area = area.intersect(&canvas);
    let mut s = Surface::with_default(FORMAT, &[p_def * v_out]);
    if !area.is_empty() {
        let (w, h) = (area.width() as usize, area.height() as usize);
        let mut v = match &compiled {
            Some(vm) => vm.render_sequential(area),
            None => vec![1.0; w * h],
        };
        gaussian(&mut v, w, h, sv);
        if let Some(m) = pixel {
            let mut pm = Vec::new();
            m.values_into(area, &mut pm);
            gaussian(&mut pm, w, h, sp);
            for (a, b) in v.iter_mut().zip(pm) {
                *a *= b;
            }
        }
        s.write_region(area, &v);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use photocraft_doc::LayerMask;
    use photocraft_doc::vector::{Knot, Path, Subpath, VectorMask};

    fn pending(c: &Mutex<Cache>, k: u64) -> Arc<OnceLock<Surface>> {
        match lookup(c, k).0 {
            CachedMask::Pending(slot) => slot,
            CachedMask::Hit(_) => panic!("expected a cold mask"),
        }
    }

    fn test_surface() -> Surface {
        let mut surface = Surface::new(FORMAT);
        surface.fill_rect(Rect::new(0, 0, 16, 16), &[0.5]);
        surface
    }

    fn one_tile(sample: SampleType) -> Surface {
        let mut surface = Surface::with_default(PixelFormat { mode: ColorMode::Grayscale, sample, alpha: false }, &[1.0]);
        surface.fill_rect(Rect::new(0, 0, 1, 1), &[0.0]);
        surface
    }

    #[test]
    fn charges_input_and_output_at_every_depth_including_off_canvas_tiles() {
        let result = one_tile(SampleType::F32);
        for sample in SampleType::ALL {
            let mut input = one_tile(sample);
            input.fill_rect(Rect::new(1024, 1024, 1025, 1025), &[0.0]);
            let tile_pixels = (photocraft_geom::TILE_SIZE * photocraft_geom::TILE_SIZE) as usize;
            let expected = tile_pixels * (4 + 2 * sample.bytes()) + 4 + sample.bytes();
            let bytes = entry_bytes(&result, Some(&input));
            assert_eq!(bytes, expected, "{sample:?}");
            let mut cache = Cache::default();
            cache.insert(1, result.clone(), bytes, bytes * 2, cache.generation, None);
            cache.insert(2, result.clone(), bytes, bytes * 2, cache.generation, None);
            assert_eq!(cache.bytes, bytes * 2, "shared tiles are conservatively charged per entry");
        }
    }

    #[test]
    fn byte_budget_evicts_the_least_recently_used_entry() {
        let surface = one_tile(SampleType::F32);
        let bytes = entry_bytes(&surface, None);
        let mut cache = Cache::default();
        cache.insert(1, surface.clone(), bytes, bytes * 2, cache.generation, None);
        cache.insert(2, surface.clone(), bytes, bytes * 2, cache.generation, None);
        assert!(cache.get(1).is_some());
        cache.insert(3, surface, bytes, bytes * 2, cache.generation, None);
        assert!(cache.get(2).is_none());
        assert!(cache.get(1).is_some());
        assert!(cache.get(3).is_some());
        assert_eq!(cache.bytes, bytes * 2);
    }

    #[test]
    fn entry_limit_also_bounds_masks_with_no_tiles() {
        let surface = Surface::new(FORMAT);
        let bytes = entry_bytes(&surface, None);
        let mut cache = Cache::default();
        for key in 0..CAPACITY as u64 {
            cache.insert(key, surface.clone(), bytes, CACHE_BUDGET, cache.generation, None);
        }
        assert!(cache.get(0).is_some());
        cache.insert(CAPACITY as u64, surface, bytes, CACHE_BUDGET, cache.generation, None);
        assert_eq!(cache.map.len(), CAPACITY);
        assert!(cache.get(1).is_none());
        assert!(cache.get(0).is_some());
        assert_eq!(cache.bytes, bytes * CAPACITY);
    }

    #[test]
    fn oversized_results_are_usable_without_retention_or_other_eviction() {
        let surface = one_tile(SampleType::F32);
        let bytes = entry_bytes(&surface, None);
        let mut cache = Cache::default();
        let small = Surface::new(FORMAT);
        let small_bytes = entry_bytes(&small, None);
        cache.insert(1, small, small_bytes, bytes - 1, cache.generation, None);
        let tile = surface.tiles().next().unwrap().1.clone();
        let owners = Arc::strong_count(&tile);
        cache.insert(2, surface.clone(), bytes, bytes - 1, cache.generation, None);
        assert!(cache.get(2).is_none());
        assert!(cache.get(1).is_some());
        assert_eq!(cache.bytes, small_bytes);
        assert_eq!(Arc::strong_count(&tile), owners);
        assert_eq!(surface.pixel(0, 0), vec![0.0]);
    }

    #[test]
    fn replacement_and_purge_release_accounted_bytes_and_tile_references() {
        let first = one_tile(SampleType::U8);
        let second = one_tile(SampleType::F32);
        let first_bytes = entry_bytes(&first, None);
        let second_bytes = entry_bytes(&second, None);
        let first_tile = first.tiles().next().unwrap().1.clone();
        let second_tile = second.tiles().next().unwrap().1.clone();
        let mut cache = Cache::default();
        cache.insert(1, first.clone(), first_bytes, CACHE_BUDGET, cache.generation, None);
        assert_eq!(Arc::strong_count(&first_tile), 3);
        cache.insert(1, second.clone(), second_bytes, CACHE_BUDGET, cache.generation, None);
        assert_eq!(cache.bytes, second_bytes);
        assert_eq!(cache.map.len(), 1);
        assert_eq!(Arc::strong_count(&first_tile), 2);
        assert_eq!(Arc::strong_count(&second_tile), 3);
        assert_eq!(cache.clear(), second_bytes);
        assert_eq!(cache.bytes, 0);
        assert!(cache.map.is_empty());
        assert_eq!(Arc::strong_count(&second_tile), 2);
        assert_eq!(cache.clear(), 0);
    }

    #[test]
    fn a_build_started_before_purge_cannot_refill_the_cache() {
        let surface = one_tile(SampleType::F32);
        let bytes = entry_bytes(&surface, None);
        let mut cache = Cache::default();
        assert!(cache.get(1).is_none());
        let generation_before_purge = cache.generation;
        cache.clear();
        cache.insert(1, surface.clone(), bytes, CACHE_BUDGET, generation_before_purge, None);
        assert!(cache.map.is_empty());
        assert_eq!(cache.bytes, 0);
        cache.insert(1, surface, bytes, CACHE_BUDGET, cache.generation, None);
        assert!(cache.get(1).is_some());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn cold_callers_share_one_build_without_retaining_a_second_cache() {
        use std::sync::Barrier;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let c = Mutex::new(Cache::default());
        let slots: Vec<_> = (0..8).map(|_| pending(&c, 7)).collect();
        assert!(slots.iter().all(|slot| Arc::ptr_eq(slot, &slots[0])));
        let barrier = Barrier::new(slots.len());
        let builds = AtomicUsize::new(0);
        let outputs = std::thread::scope(|scope| {
            let handles: Vec<_> = slots
                .iter()
                .map(|slot| {
                    let (c, barrier, builds) = (&c, &barrier, &builds);
                    scope.spawn(move || {
                        barrier.wait();
                        finish_mask(c, 7, slot, None, 0, || {
                            builds.fetch_add(1, Ordering::SeqCst);
                            test_surface()
                        })
                    })
                })
                .collect();
            handles.into_iter().map(|handle| handle.join().unwrap()).collect::<Vec<_>>()
        });
        assert_eq!(builds.load(Ordering::SeqCst), 1);
        assert!(outputs.iter().all(|surface| Arc::ptr_eq(surface.tiles().next().unwrap().1, outputs[0].tiles().next().unwrap().1)));

        c.lock().unwrap().map.clear();
        let again = finish_mask(&c, 7, &slots[0], None, 0, || panic!("rebuilt an active slot"));
        assert!(Arc::ptr_eq(again.tiles().next().unwrap().1, outputs[0].tiles().next().unwrap().1));
        drop(slots);
        let next = pending(&c, 8);
        assert_eq!(c.lock().unwrap().in_flight.len(), 1);
        drop(next);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn distinct_keys_build_while_another_key_is_paused() {
        use std::sync::mpsc;
        use std::time::Duration;

        let c = Mutex::new(Cache::default());
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (second_tx, second_rx) = mpsc::channel();
        std::thread::scope(|scope| {
            let c = &c;
            let first = scope.spawn(move || {
                cached_mask(c, 1, None, || {
                    started_tx.send(()).unwrap();
                    release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                    test_surface()
                })
            });
            started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            let second = scope.spawn(move || {
                let surface = cached_mask(c, 2, None, test_surface);
                second_tx.send(surface.sample_channel(0, 0, 0)).unwrap();
            });
            assert_eq!(second_rx.recv_timeout(Duration::from_secs(5)).unwrap(), 0.5);
            release_tx.send(()).unwrap();
            first.join().unwrap();
            second.join().unwrap();
        });
    }

    #[test]
    fn obsolete_in_flight_build_does_not_repopulate_the_cache() {
        let c = Mutex::new(Cache::default());
        let old = pending(&c, 1);
        c.lock().unwrap().in_flight.clear();
        let current = pending(&c, 1);
        assert!(!Arc::ptr_eq(&old, &current));
        let old_surface = finish_mask(&c, 1, &old, None, 0, test_surface);
        assert!(c.lock().unwrap().map.is_empty());
        let current_surface = finish_mask(&c, 1, &current, None, 0, test_surface);
        assert!(!Arc::ptr_eq(old_surface.tiles().next().unwrap().1, current_surface.tiles().next().unwrap().1));
        assert_eq!(c.lock().unwrap().map.len(), 1);
    }

    #[test]
    fn pixel_edits_refresh_cached_masks_at_every_depth() {
        let canvas = Rect::new(0, 0, 64, 64);
        for sample in [SampleType::U8, SampleType::U16, SampleType::F32] {
            let mut layer = Layer::raster("edited mask", PixelFormat::RGBA8);
            let mut mask = LayerMask::hide_all();
            mask.surface = Surface::new(PixelFormat { sample, ..FORMAT });
            mask.surface.fill_rect(canvas, &[1.0]);
            mask.feather = 4.0;
            layer.mask = Some(mask);

            let first = combined_mask(&layer, canvas).unwrap();
            assert!((first.sample_channel(32, 32, 0) - 1.0).abs() < 1e-6);
            drop(first);
            layer.mask.as_mut().unwrap().surface.fill_rect(canvas, &[0.0]);
            let edited = combined_mask(&layer, canvas).unwrap();
            assert_eq!(edited.sample_channel(32, 32, 0), 0.0, "{sample:?}");

            layer.mask.as_mut().unwrap().surface.fill_rect(canvas, &[0.5]);
            let repainted = combined_mask(&layer, canvas).unwrap();
            assert!((repainted.sample_channel(32, 32, 0) - 0.5).abs() < 0.005, "{sample:?}");
            let warm = combined_mask(&layer, canvas).unwrap();
            assert!(repainted.tiles().zip(warm.tiles()).all(|((_, a), (_, b))| Arc::ptr_eq(a, b)));
        }
    }

    #[test]
    fn matches_the_cpu_mask_and_is_cached() {
        let mut l = Layer::raster("l", PixelFormat::RGBA8);
        l.mask = Some(LayerMask::reveal_all());
        let b = Rect::new(0, 0, 20, 20);
        l.vector_mask = Some(VectorMask::new(Path::new(vec![Subpath::new(vec![
            Knot::corner(5.0, 5.0),
            Knot::corner(15.0, 5.0),
            Knot::corner(15.0, 15.0),
            Knot::corner(5.0, 15.0),
        ])])));
        let m = combined_mask(&l, b).unwrap();
        assert_eq!(m.bounds(), b);
        let m2 = combined_mask(&l, b).unwrap();
        assert!(m.tiles().zip(m2.tiles()).all(|((_, a), (_, b))| Arc::ptr_eq(a, b)));
    }
}
