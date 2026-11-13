//! Colors and small reusable building blocks for Lodestone's look.

use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _};
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;

use crate::core::loaders::Loader;

pub const BG: u32 = 0x0e1014;
pub const PANEL: u32 = 0x15181e;
pub const PANEL_2: u32 = 0x1b1f27;
pub const PANEL_3: u32 = 0x232833;
pub const BORDER: u32 = 0x272c37;
pub const TEXT: u32 = 0xe7e9ee;
pub const MUTED: u32 = 0x8c94a3;
pub const FAINT: u32 = 0x5d6473;
pub const ACCENT: u32 = 0x3ddc84;
pub const ACCENT_DIM: u32 = 0x1f6b44;
pub const DANGER: u32 = 0xf2555a;
pub const WARN: u32 = 0xf5b14c;
pub const INFO: u32 = 0x5aa9ff;

/// Pixel font for titles and labels (Silkscreen, SIL Open Font License).
pub const PIXEL: &str = "Silkscreen";

pub fn register_fonts(cx: &mut App) {
    let _ = cx.text_system().add_fonts(vec![
        std::borrow::Cow::Borrowed(include_bytes!("../../assets/fonts/Silkscreen-Regular.ttf").as_slice()),
        std::borrow::Cow::Borrowed(include_bytes!("../../assets/fonts/Silkscreen-Bold.ttf").as_slice()),
        std::borrow::Cow::Borrowed(include_bytes!("../../assets/fonts/PixelifySans.ttf").as_slice()),
        std::borrow::Cow::Borrowed(include_bytes!("../../assets/fonts/VT323-Regular.ttf").as_slice()),
    ]);
}

/// Hard, offset, unblurred drop shadow — the pixel-art look.
pub fn pixel_shadow(offset: f32) -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: Hsla::from(rgb(0x000000)).opacity(0.55),
        offset: point(px(offset), px(offset)),
        blur_radius: px(0.),
        spread_radius: px(0.),
        inset: false,
    }]
}

/// Bevel highlights (light top/left, dark bottom/right) like a game block.
pub fn bevel(d: Div, light: f32, dark: f32, w: f32) -> Div {
    d.relative()
        .child(div().absolute().top_0().left_0().right_0().h(px(w)).bg(Hsla::from(rgb(0xffffff)).opacity(light)))
        .child(div().absolute().top_0().left_0().bottom_0().w(px(w)).bg(Hsla::from(rgb(0xffffff)).opacity(light * 0.7)))
        .child(div().absolute().bottom_0().left_0().right_0().h(px(w)).bg(Hsla::from(rgb(0x000000)).opacity(dark)))
        .child(div().absolute().top_0().right_0().bottom_0().w(px(w)).bg(Hsla::from(rgb(0x000000)).opacity(dark * 0.8)))
}

/// Monospace pixel font for the console (VT323, SIL Open Font License).
pub const MONO: &str = "VT323";
/// Body font (Pixelify Sans, SIL Open Font License).
pub const BODY: &str = "Pixelify Sans";

pub fn loader_color(l: Loader) -> u32 {
    match l {
        Loader::Vanilla => 0x5fbf4a,
        Loader::Fabric => 0xd8b27c,
        Loader::Quilt => 0xa678e6,
        Loader::Forge => 0x6d8fd1,
        Loader::NeoForge => 0xee8a3c,
    }
}

pub fn apply(cx: &mut App) {
    gpui_kit::component::Theme::change(gpui_kit::component::ThemeMode::Dark, None, cx);
    gpui_kit::component::Theme::update(cx, |t| paint(t));
}

fn paint(t: &mut gpui_kit::component::Theme) {
    let c = |v: u32| -> Hsla { rgb(v).into() };
    t.background = c(BG);
    t.foreground = c(TEXT);
    t.border = c(BORDER);
    t.input = c(BORDER);
    t.ring = c(ACCENT_DIM);
    t.muted = c(PANEL_2);
    t.muted_foreground = c(MUTED);
    t.popover = c(PANEL_2);
    t.popover_foreground = c(TEXT);
    t.primary = c(ACCENT);
    t.primary_hover = c(0x5be79a);
    t.primary_active = c(0x2fbf70);
    t.primary_foreground = c(0x06210f);
    t.button_primary = c(ACCENT);
    t.button_primary_hover = c(0x5be79a);
    t.button_primary_active = c(0x2fbf70);
    t.button_primary_foreground = c(0x06210f);
    t.selection = c(0x1f4d36);
    t.secondary = c(PANEL_3);
    t.secondary_hover = c(0x2b3140);
    t.secondary_active = c(0x313848);
    t.secondary_foreground = c(TEXT);
    t.accent = c(PANEL_3);
    t.accent_foreground = c(TEXT);
    // Pixel look: square corners everywhere.
    t.radius = px(0.);
    t.radius_lg = px(0.);
    // Pixel fonts everywhere.
    t.font_family = BODY.into();
    t.mono_font_family = MONO.into();
}

