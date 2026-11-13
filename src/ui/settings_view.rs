use gpui_kit::assets::IconName as I;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::StyledExt as _;
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;

use super::theme::*;
use super::{LodestoneApp, page_header};
use crate::core::paths::paths;
use crate::core::settings::{Settings, total_memory_mb};

pub struct SettingsInputs {
    pub max_mem: Entity<InputState>,
    pub min_mem: Entity<InputState>,
    pub java: Entity<InputState>,
    pub jvm_args: Entity<InputState>,
    pub width: Entity<InputState>,
    pub height: Entity<InputState>,
}

impl SettingsInputs {
    pub fn new(s: &Settings, window: &mut Window, cx: &mut Context<LodestoneApp>) -> Self {
        let mk = |v: String, ph: &'static str, window: &mut Window, cx: &mut Context<LodestoneApp>| cx.new(|cx| InputState::new(window, cx).placeholder(ph).default_value(v));
        SettingsInputs {
            max_mem: mk(s.max_memory_mb.to_string(), "4096", window, cx),
            min_mem: mk(s.min_memory_mb.to_string(), "512", window, cx),
            java: mk(s.java_override.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(), "Automatic (Mojang's Java runtime)", window, cx),
            jvm_args: mk(s.extra_jvm_args.clone(), "Optional, for every instance", window, cx),
            width: mk(s.width.map(|w| w.to_string()).unwrap_or_default(), "Default", window, cx),
            height: mk(s.height.map(|h| h.to_string()).unwrap_or_default(), "Default", window, cx),
        }
    }
}

impl LodestoneApp {
    fn save_settings(&mut self, cx: &mut Context<Self>) {
        let i = &self.settings_inputs;
        let read = |e: &Entity<InputState>, cx: &Context<Self>| e.read(cx).value().trim().to_string();
        let max: Option<u32> = read(&i.max_mem, cx).parse().ok();
        let min: Option<u32> = read(&i.min_mem, cx).parse().ok();
        let (Some(max), Some(min)) = (max, min) else {
            self.notify_err("Memory values must be numbers of megabytes");
            cx.notify();
            return;
        };
        if max < 512 || min > max {
            self.notify_err("Maximum memory must be at least 512 MB and not below the minimum");
            cx.notify();
            return;
        }
        let java = read(&i.java, cx);
        self.settings.max_memory_mb = max;
        self.settings.min_memory_mb = min;
        self.settings.java_override = if java.is_empty() { None } else { Some(java.into()) };
        self.settings.extra_jvm_args = read(&i.jvm_args, cx);
        self.settings.width = read(&i.width, cx).parse().ok();
        self.settings.height = read(&i.height, cx).parse().ok();
        self.settings.save();
        self.notify_ok("Settings saved");
        cx.notify();
    }

    pub fn settings_view(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let i = &self.settings_inputs;
        let total = total_memory_mb();
        let snapshots = self.settings.show_snapshots;
        let old = self.settings.show_old;
        let root = paths().root.clone();
        let root_s = root.to_string_lossy().to_string();

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(page_header("Settings", "Defaults for every instance. Each instance can override memory and JVM arguments in its Options tab.", vec![]))
            .child(
                div().id("settings-scroll").flex_1().overflow_y_scrollbar().child(
                    div()
                        .px(px(32.))
                        .pb(px(32.))
                        .v_flex()
                        .gap(px(14.))
                        .max_w(px(820.))
                        .child(
                            card()
                                .p(px(18.))
                                .v_flex()
                                .gap(px(14.))
                                .child(section_title("Java & memory"))
                                .child(div().flex().gap(px(14.)).child(div().flex_1().child(field("Maximum memory (MB)", Input::new(&i.max_mem)))).child(div().flex_1().child(field("Minimum memory (MB)", Input::new(&i.min_mem)))))
                                .child(muted(format!("This computer has {} MB of RAM. 4096–8192 MB is plenty for most modpacks.", total)))
                                .child(field("Java executable", Input::new(&i.java)))
                                .child(muted("Leave empty and Lodestone downloads the exact Java each Minecraft version needs."))
                                .child(field("Extra JVM arguments", Input::new(&i.jvm_args))),
                        )
                        .child(
                            card()
                                .p(px(18.))
                                .v_flex()
                                .gap(px(14.))
                                .child(section_title("Game window"))
                                .child(div().flex().gap(px(14.)).child(div().flex_1().child(field("Width", Input::new(&i.width)))).child(div().flex_1().child(field("Height", Input::new(&i.height))))),
                        )
                        .child(
                            card()
                                .p(px(18.))
                                .v_flex()
                                .gap(px(14.))
                                .child(section_title("Versions"))
                                .child(Switch::new("set-snap").checked(snapshots).label("Show snapshots when creating instances").on_click(cx.listener(|this, v: &bool, _, cx| {
                                    this.settings.show_snapshots = *v;
                                    this.settings.save();
                                    cx.notify();
                                })))
                                .child(Switch::new("set-old").checked(old).label("Show old alpha and beta versions").on_click(cx.listener(|this, v: &bool, _, cx| {
                                    this.settings.show_old = *v;
                                    this.settings.save();
                                    cx.notify();
                                }))),
                        )
                        .child(div().flex().justify_end().child(Button::new("save-settings").primary().icon(I::Check).label("Save settings").on_click(cx.listener(|this, _, _, cx| this.save_settings(cx)))))
                        .child(
                            card()
                                .p(px(18.))
                                .v_flex()
                                .gap(px(12.))
                                .child(section_title("Storage"))
                                .child(div().flex().items_center().gap(px(10.)).child(div().flex_1().min_w_0().text_sm().font_family(MONO).truncate().child(root_s)).child(Button::new("open-root").icon(I::FolderOpen).label("Open").on_click(move |_, _, cx| cx.reveal_path(&root))))
                                .child(muted("Versions, libraries, assets and Java runtimes are shared by all instances, so each version is only downloaded once.")),
                        )
                        .child(
                            card()
                                .p(px(18.))
                                .v_flex()
                                .gap(px(8.))
                                .child(section_title("About"))
                                .child(div().flex().items_center().gap(px(10.)).child(logo(22.)).child(div().font_weight(FontWeight::SEMIBOLD).child(format!("Lodestone {}", env!("CARGO_PKG_VERSION")))))
                                .child(muted("A native Minecraft launcher written in Rust with GPUI. MIT licensed."))
                                .child(muted("Not an official Minecraft product. Not approved by or associated with Mojang or Microsoft.")),
                        ),
                ),
            )
    }
}
