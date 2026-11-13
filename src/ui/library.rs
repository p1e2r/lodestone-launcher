use gpui_kit::assets::IconName as I;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Disableable as _, Sizable as _, StyledExt as _};
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;

use super::theme::*;
use super::{LodestoneApp, page_header};
use crate::core::instance::{Instance, Kind};
use crate::core::session::{self, RunState};

pub fn status_of(inst: &Instance) -> (RunState, String, f32, u64) {
    let s = session::session(&inst.id);
    let s = s.lock().unwrap();
    (s.state.clone(), s.stage.clone(), s.fraction(), s.total)
}

fn relative_time(ts: i64) -> String {
    let d = chrono::Utc::now().timestamp() - ts;
    match d {
        d if d < 60 => "just now".into(),
        d if d < 3600 => format!("{} min ago", d / 60),
        d if d < 86400 => format!("{} h ago", d / 3600),
        d if d < 86400 * 30 => format!("{} days ago", d / 86400),
        _ => chrono::DateTime::from_timestamp(ts, 0).map(|t| t.format("%b %d, %Y").to_string()).unwrap_or_default(),
    }
}

impl LodestoneApp {
    pub fn library_view(&mut self, kind: Kind, cx: &mut Context<Self>) -> impl IntoElement {
        let list: Vec<Instance> = self.instances.iter().filter(|i| i.kind == kind).cloned().collect();
        let (title, subtitle, new_label) = match kind {
            Kind::Client => ("Library", "Your Minecraft instances. Each one has its own version, mod loader and folder.", "New instance"),
            Kind::Server => ("Servers", "Host Minecraft servers on this computer — vanilla or any mod loader.", "New server"),
        };
        let import_busy = session::session("import").lock().unwrap().busy();
        let import_stage = session::session("import").lock().unwrap().stage.clone();

        let actions = vec![
            Button::new("import-mrpack")
                .label("Import .mrpack")
                .icon(I::FileUp)
                .on_click(cx.listener(move |this, _, _, cx| this.import_mrpack(kind, cx)))
                .into_any_element(),
            Button::new("new-instance")
                .primary()
                .label(new_label)
                .icon(I::Plus)
                .on_click(cx.listener(move |this, _, window, cx| this.open_new_dialog(kind, window, cx)))
                .into_any_element(),
        ];

        let hero = list.iter().find(|i| i.last_played.is_some()).cloned();
        let hero_el = hero.as_ref().map(|h| self.hero(h, cx).into_any_element());
        let mut cards = Vec::new();
        for inst in &list {
            cards.push(self.instance_card(inst, cx).into_any_element());
        }
        let body = if list.is_empty() {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .v_flex()
                        .items_center()
                        .gap(px(14.))
                        .max_w(px(420.))
                        .child(div().size(px(64.)).bg(rgb(PANEL_2)).border_1().border_color(rgb(BORDER)).flex().items_center().justify_center().child(icon(if kind == Kind::Server { I::Server } else { I::Gamepad2 }).size(px(28.)).text_color(rgb(ACCENT))))
                        .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child(if kind == Kind::Server { "No servers yet" } else { "No instances yet" }))
                        .child(div().text_sm().text_color(rgb(MUTED)).text_center().child(if kind == Kind::Server {
                            "Create a server for any version and mod loader, or import a modpack as a server."
                        } else {
                            "Create an instance for any Minecraft version — vanilla, Fabric, Quilt, Forge or NeoForge — or grab a modpack from Discover."
                        }))
                        .child(
                            Button::new("empty-new")
                                .primary()
                                .label(new_label)
                                .icon(I::Plus)
                                .on_click(cx.listener(move |this, _, window, cx| this.open_new_dialog(kind, window, cx))),
                        ),
                )
                .into_any_element()
        } else {
            div()
                .id("grid-scroll")
                .flex_1()
                .overflow_y_scrollbar()
                .child(
                    div()
                        .px(px(32.))
                        .pb(px(32.))
                        .v_flex()
                        .gap(px(18.))
                        .children(hero_el)
                        .child(section_title(if kind == Kind::Server { "All servers" } else { "All instances" }))
                        .child(div().flex().flex_wrap().gap(px(16.)).children(cards)),
                )
                .into_any_element()
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(page_header(title, subtitle, actions))
            .when(import_busy, |d| {
                d.child(
                    div().px(px(32.)).pb(px(14.)).child(
                        card().p(px(12.)).flex().items_center().gap(px(12.)).text_sm().child(icon(I::Download).text_color(rgb(ACCENT))).child(div().flex_1().child(import_stage)),
                    ),
                )
            })
            .child(body)
    }

    fn hero(&self, inst: &Instance, cx: &mut Context<Self>) -> impl IntoElement {
        let (state, stage, frac, total) = status_of(inst);
        let id = inst.id.clone();
        let id2 = inst.id.clone();
        let running = state == RunState::Running;
        let working = state == RunState::Working;
        let is_server = inst.kind == Kind::Server;
        let label = match (is_server, running, working) {
            (_, _, true) => "Preparing…",
            (_, true, _) => "Stop",
            (true, _, _) => "Start server",
            _ => "Play",
        };
        let kicker = if is_server { "JUMP BACK IN" } else { "CONTINUE PLAYING" };
        div()
            .id("hero")
            .h(px(240.))
            
            .overflow_hidden()
            .relative()
            .border_2()
            .border_color(rgb(BORDER))
            .shadow(pixel_shadow(6.))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, window, cx| this.open_instance(&id2, window, cx)))
            .child(div().absolute().top_0().left_0().size_full().child(img(super::art::banner(&inst.id)).size_full().object_fit(ObjectFit::Cover)))
            .child(div().absolute().top_0().left_0().size_full().bg(linear_gradient(90., linear_color_stop(Hsla::from(rgb(BG)).opacity(0.94), 0.0), linear_color_stop(Hsla::from(rgb(BG)).opacity(0.0), 0.72))))
            .child(div().absolute().top_0().left_0().size_full().bg(linear_gradient(180., linear_color_stop(Hsla::from(rgb(BG)).opacity(0.0), 0.55), linear_color_stop(Hsla::from(rgb(BG)).opacity(0.55), 1.))))
            .child(
                div()
                    .relative()
                    .size_full()
                    .p(px(28.))
                    .v_flex()
                    .justify_end()
                    .gap(px(10.))
                    .child(div().text_xs().font_family(PIXEL).text_color(rgb(ACCENT)).child(kicker))
                    .child(div().flex().items_center().gap(px(14.)).child(instance_tile(&inst.name, inst.loader, 52.)).child(
                        div()
                            .v_flex()
                            .gap(px(2.))
                            .child(div().text_3xl().font_family(PIXEL).text_color(rgb(0xffffff)).child(inst.name.to_uppercase()))
                            .child(div().text_sm().text_color(rgb(0xd6dbe4)).child(format!("Minecraft {} · {}", inst.mc_version, inst.loader_label()))),
                    ))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(14.))
                            .mt(px(6.))
                            .child(
                                Button::new("hero-play")
                                    .large()
                                    .icon(if running { I::Square } else { I::Play })
                                    .label(label)
                                    .map(|b| if running { b.danger() } else { b.primary() })
                                    .disabled(working)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        if running {
                                            this.stop(&id, cx)
                                        } else {
                                            this.play(&id, cx)
                                        }
                                    })),
                            )
                            .when(working, |d| d.child(div().w(px(240.)).v_flex().gap(px(6.)).child(div().text_xs().text_color(rgb(0xd6dbe4)).truncate().child(stage.clone())).child(progress_bar(frac, total == 0))))
                            .when(running, |d| d.child(live_badge())),
                    ),
            )
    }

    fn instance_card(&self, inst: &Instance, cx: &mut Context<Self>) -> impl IntoElement {
        let (state, stage, frac, total) = status_of(inst);
        let id = inst.id.clone();
        let id2 = inst.id.clone();
        let col: Hsla = rgb(loader_color(inst.loader)).into();
        let subtitle = format!("{} · {}", inst.mc_version, inst.loader_label());
        let footer_text = match &state {
            RunState::Running => if inst.kind == Kind::Server { format!("Online · port {}", inst.server.port) } else { "Playing".into() },
            RunState::Working => if stage.is_empty() { "Preparing…".into() } else { stage.clone() },
            RunState::Failed(_) => "Last launch failed".into(),
            RunState::Idle => match inst.last_played {
                Some(t) => format!("Played {}", relative_time(t)),
                None => "Never played".into(),
            },
        };
        let busy = matches!(state, RunState::Working | RunState::Running);
        let play_label = match (inst.kind, &state) {
            (_, RunState::Running) => "Stop",
            (_, RunState::Working) => "…",
            (Kind::Server, _) => "Start",
            (Kind::Client, _) => "Play",
        };

        div()
            .id(SharedString::from(format!("card-{}", inst.id)))
            .w(px(236.))
            
            .bg(rgb(PANEL))
            .border_2()
            .border_color(rgb(BORDER))
            .overflow_hidden()
            .cursor_pointer()
            .shadow(pixel_shadow(5.))
            .hover(|s| s.border_color(col.opacity(0.8)).bg(rgb(PANEL_2)))
            .on_click(cx.listener(move |this, _, window, cx| this.open_instance(&id, window, cx)))
            .child(
                div()
                    .h(px(118.))
                    .relative()
                    .overflow_hidden()
                    .flex()
                    .items_end()
                    .px(px(14.))
                    .pb(px(10.))
                    .child(div().absolute().top_0().left_0().size_full().child(img(super::art::banner(&inst.id)).size_full().object_fit(ObjectFit::Cover)))
                    .child(div().absolute().top_0().left_0().size_full().bg(linear_gradient(180., linear_color_stop(Hsla::from(rgb(PANEL)).opacity(0.0), 0.45), linear_color_stop(rgb(PANEL), 1.))))
                    .child(div().relative().child(instance_tile(&inst.name, inst.loader, 46.)))
                    .child(
                        div().absolute().top(px(10.)).right(px(10.)).flex().gap(px(6.))
                            .when(inst.modpack.is_some(), |d| d.child(badge("Modpack", INFO)))
                            .when(state == RunState::Running, |d| d.child(live_badge())),
                    ),
            )
            .child(
                div()
                    .px(px(14.))
                    .pt(px(10.))
                    .pb(px(14.))
                    .v_flex()
                    .gap(px(3.))
                    .child(div().text_sm().font_family(PIXEL).truncate().child(inst.name.to_uppercase()))
                    .child(div().text_xs().text_color(rgb(MUTED)).truncate().child(subtitle))
                    .child(div().h(px(10.)))
                    .when(state == RunState::Working, |d| d.child(progress_bar(frac, total == 0)).child(div().h(px(8.))))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(8.))
                            .child(div().flex_1().min_w_0().text_xs().text_color(if matches!(state, RunState::Failed(_)) { rgb(DANGER) } else { rgb(FAINT) }).truncate().child(footer_text))
                            .child(
                                Button::new(SharedString::from(format!("play-{}", inst.id)))
                                    .small()
                                    .map(|b| if state == RunState::Running { b.danger() } else { b.primary() })
                                    .disabled(state == RunState::Working)
                                    .label(play_label)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        if busy {
                                            this.stop(&id2, cx);
                                        } else {
                                            this.play(&id2, cx);
                                        }
                                    })),
                            ),
                    ),
            )
    }
}