/// Re-apply our palette if the kit re-derived its theme (e.g. on window open
/// or when the system appearance changes).
pub fn ensure(cx: &mut App) {
    let want: Hsla = rgb(ACCENT).into();
    let current: Hsla = gpui_kit::component::Theme::global(cx).tokens.button_primary.into();
    if current != want {
        apply(cx);
    }
}

pub fn icon(name: gpui_kit::assets::IconName) -> Icon {
    Icon::new(name)
}

/// Panel with border and rounded corners.
pub fn card() -> Div {
    div().bg(rgb(PANEL)).border_2().border_color(rgb(BORDER)).shadow(pixel_shadow(4.))
}

pub fn section_title(text: impl Into<SharedString>) -> Div {
    div().text_xs().font_family(PIXEL).text_color(rgb(MUTED)).child(text.into().to_uppercase())
}

pub fn badge(text: impl Into<SharedString>, color: u32) -> Div {
    let col: Hsla = rgb(color).into();
    div()
        .px(px(7.))
        .py(px(1.))
        
        .text_xs()
        .font_family(PIXEL)
        .text_color(col)
        .bg(col.opacity(0.14))
        .border_1()
        .border_color(col.opacity(0.45))
        .child(text.into().to_uppercase())
}

/// Pulsing "Live" pill for running games/servers.
pub fn live_badge() -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .px(px(8.))
        .py(px(2.))
        
        .bg(Hsla::from(rgb(0x0b1a12)).opacity(0.85))
        .border_1()
        .border_color(Hsla::from(rgb(ACCENT)).opacity(0.5))
        .text_xs()
        .font_family(PIXEL)
        .text_color(rgb(ACCENT))
        .child(
            div().size(px(7.)).bg(rgb(ACCENT)).with_animation(
                "live-pulse",
                Animation::new(std::time::Duration::from_millis(1400)).repeat().with_easing(pulsating_between(0.25, 1.0)),
                |d, t| d.opacity(t),
            ),
        )
        .child("LIVE")
}

/// Segmented, blocky progress bar.
pub fn progress_bar(fraction: f32, indeterminate: bool) -> Div {
    const CELLS: usize = 24;
    let filled = if indeterminate {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        ((t / 120) as usize) % CELLS
    } else {
        (fraction.clamp(0.0, 1.0) * CELLS as f32).round() as usize
    };
    div()
        .h(px(12.))
        .w_full()
        .p(px(2.))
        .bg(rgb(0x0a0c0f))
        .border_2()
        .border_color(rgb(BORDER))
        .flex()
        .gap(px(2.))
        .children((0..CELLS).map(move |i| {
            let on = if indeterminate { i == filled || i + 1 == filled || i + 2 == filled } else { i < filled };
            div().flex_1().h_full().bg(if on { rgb(ACCENT) } else { rgb(PANEL_3) })
        }))
}

/// Segmented control: a row of options, one selected.
pub fn segmented<T: Clone + PartialEq + 'static>(
    id: &'static str,
    options: Vec<(T, SharedString)>,
    selected: &T,
    on_pick: impl Fn(T, &mut Window, &mut App) + 'static,
) -> Div {
    let on_pick = std::rc::Rc::new(on_pick);
    div()
        .flex()
        .gap(px(4.))
        .p(px(3.))
        .bg(rgb(PANEL_2))
        .border_2()
        .border_color(rgb(BORDER))
        .children(options.into_iter().enumerate().map(move |(i, (value, label))| {
            let active = &value == selected;
            let on_pick = on_pick.clone();
            div()
                .id((id, i))
                .px(px(12.))
                .py(px(5.))
                
                .text_xs()
                .font_family(PIXEL)
                .cursor_pointer()
                .when(active, |d| d.bg(rgb(ACCENT)).text_color(rgb(0x06210f)))
                .when(!active, |d| d.text_color(rgb(MUTED)).hover(|s| s.text_color(rgb(TEXT))))
                .child(label)
                .on_click(move |_, window, cx| on_pick(value.clone(), window, cx))
        }))
}

