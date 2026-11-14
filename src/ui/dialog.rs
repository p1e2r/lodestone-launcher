//! "New instance / New server" dialog.

use gpui_kit::assets::IconName as I;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Disableable as _, StyledExt as _};
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;

use super::theme::*;
use super::LodestoneApp;
use crate::core::instance::{Instance, Kind};
use crate::core::loaders::{self, Loader, LoaderVersion};

pub struct NewInstance {
    pub kind: Kind,
    pub name: Entity<InputState>,
    pub filter: Entity<InputState>,
    pub mc: Option<String>,
    pub loader: Loader,
    pub loader_versions: Option<Result<Vec<LoaderVersion>, String>>,
    pub loader_version: Option<String>,
    pub loader_key: Option<(Loader, String)>,
    pub snapshots: bool,
    pub old: bool,
    pub eula: bool,
    pub name_touched: bool,
    _subs: Vec<Subscription>,
}

impl LodestoneApp {
    pub fn open_new_dialog(&mut self, kind: Kind, window: &mut Window, cx: &mut Context<Self>) {
        let name = cx.new(|cx| InputState::new(window, cx).placeholder(if kind == Kind::Server { "My server" } else { "My instance" }));
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Search versions…"));
        let mut subs = Vec::new();
        subs.push(cx.subscribe_in(&filter, window, |_, _, _ev: &InputEvent, _, cx| cx.notify()));
        subs.push(cx.subscribe_in(&name, window, |this, _, ev: &InputEvent, _, cx| {
            if matches!(ev, InputEvent::Change) {
                if let Some(d) = &mut this.dialog {
                    d.name_touched = true;
                }
            }
            cx.notify();
        }));
        let latest = self.manifest.as_ref().map(|m| m.latest.release.clone());
        self.dialog = Some(NewInstance {
            kind,
            name,
            filter,
            mc: latest,
            loader: Loader::Vanilla,
            loader_versions: None,
            loader_version: None,
            loader_key: None,
            snapshots: self.settings.show_snapshots,
            old: self.settings.show_old,
            eula: false,
            name_touched: false,
            _subs: subs,
        });
        cx.notify();
    }

    fn refresh_loader_versions(&mut self, cx: &mut Context<Self>) {
        let Some(d) = &mut self.dialog else { return };
        let Some(mc) = d.mc.clone() else { return };
        if d.loader == Loader::Vanilla {
            d.loader_versions = None;
            d.loader_version = None;
            d.loader_key = None;
            return;
        }
        let key = (d.loader, mc.clone());
        if d.loader_key.as_ref() == Some(&key) {
            return;
        }
        d.loader_key = Some(key.clone());
        d.loader_versions = None;
        d.loader_version = None;
        let loader = d.loader;
        self.bg(cx, async move { loaders::list_versions(loader, &mc).await.map_err(|e| format!("{e:#}")) }, move |this, r, _| {
            let Some(d) = &mut this.dialog else { return };
            if d.loader_key.as_ref() != Some(&key) {
                return; // stale
            }
            if let Ok(list) = &r {
                d.loader_version = list.first().map(|v| v.version.clone());
            }
            d.loader_versions = Some(r);
        });
    }

