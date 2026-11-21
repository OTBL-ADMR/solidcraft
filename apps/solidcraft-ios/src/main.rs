//! SolidCraft on iPad: the shared app (`solidcraft_ui_egui::SolveApp`) in eframe's native runner,
//! drawn with Metal through wgpu. The window is winit's UIKit window; `packaging/ios` puts the
//! binary in an app bundle. Built for `aarch64-apple-ios`; on other systems the binary only says so.

// Built and tested everywhere; only the iOS app uses them.
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
mod files;
#[cfg(any(target_os = "ios", feature = "host-check"))]
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
mod ios;
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
mod prefs_writer;

#[cfg(target_os = "ios")]
fn main() {
    ios::run();
}

#[cfg(not(target_os = "ios"))]
fn main() {
    eprintln!("solidcraft-ios only runs on iPadOS: build it with packaging/ios/build-ipa.sh on a Mac (see packaging/ios/README.md)");
}