/// Square tile with the loader's color and the instance's initial.
pub fn instance_tile(name: &str, loader: Loader, size: f32) -> Div {
    let col: Hsla = rgb(loader_color(loader)).into();
    let letter = name.chars().find(|c| c.is_alphanumeric()).unwrap_or('?').to_ascii_uppercase();
    let tile = div()
        .flex_none()
        .size(px(size))

        .bg(linear_gradient(
            135.,
            linear_color_stop(col.opacity(0.95), 0.),
            linear_color_stop(col.opacity(0.45), 1.),
        ))
        .flex()
        .items_center()
        .justify_center()
        .text_color(rgb(0x0b0d10))
        .font_family(PIXEL)
        .text_size(px(size * 0.42))
        .child(letter.to_string());
    bevel(tile, 0.35, 0.3, (size / 16.).max(2.)).shadow(pixel_shadow(3.))
}

/// An 8×8 pixel-art face generated from the username (original art).
pub fn avatar(name: &str, size: f32) -> Div {
    let h = crate::core::net::sha1_bytes(name.as_bytes());
    let b = hex::decode(&h).unwrap_or_default();
    let byte = |i: usize| b.get(i).copied().unwrap_or(0) as u32;
    let skin = [0xf1c27d, 0xe0ac69, 0xc68642, 0x8d5524, 0xffdbac][(byte(0) % 5) as usize];
    let hair = [0x3b2a1a, 0x6b4423, 0xd4a017, 0x1c1c1c, 0xa0522d, 0x5a3e85, 0x2e86c1][(byte(1) % 7) as usize];
    let eyes = [0x2c5aa0, 0x3d8b37, 0x5b3a1e, 0x222222][(byte(2) % 4) as usize];
    let shirt = 0x30_0000 + byte(3) * 0x101 + byte(4);
    let px_size = size / 8.0;
    let mut rows = Vec::new();
    for y in 0..8 {
        let mut row = Vec::new();
        for x in 0..8 {
            let color = match (x, y) {
                (_, 0) | (_, 1) => hair,
                (0, 2) | (7, 2) => hair,
                (1, 3) | (5, 3) => 0xffffff,
                (2, 3) | (6, 3) => eyes,
                (3..=4, 5) => 0x7a4a2a,
                (_, 7) => shirt | 0x202020,
                _ => skin,
            };
            row.push(div().size(px(px_size)).bg(rgb(color)));
        }
        rows.push(div().flex().children(row));
    }
    div().flex_none().overflow_hidden().flex().flex_col().children(rows)
}

/// The Lodestone logo: a small 3×3 block mark.
pub fn logo(size: f32) -> Div {
    let cells = [0x2b313d, 0x3a4252, 0x2b313d, 0x3a4252, ACCENT, 0x3a4252, 0x2b313d, 0x3a4252, 0x2b313d];
    let s = size / 3.0;
    div()
        .flex_none()
        
        .overflow_hidden()
        .flex()
        .flex_col()
        .children((0..3).map(move |r| div().flex().children((0..3).map(move |c| div().size(px(s)).bg(rgb(cells[r * 3 + c]))))))
}

pub fn small_button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> gpui_kit::component::button::Button {
    gpui_kit::component::button::Button::new(id).label(label).small()
}

pub fn muted(text: impl Into<SharedString>) -> Div {
    div().text_sm().text_color(rgb(MUTED)).child(text.into())
}

pub fn field(label: &'static str, control: impl IntoElement) -> Div {
    div().v_flex().gap(px(6.)).child(div().text_sm().font_weight(FontWeight::MEDIUM).text_color(rgb(TEXT)).child(label)).child(control)
}

pub fn fmt_bytes(n: u64) -> String {
    if n >= 1 << 20 {
        format!("{:.1} MB", n as f64 / (1u64 << 20) as f64)
    } else {
        format!("{:.0} KB", n as f64 / 1024.0)
    }
}

pub fn fmt_count(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1e6)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1e3)
    } else {
        n.to_string()
    }
}

pub fn theme_ok(cx: &App) -> bool {
    let _ = cx.theme();
    true
}