    fn create_from_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(d) = &self.dialog else { return };
        let Some(mc) = d.mc.clone() else { return };
        let mut name = d.name.read(cx).value().trim().to_string();
        if name.is_empty() {
            name = match d.loader {
                Loader::Vanilla => format!("Minecraft {mc}"),
                l => format!("{} {mc}", l.name()),
            };
            if d.kind == Kind::Server {
                name.push_str(" Server");
            }
        }
        if d.loader != Loader::Vanilla && d.loader_version.is_none() {
            self.notify_err(format!("Pick a {} version", d.loader.name()));
            cx.notify();
            return;
        }
        if d.kind == Kind::Server && !d.eula {
            self.notify_err("You need to accept the Minecraft EULA to run a server");
            cx.notify();
            return;
        }
        let mut inst = Instance::new(&name, d.kind, &mc, d.loader, d.loader_version.clone());
        inst.server.eula = d.eula;
        if let Err(e) = inst.save() {
            self.notify_err(format!("{e:#}"));
            cx.notify();
            return;
        }
        let id = inst.id.clone();
        self.dialog = None;
        self.reload_instances();
        self.notify_ok(format!("Created “{name}”"));
        self.open_instance(&id, window, cx);
    }

    pub fn dialog_view(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.refresh_loader_versions(cx);
        let d = self.dialog.as_ref().unwrap();
        let filter = d.filter.read(cx).value().to_lowercase();
        let (snapshots, old) = (d.snapshots, d.old);
        let selected_mc = d.mc.clone();
        let loader = d.loader;
        let kind = d.kind;

        let versions: Vec<(String, String, String)> = match &self.manifest {
            Some(m) => m
                .versions
                .iter()
                .filter(|v| match v.kind.as_str() {
                    "release" => true,
                    "snapshot" => snapshots,
                    _ => old,
                })
                .filter(|v| filter.is_empty() || v.id.to_lowercase().contains(&filter))
                .take(300)
                .map(|v| (v.id.clone(), v.kind.clone(), v.release_time.get(..10).unwrap_or("").to_string()))
                .collect(),
            None => vec![],
        };

        let version_list = div()
            .id("dlg-versions")
            .h(px(230.))
            
            .border_1()
            .border_color(rgb(BORDER))
            .bg(rgb(PANEL_2))
            .when(self.manifest.is_none(), |d| {
                d.child(div().p(px(14.)).child(muted(self.manifest_error.clone().map(|e| format!("Couldn't load versions: {e}")).unwrap_or_else(|| "Loading versions…".into()))))
            })
            .children(versions.into_iter().enumerate().map(|(i, (id, kind_s, date))| {
                let active = selected_mc.as_deref() == Some(id.as_str());
                let id2 = id.clone();
                div()
                    .id(("ver", i))
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(12.))
                    .py(px(7.))
                    .cursor_pointer()
                    .text_sm()
                    .when(active, |d| d.bg(rgb(0x1d3b2b)).text_color(rgb(TEXT)))
                    .when(!active, |d| d.hover(|s| s.bg(rgb(PANEL_3))))
                    .child(div().flex().items_center().gap(px(8.)).child(div().font_weight(if active { FontWeight::SEMIBOLD } else { FontWeight::NORMAL }).child(id)).when(kind_s != "release", |d| {
                        d.child(badge(
                            match kind_s.as_str() {
                                "snapshot" => "Snapshot",
                                "old_beta" => "Beta",
                                _ => "Alpha",
                            },
                            if kind_s == "snapshot" { WARN } else { 0xb48ead },
                        ))
                    }))
                    .child(div().text_xs().text_color(rgb(FAINT)).child(date))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(d) = &mut this.dialog {
                            d.mc = Some(id2.clone());
                        }
                        cx.notify();
                    }))
            }))
            .overflow_y_scrollbar();

        let entity = cx.entity();
        let loader_picker = segmented(
            "dlg-loader",
            Loader::ALL.iter().map(|l| (*l, SharedString::from(l.name()))).collect(),
            &loader,
            move |l, _, cx| {
                entity.update(cx, |this, cx| {
                    if let Some(d) = &mut this.dialog {
                        d.loader = l;
                    }
                    cx.notify();
                })
            },
        );

        let loader_versions: AnyElement = if loader == Loader::Vanilla {
            muted("No mod loader — plain Minecraft.").into_any_element()
        } else {
            match &d.loader_versions {
                None => muted(format!("Loading {} versions…", loader.name())).into_any_element(),
                Some(Err(e)) => div().text_sm().text_color(rgb(DANGER)).child(format!("Couldn't load {} versions: {e}", loader.name())).into_any_element(),
                Some(Ok(list)) if list.is_empty() => div()
                    .text_sm()
                    .text_color(rgb(WARN))
                    .child(format!("{} isn't available for Minecraft {} yet.", loader.name(), selected_mc.clone().unwrap_or_default()))
                    .into_any_element(),
                Some(Ok(list)) => {
                    let sel = d.loader_version.clone();
                    div()
                        .id("dlg-lv")
                        .h(px(120.))
                        
                        .border_1()
                        .border_color(rgb(BORDER))
                        .bg(rgb(PANEL_2))
                        .children(list.iter().take(200).enumerate().map(|(i, v)| {
                            let active = sel.as_deref() == Some(v.version.as_str());
                            let ver = v.version.clone();
                            div()
                                .id(("lv", i))
                                .flex()
                                .items_center()
                                .justify_between()
                                .px(px(12.))
                                .py(px(6.))
                                .text_sm()
                                .cursor_pointer()
                                .when(active, |d| d.bg(rgb(0x1d3b2b)))
                                .when(!active, |d| d.hover(|s| s.bg(rgb(PANEL_3))))
                                .child(crate::core::instance::short_loader_version(&v.version))
                                .when(v.stable && i == 0, |d| d.child(badge("Recommended", ACCENT)))
                                .when(!v.stable, |d| d.child(badge("Beta", WARN)))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(d) = &mut this.dialog {
                                        d.loader_version = Some(ver.clone());
                                    }
                                    cx.notify();
                                }))
                        }))
                        .overflow_y_scrollbar()
                        .into_any_element()
                }
            }
        };

        let can_create = selected_mc.is_some()
            && (loader == Loader::Vanilla || d.loader_version.is_some())
            && (kind == Kind::Client || d.eula);
        let eula = d.eula;

        div()
            .id("dialog-backdrop")
            // Keep clicks and scrolling from reaching the page behind the dialog.
            .occlude()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .bg(Hsla::from(rgb(0x000000)).opacity(0.6))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .id("dialog")
                    .w(px(560.))
                    .max_h(relative(0.92))
                    .bg(rgb(PANEL))
                    .border_2()
                    .border_color(rgb(BORDER))
                    .shadow(pixel_shadow(8.))
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .p(px(24.))
                            .v_flex()
                            .gap(px(16.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(div().text_xl().font_family(PIXEL).child(if kind == Kind::Server { "NEW SERVER" } else { "NEW INSTANCE" }))
                                    .child(div().id("dlg-close").cursor_pointer().text_color(rgb(MUTED)).hover(|s| s.text_color(rgb(TEXT))).child(icon(I::X)).on_click(cx.listener(|this, _, _, cx| {
                                        this.dialog = None;
                                        cx.notify();
                                    }))),
                            )
                            .child(field("Name", Input::new(&d.name)))
                            .child(
                                div()
                                    .v_flex()
                                    .gap(px(8.))
                                    .child(div().flex().items_center().justify_between().child(div().text_sm().font_weight(FontWeight::MEDIUM).child("Minecraft version")).child(
                                        div()
                                            .flex()
                                            .gap(px(14.))
                                            .child(Switch::new("dlg-snap").checked(snapshots).label("Snapshots").on_click(cx.listener(|this, v: &bool, _, cx| {
                                                if let Some(d) = &mut this.dialog {
                                                    d.snapshots = *v;
                                                }
                                                cx.notify();
                                            })))
                                            .child(Switch::new("dlg-old").checked(old).label("Old alpha/beta").on_click(cx.listener(|this, v: &bool, _, cx| {
                                                if let Some(d) = &mut this.dialog {
                                                    d.old = *v;
                                                }
                                                cx.notify();
                                            }))),
                                    ))
                                    .child(Input::new(&d.filter).prefix(icon(I::Search).text_color(rgb(FAINT))))
                                    .child(version_list),
                            )
                            .child(div().v_flex().gap(px(8.)).child(div().text_sm().font_weight(FontWeight::MEDIUM).child("Mod loader")).child(loader_picker).child(loader_versions))
                            .when(kind == Kind::Server, |d| {
                                d.child(
                                    card()
                                        .p(px(12.))
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .child(div().v_flex().child(div().text_sm().font_weight(FontWeight::MEDIUM).child("I agree to the Minecraft EULA")).child(div().id("dlg-eula").text_xs().text_color(rgb(INFO)).cursor_pointer().child("Read it at aka.ms/MinecraftEULA").on_click(|_, _, cx| cx.open_url("https://aka.ms/MinecraftEULA"))))
                                        .child(Switch::new("dlg-eula-sw").checked(eula).on_click(cx.listener(|this, v: &bool, _, cx| {
                                            if let Some(d) = &mut this.dialog {
                                                d.eula = *v;
                                            }
                                            cx.notify();
                                        }))),
                                )
                            })
                            .child(
                                div()
                                    .flex()
                                    .justify_end()
                                    .gap(px(8.))
                                    .child(Button::new("dlg-cancel").label("Cancel").on_click(cx.listener(|this, _, _, cx| {
                                        this.dialog = None;
                                        cx.notify();
                                    })))
                                    .child(Button::new("dlg-create").primary().icon(I::Plus).label("Create").disabled(!can_create).on_click(cx.listener(|this, _, window, cx| this.create_from_dialog(window, cx)))),
                            ),
                    )
                    .overflow_y_scrollbar(),
            )
    }
}
