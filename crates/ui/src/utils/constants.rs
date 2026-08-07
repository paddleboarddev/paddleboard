use gpui::{Pixels, Window, px};

pub const MACOS_SDK_26_OR_LATER: bool = cfg!(macos_sdk_26_or_later);

// Use pixels here instead of a rem-based size because the macOS traffic
// lights are a static size, and don't scale with the rest of the UI.
//
// Magic number: There is one extra pixel of padding on the left side due to
// the 1px border around the window on macOS apps.
pub const TRAFFIC_LIGHT_PADDING: f32 = if MACOS_SDK_26_OR_LATER { 78. } else { 71. };

/// PaddleBoard: vertical space to reserve above content that reaches the
/// window's top-left corner, so it clears the traffic lights rather than
/// rendering underneath them.
///
/// The horizontal `TRAFFIC_LIGHT_PADDING` above is the right answer for a
/// horizontal strip like a tab bar, which can simply start further right. A
/// left-docked panel cannot — indenting a whole tree by 78px would look broken
/// — so it reserves height instead. Static for the same reason: the buttons
/// are a fixed size and do not scale with the UI.
pub const TRAFFIC_LIGHT_STRIP_HEIGHT: f32 = 36.;

/// Returns the platform-appropriate title bar height.
///
/// On Windows, this returns a fixed height of 32px.
/// On other platforms, it scales with the window's rem size (1.75x) with a minimum of 34px.
#[cfg(not(target_os = "windows"))]
pub fn platform_title_bar_height(window: &Window) -> Pixels {
    (1.75 * window.rem_size()).max(px(34.))
}

#[cfg(target_os = "windows")]
pub fn platform_title_bar_height(_window: &Window) -> Pixels {
    // todo(windows) instead of hard coded size report the actual size to the Windows platform API
    px(32.)
}
