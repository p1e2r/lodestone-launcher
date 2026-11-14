//! Pick a specific version of a mod or modpack.
//!
//! Mods only list versions that work with the target instance's Minecraft
//! version and loader. Modpacks list every version.

use gpui_kit::assets::IconName as I;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Disableable as _, Sizable as _, StyledExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::LodestoneApp;
use super::theme::*;
use crate::core::instance::Kind;
use crate::core::modrinth::{self, MVersion};
use crate::core::session::{self, Reporter, RunState};

#[derive(Clone, Debug, PartialEq)]
pub enum PickMode {
    /// Install a mod / resource pack / shader into an instance.
    Content { inst_id: String },
    /// Install a modpack as a new instance.
    NewPack { kind: Kind },
    /// Switch an existing modpack instance to another pack version.
    ChangePack { inst_id: String },
}

pub struct VersionPicker {
    pub project_id: String,
    pub title: String,
    pub mode: PickMode,
    pub list: Option<Result<Vec<MVersion>, String>>,
    /// Version number already installed, if any.
    pub current: Option<String>,
    pub busy: bool,
}

impl LodestoneApp {
    pub fn open_version_picker(&mut self, project_id: String, title: String, mode: PickMode, cx: &mut Context<Self>) {
        let current = match &mode {
            PickMode::Content { inst_id } => self
                .instance(inst_id)
                .and_then(|i| modrinth::InstalledIndex::load(i).projects.get(&project_id).map(|p| p.version.clone())),
            PickMode::ChangePack { inst_id } => self.instance(inst_id).and_then(|i| i.modpack_version_name.clone()),
            PickMode::NewPack { .. } => None,
        };
        let target = match &mode {
            PickMode::Content { inst_id } => self.instance(inst_id).cloned(),
            _ => None,
        };
        self.picker = Some(VersionPicker { project_id: project_id.clone(), title, mode, list: None, current, busy: false });
        let pid = project_id.clone();
        self.bg(
            cx,
            async move {
                match target {
                    Some(inst) => modrinth::compatible_versions(&inst, &pid).await,
                    None => modrinth::modpack_versions(&pid).await,
                }
                .map_err(|e| format!("{e:#}"))
            },
            move |this, r, _| {
                if let Some(p) = &mut this.picker {
                    if p.project_id == project_id {
                        p.list = Some(r);
                    }
                }
            },
        );
        cx.notify();
    }

    fn pick_version(&mut self, v: MVersion, cx: &mut Context<Self>) {
        let Some(p) = &mut self.picker else { return };
        p.busy = true;
        let mode = p.mode.clone();
        let title = p.title.clone();
        let pid = p.project_id.clone();
        match mode {
            PickMode::Content { inst_id } => {
                self.picker = None;
                self.install_content(&inst_id, pid, title, Some(v.id), cx);
            }
            PickMode::NewPack { kind } => {
                self.picker = None;
                self.install_pack(pid, title, kind, Some(v.id), cx);
            }
            PickMode::ChangePack { inst_id } => {
                let Some(inst) = self.instance(&inst_id).cloned() else { return };
                let inst_id = inst_id.clone();
                if session::session(&inst.id).lock().unwrap().busy() {
                    if let Some(p) = &mut self.picker {
                        p.busy = false;
                    }
                    self.notify_err("Stop it before changing the modpack version");
                    cx.notify();
                    return;
                }
                self.picker = None;
                let rep = Reporter::new(&inst.id);
                rep.clear_log();
                rep.set_state(RunState::Working);
                self.notify_ok(format!("Switching to {title} {}…", v.version_number));
                let vid = v.id.clone();
                self.bg(
                    cx,
                    async move {
                        let r = modrinth::change_modpack_version(&inst, &vid, &rep).await;
                        rep.set_state(RunState::Idle);
                        r.map_err(|e| format!("{e:#}"))
                    },
                    move |this, r, _| {
                        this.reload_instances();
                        if this.open.as_deref() == Some(inst_id.as_str()) {
                            this.inst_inputs = None;
                        }
                        match r {
                            Ok((inst, change)) => {
                                let mut msg = format!("Now on {} · Minecraft {} {}", inst.modpack_version_name.clone().unwrap_or_default(), inst.mc_version, inst.loader.name());
                                if !change.updated.is_empty() {
                                    msg.push_str(&format!(" · updated {} of your mods", change.updated.len()));
                                }
                                if !change.disabled.is_empty() {
                                    msg.push_str(&format!(" · switched off {} (no compatible version): {}", change.disabled.len(), change.disabled.join(", ")));
                                }
                                this.notify_ok(msg);
                            }
                            Err(e) => this.notify_err(e),
                        }
                    },
                );
            }
        }
        cx.notify();
    }

