//! macOS title bar dragging. The window draws under a transparent title bar (`main.rs`), so AppKit
//! would otherwise drag the window from anywhere in that strip, menus included: pressing a menu
//! title and dragging down to an item moved the window instead. Turning AppKit's own dragging off
//! leaves only the app's drag region (the free gap between the menus and the controls,
//! `panels::title_bar`), which starts a drag explicitly (`ViewportCommand::StartDrag`, AppKit's
//! `performWindowDragWithEvent:`, which works on an unmovable window). All safe objc2 calls.

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSTitlebarSeparatorStyle, NSWindowTitleVisibility, NSWindowToolbarStyle};
use objc2_foundation::NSProcessInfo;

const LIQUID_GLASS_MIN_MACOS_MAJOR: isize = 27;

fn liquid_glass_supported(major: isize) -> bool {
    major >= LIQUID_GLASS_MIN_MACOS_MAJOR
}

fn macos_major_version() -> isize {
    NSProcessInfo::processInfo().operatingSystemVersion().majorVersion
}

/// Stop AppKit dragging the app's windows by their title bar strip.
pub fn disable_native_title_drag() {
    let Some(mtm) = MainThreadMarker::new() else {
        log::warn!("title bar: not on the main thread; AppKit window dragging left on");
        return;
    };
    for window in NSApplication::sharedApplication(mtm).windows().iter() {
        window.setMovable(false);
    }
}

/// Use the macOS 27+ integrated "Liquid Glass" traffic-light appearance.
pub fn apply_liquid_glass_traffic_lights() {
    if !liquid_glass_supported(macos_major_version()) {
        return;
    }
    let Some(mtm) = MainThreadMarker::new() else {
        log::warn!("title bar: not on the main thread; skipped Liquid Glass traffic lights");
        return;
    };
    for window in NSApplication::sharedApplication(mtm).windows().iter() {
        window.setTitlebarAppearsTransparent(true);
        window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
        window.setToolbarStyle(NSWindowToolbarStyle::UnifiedCompact);
        window.setTitlebarSeparatorStyle(NSTitlebarSeparatorStyle::None);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn liquid_glass_gate_is_macos_27_plus() {
        assert!(!super::liquid_glass_supported(26));
        assert!(super::liquid_glass_supported(27));
    }
}