    pub fn version_picker_view(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.picker.as_ref().unwrap();
        let (heading, note): (String, String) = match &p.mode {
            PickMode::Content { inst_id } => {
                let i = self.instance(inst_id);
                (
                    "CHOOSE A VERSION".into(),
                    match i {
                        Some(i) => format!("Only versions that work with {} (Minecraft {} {}) are shown.", i.name, i.mc_version, i.loader.name()),
                        None => String::new(),
                    },
                )
            }
            PickMode::NewPack { .. } => ("MODPACK VERSION".into(), "Each version creates a new instance with the Minecraft version and loader it needs.".into()),
            PickMode::ChangePack { .. } => (
                "CHANGE VERSION".into(),
                "Your worlds and settings are kept. If the Minecraft version changes, mods you added yourself move to a compatible version, or are switched off if there isn't one.".into(),
            ),
        };
        let action = match &p.mode {
            PickMode::Content { .. } => "Install",
            PickMode::NewPack { kind: Kind::Server } => "Install as server",
            PickMode::NewPack { .. } => "Install",
            PickMode::ChangePack { .. } => "Switch",
        };
        let current = p.current.clone();
        let busy = p.busy;

        let body: AnyElement = match &p.list {
            None => div().p(px(20.)).child(muted("Loading versions…")).into_any_element(),
            Some(Err(e)) => div().p(px(20.)).text_sm().text_color(rgb(DANGER)).child(format!("Couldn't load versions: {e}")).into_any_element(),
            Some(Ok(list)) if list.is_empty() => div()
                .p(px(20.))
                .child(muted("No version of this works with the instance's Minecraft version and loader."))
                .into_any_element(),
            Some(Ok(list)) => div()
                .v_flex()
                .children(list.iter().enumerate().map(|(i, v)| {
                    let is_current = current.as_deref() == Some(v.version_number.as_str());
                    let v2 = v.clone();
                    let games = summarize_games(&v.game_versions);
                    let loaders = v.loaders.iter().filter(|l| *l != "minecraft").cloned().collect::<Vec<_>>().join(", ");
                    let date = v.date_published.get(..10).unwrap_or("").to_string();
                    div()
                        .id(("pv", i))
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .px(px(14.))
                        .py(px(10.))
                        .when(i > 0, |d| d.border_t_1().border_color(rgb(BORDER)))
                        .when(is_current, |d| d.bg(rgb(0x14241b)))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .v_flex()
                                .gap(px(3.))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(8.))
                                        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).truncate().child(v.version_number.clone()))
                                        .when(v.version_type == "beta", |d| d.child(badge("BETA", WARN)))
                                        .when(v.version_type == "alpha", |d| d.child(badge("ALPHA", DANGER)))
                                        .when(is_current, |d| d.child(badge("INSTALLED", ACCENT))),
                                )
                                .child(div().text_xs().text_color(rgb(FAINT)).truncate().child(
                                    [games, loaders, date].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · "),
                                )),
                        )
                        .child(
                            Button::new(("pick", i))
                                .small()
                                .map(|b| if is_current { b } else { b.primary() })
                                .label(if is_current { "Reinstall" } else { action })
                                .disabled(busy)
                                .on_click(cx.listener(move |this, _, _, cx| this.pick_version(v2.clone(), cx))),
                        )
                }))
                .into_any_element(),
        };

        div()
            .id("picker-backdrop")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .bg(Hsla::from(rgb(0x000000)).opacity(0.6))
            .flex()
            .items_center()
            .justify_center()
            .on_click(cx.listener(|this, _, _, cx| {
                this.picker = None;
                cx.notify();
            }))
            .child(
                div()
                    .id("picker")
                    .w(px(600.))
                    .h(relative(0.8))
                    .bg(rgb(PANEL))
                    .border_2()
                    .border_color(rgb(BORDER))
                    .shadow(pixel_shadow(8.))
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .p(px(22.))
                            .pb(px(14.))
                            .v_flex()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(div().text_lg().font_family(PIXEL).child(heading))
                                    .child(Button::new("picker-close").small().icon(I::X).on_click(cx.listener(|this, _, _, cx| {
                                        this.picker = None;
                                        cx.notify();
                                    }))),
                            )
                            .child(div().text_base().font_weight(FontWeight::SEMIBOLD).child(p.title.clone()))
                            .when(!note.is_empty(), |d| d.child(muted(note))),
                    )
                    .child(
                        div()
                            .id("picker-scroll")
                            .flex_1()
                            .mx(px(22.))
                            .mb(px(22.))
                            .border_1()
                            .border_color(rgb(BORDER))
                            .bg(rgb(PANEL_2))
                            .overflow_y_scrollbar()
                            .child(body),
                    ),
            )
    }
}

/// "Minecraft 26.2", "Minecraft 1.20.1, 1.20.2", or "Minecraft 1.19 – 1.20.4 (9 versions)".
fn summarize_games(g: &[String]) -> String {
    match g {
        [] => String::new(),
        _ if g.len() <= 3 => format!("Minecraft {}", g.join(", ")),
        [first, .., last] => format!("Minecraft {first} – {last} ({} versions)", g.len()),
        _ => String::new(),
    }
}
