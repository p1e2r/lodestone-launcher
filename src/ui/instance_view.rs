//! Detail page for one instance or server: header, Play/Start, content,
//! console, options.

use std::rc::Rc;

use gpui_kit::assets::IconName as I;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Disableable as _, Sizable as _, StyledExt as _};
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;

use super::theme::*;
use super::{LodestoneApp, Page, Tab};
use crate::core::instance::{self, Instance, Kind};
use crate::core::loaders::Loader;
use crate::core::session::{self, Level, RunState};
use crate::core::modrinth;

pub struct InstanceInputs {
    pub name: Entity<InputState>,
    pub memory: Entity<InputState>,
    pub jvm_args: Entity<InputState>,
    pub port: Entity<InputState>,
    pub motd: Entity<InputState>,
    pub max_players: Entity<InputState>,
}

impl InstanceInputs {
    pub fn new(inst: &Instance, window: &mut Window, cx: &mut Context<LodestoneApp>) -> Self {
        let mk = |v: String, ph: &'static str, window: &mut Window, cx: &mut Context<LodestoneApp>| {
            cx.new(|cx| InputState::new(window, cx).placeholder(ph).default_value(v))
        };
        InstanceInputs {
            name: mk(inst.name.clone(), "Name", window, cx),
            memory: mk(inst.memory_mb.map(|m| m.to_string()).unwrap_or_default(), "Default (from Settings)", window, cx),
            jvm_args: mk(inst.jvm_args.clone(), "e.g. -XX:+UseZGC", window, cx),
            port: mk(inst.server.port.to_string(), "25565", window, cx),
            motd: mk(inst.server.motd.clone(), "A Lodestone server", window, cx),
            max_players: mk(inst.server.max_players.to_string(), "20", window, cx),
        }
    }
}

impl LodestoneApp {
    /// Copy dropped/picked files into an instance's mods/, resourcepacks/ or shaderpacks/.
    pub fn add_content_files(&mut self, inst_id: &str, sub: &str, paths: &[std::path::PathBuf], cx: &mut Context<Self>) {
        let Some(inst) = self.instance(inst_id).cloned() else { return };
        let (added, skipped) = match inst.import_content(sub, paths) {
            Ok(r) => r,
            Err(e) => {
                self.notify_err(format!("{e:#}"));
                cx.notify();
                return;
            }
        };
        let kind = match sub {
            "mods" => ".jar mods",
            "resourcepacks" => ".zip resource packs",
            _ => ".zip shader packs",
        };
        match (added.len(), skipped.len()) {
            (0, 0) => {}
            (0, _) => self.notify_err(format!("Nothing added — this tab only takes {kind}")),
            (1, 0) => self.notify_ok(format!("Added {}", added[0])),
            (n, 0) => self.notify_ok(format!("Added {n} files")),
            (n, k) => self.notify_ok(format!("Added {n} files, skipped {k} that aren't {kind}")),
        }
        cx.notify();
    }

    fn pick_content_files(&mut self, inst_id: String, sub: &'static str, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Add".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else { return };
            let _ = this.update(cx, |this, cx| this.add_content_files(&inst_id, sub, &paths, cx));
        })
        .detach();
    }

    fn update_instance(&mut self, id: &str, f: impl FnOnce(&mut Instance)) {
        if let Some(inst) = self.instances.iter_mut().find(|i| i.id == id) {
            f(inst);
            if let Err(e) = inst.save() {
                self.notify_err(format!("Couldn't save: {e:#}"));
            }
        }
    }

    pub fn instance_view(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(inst) = self.instance(id).cloned() else {
            return div().p(px(32.)).child("This instance no longer exists.").into_any_element();
        };
        let (state, stage, frac, total) = super::library::status_of(&inst);
        let is_server = inst.kind == Kind::Server;

        let tabs: Vec<(Tab, SharedString)> = if is_server {
            vec![(Tab::Console, "Console".into()), (Tab::Content, "Mods".into()), (Tab::Options, "Options".into())]
        } else {
            vec![(Tab::Content, "Content".into()), (Tab::Console, "Log".into()), (Tab::Options, "Options".into())]
        };
        let tab = self.tab;
        let entity = cx.entity();
        let tab_bar = segmented("inst-tabs", tabs, &tab, move |t, _, cx| {
            entity.update(cx, |this, cx| {
                this.tab = t;
                cx.notify();
            })
        });

        let body: AnyElement = match tab {
            Tab::Content => self.content_tab(&inst, cx).into_any_element(),
            Tab::Console => self.console_tab(&inst, cx).into_any_element(),
            Tab::Options => self.options_tab(&inst, window, cx).into_any_element(),
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .relative()
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .w_full()
                    .h(px(230.))
                    .overflow_hidden()
                    .child(img(super::art::banner(&inst.id)).size_full().object_fit(ObjectFit::Cover))
                    .child(div().absolute().top_0().left_0().size_full().bg(linear_gradient(180., linear_color_stop(Hsla::from(rgb(BG)).opacity(0.35), 0.), linear_color_stop(rgb(BG), 1.)))),
            )
            .child(div().relative().child(self.instance_header(&inst, &state, &stage, frac, total, cx)))
            .child(div().relative().px(px(32.)).pb(px(16.)).child(tab_bar))
            .child(div().relative().flex_1().min_h_0().px(px(32.)).pb(px(24.)).child(body))
            .into_any_element()
    }

    fn instance_header(&self, inst: &Instance, state: &RunState, stage: &str, frac: f32, total: u64, cx: &mut Context<Self>) -> impl IntoElement {
        let id = inst.id.clone();
        let is_server = inst.kind == Kind::Server;
        let back_page = if is_server { Page::Servers } else { Page::Library };
        let col: Hsla = rgb(loader_color(inst.loader)).into();
        let running = *state == RunState::Running;
        let working = *state == RunState::Working;
        let (btn_label, btn_icon) = match (is_server, running) {
            (_, true) => ("Stop", I::Square),
            (true, false) => ("Start server", I::Play),
            (false, false) => ("Play", I::Play),
        };
        let lan = if is_server && running { instance::lan_ip() } else { None };
        let port = inst.server.port;

        div()
            .px(px(32.))
            .pt(px(22.))
            .pb(px(18.))
            .v_flex()
            .gap(px(16.))
            .child(
                div()
                    .id("back")
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_sm()
                    .text_color(rgb(0xd6dbe4))
                    .cursor_pointer()
                    .hover(|s| s.text_color(rgb(0xffffff)))
                    .child(icon(I::ArrowLeft))
                    .child(if is_server { "Servers" } else { "Library" })
                    .on_click(cx.listener(move |this, _, _, cx| this.go(back_page, cx))),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(18.))
                    .child(instance_tile(&inst.name, inst.loader, 72.).shadow_lg())
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .v_flex()
                            .gap(px(6.))
                            .child(div().text_3xl().font_family(PIXEL).text_color(rgb(0xffffff)).truncate().child(inst.name.to_uppercase()))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(badge(format!("Minecraft {}", inst.mc_version), 0x9aa3b2))
                                    .child(div().px(px(7.)).py(px(1.)).text_xs().font_family(PIXEL).text_color(col).bg(col.opacity(0.14)).border_1().border_color(col.opacity(0.45)).child(inst.loader_label().to_uppercase()))
                                    .when(is_server, |d| d.child(badge(if inst.server.online_mode { "Online mode" } else { "Offline mode" }, 0x9aa3b2)))
                                    .when(running, |d| d.child(live_badge()))
                                    ,
                            ),
                    )
                    .child(
                        Button::new("main-action")
                            .large()
                            .icon(btn_icon)
                            .label(if working { "Preparing…" } else { btn_label })
                            .map(|b| if running { b.danger() } else { b.primary() })
                            .disabled(working)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if running {
                                    this.stop(&id, cx)
                                } else {
                                    this.play(&id, cx)
                                }
                            })),
                    ),
            )
            .when(working, |d| {
                d.child(
                    card()
                        .p(px(14.))
                        .v_flex()
                        .gap(px(10.))
                        .child(
                            div()
                                .flex()
                                .justify_between()
                                .text_sm()
                                .child(div().text_color(rgb(TEXT)).child(if stage.is_empty() { "Preparing…".to_string() } else { stage.to_string() }))
                                .when(total > 0, |d| d.child(div().text_color(rgb(MUTED)).child(format!("{:.0}%", frac * 100.0)))),
                        )
                        .child(progress_bar(frac, total == 0)),
                )
            })
            .when_some(match state { RunState::Failed(e) => Some(e.clone()), _ => None }, |d, e| {
                d.child(
                    div()
                        .p(px(12.))
                        
                        .border_1()
                        .border_color(Hsla::from(rgb(DANGER)).opacity(0.5))
                        .bg(Hsla::from(rgb(DANGER)).opacity(0.08))
                        .flex()
                        .gap(px(10.))
                        .text_sm()
                        .child(icon(I::CircleAlert).text_color(rgb(DANGER)))
                        .child(div().flex_1().child(e)),
                )
            })
            .when(is_server && running, |d| {
                let local = format!("localhost:{port}");
                let lan_addr = lan.map(|ip| format!("{ip}:{port}"));
                let local2 = local.clone();
                d.child(
                    card()
                        .p(px(12.))
                        .flex()
                        .items_center()
                        .gap(px(14.))
                        .text_sm()
                        .child(div().size(px(8.)).bg(rgb(ACCENT)))
                        .child(div().text_color(rgb(MUTED)).child("Join at"))
                        .child(div().font_family(MONO).child(local.clone()))
                        .child(small_button("copy-local", "Copy").on_click(cx.listener(move |this, _, _, cx| this.copy(local2.clone(), "Address", cx))))
                        .when_some(lan_addr, |d, a| {
                            let a2 = a.clone();
                            d.child(div().text_color(rgb(MUTED)).child("· LAN"))
                                .child(div().font_family(MONO).child(a))
                                .child(small_button("copy-lan", "Copy").on_click(cx.listener(move |this, _, _, cx| this.copy(a2.clone(), "LAN address", cx))))
                        }),
                )
            })
    }

    // ------------------------------------------------------------ content

    fn content_tab(&mut self, inst: &Instance, cx: &mut Context<Self>) -> impl IntoElement {
        let subs: Vec<(&'static str, SharedString)> = if inst.kind == Kind::Server {
            vec![("mods", "Mods".into())]
        } else {
            vec![("mods", "Mods".into()), ("resourcepacks", "Resource packs".into()), ("shaderpacks", "Shaders".into())]
        };
        if !subs.iter().any(|(s, _)| *s == self.content_sub) {
            self.content_sub = "mods";
        }
        let sub = self.content_sub;
        let files = inst.list_content(sub);
        let index = modrinth::InstalledIndex::load(inst);
        let titles: std::collections::HashMap<String, String> =
            index.projects.values().map(|p| (p.file.clone(), p.title.clone())).collect();
        let modded = inst.loader != Loader::Vanilla;
        let entity = cx.entity();
        let inst_id = inst.id.clone();
        let inst_id_add = inst.id.clone();
        let inst_id_drop = inst.id.clone();
        let folder = inst.content_dir(sub);

        let list: AnyElement = if sub == "mods" && !modded {
            div()
                .p(px(28.))
                .v_flex()
                .items_center()
                .gap(px(8.))
                .child(icon(I::Blocks).size(px(26.)).text_color(rgb(FAINT)))
                .child(div().font_weight(FontWeight::SEMIBOLD).child("Vanilla doesn't load mods"))
                .child(muted("Create a Fabric, Quilt, Forge or NeoForge instance to use mods. Resource packs and shaders still work."))
                .into_any_element()
        } else if files.is_empty() {
            div()
                .p(px(28.))
                .v_flex()
                .items_center()
                .gap(px(8.))
                .child(icon(I::Package).size(px(26.)).text_color(rgb(FAINT)))
                .child(div().font_weight(FontWeight::SEMIBOLD).child("Nothing installed yet"))
                .child(muted("Drag files here, use Add files, or find content on Modrinth from Discover."))
                .into_any_element()
        } else {
            div()
                .v_flex()
                .children(files.into_iter().enumerate().map(|(i, f)| {
                    let title = titles.get(&f.display).cloned();
                    let f2 = f.clone();
                    let f3 = f.clone();
                    div()
                        .id(("content-row", i))
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .px(px(14.))
                        .py(px(10.))
                        .when(i > 0, |d| d.border_t_1().border_color(rgb(BORDER)))
                        .child(
                            div()
                                .size(px(32.))
                                .flex_none()
                                
                                .bg(rgb(PANEL_3))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon(if sub == "mods" { I::Puzzle } else if sub == "shaderpacks" { I::Sparkles } else { I::Image }).text_color(rgb(MUTED))),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .v_flex()
                                .child(div().text_sm().font_weight(FontWeight::MEDIUM).truncate().text_color(if f.enabled { rgb(TEXT) } else { rgb(FAINT) }).child(title.clone().unwrap_or_else(|| f.display.clone())))
                                .child(div().text_xs().text_color(rgb(FAINT)).truncate().child(format!("{} · {}", f.display, fmt_bytes(f.size)))),
                        )
                        .child(Switch::new(("toggle", i)).checked(f.enabled).on_click({
                            let entity = entity.clone();
                            move |_, _, cx| {
                                let _ = f2.toggle();
                                entity.update(cx, |_, cx| cx.notify());
                            }
                        }))
                        .child(Button::new(("del", i)).small().icon(I::Trash).ghost_like().on_click({
                            let entity = entity.clone();
                            move |_, _, cx| {
                                let _ = std::fs::remove_file(&f3.path);
                                entity.update(cx, |_, cx| cx.notify());
                            }
                        }))
                }))
                .into_any_element()
        };

        let e2 = cx.entity();
        div()
            .size_full()
            .v_flex()
            .gap(px(14.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(segmented("content-sub", subs, &sub, move |s, _, cx| {
                        e2.update(cx, |this, cx| {
                            this.content_sub = s;
                            cx.notify();
                        })
                    }))
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .when(sub != "mods" || modded, |d| {
                                let id = inst_id_add.clone();
                                d.child(Button::new("add-files").icon(I::FilePlus).label(if sub == "mods" { "Add .jar" } else { "Add .zip" }).on_click(cx.listener(move |this, _, _, cx| {
                                    this.pick_content_files(id.clone(), sub, cx);
                                })))
                            })
                            .child(Button::new("open-folder").icon(I::FolderOpen).label("Open folder").on_click(move |_, _, cx| {
                                let _ = std::fs::create_dir_all(&folder);
                                cx.reveal_path(&folder);
                            }))
                            .child(Button::new("browse").primary().icon(I::Compass).label("Browse Modrinth").on_click(cx.listener(move |this, _, _, cx| {
                                this.discover.target = Some(inst_id.clone());
                                this.discover.ctype = match this.content_sub {
                                    "resourcepacks" => modrinth::ContentType::ResourcePack,
                                    "shaderpacks" => modrinth::ContentType::Shader,
                                    _ => modrinth::ContentType::Mod,
                                };
                                this.go(Page::Discover, cx);
                                this.discover_search(true, cx);
                            }))),
                    ),
            )
            .child(
                card()
                    .id("content-scroll")
                    .flex_1()
                    .when(sub != "mods" || modded, |d| {
                        d.drag_over::<ExternalPaths>(|style, _, _, _| style.border_color(rgb(ACCENT)).bg(rgb(0x14241b)))
                            .on_drop(cx.listener(move |this, paths: &ExternalPaths, _, cx| {
                                this.add_content_files(&inst_id_drop, sub, paths.paths(), cx);
                            }))
                    })
                    .child(list)
                    .when(sub != "mods" || modded, |d| {
                        d.child(
                            div()
                                .m(px(12.))
                                .py(px(14.))
                                
                                .border_2()
                                .border_dashed()
                                .border_color(rgb(BORDER))
                                .flex()
                                .items_center()
                                .justify_center()
                                .gap(px(8.))
                                .text_sm()
                                .text_color(rgb(MUTED))
                                .child(icon(I::Upload))
                                .child(if sub == "mods" { "Drop .jar files here to add mods" } else { "Drop .zip files here to add them" }),
                        )
                    })
                    .overflow_y_scrollbar(),
            )
    }

    // ------------------------------------------------------------ console

    fn console_tab(&mut self, inst: &Instance, cx: &mut Context<Self>) -> impl IntoElement {
        let s = session::session(&inst.id);
        let lines: Rc<Vec<session::LogLine>> = Rc::new(s.lock().unwrap().log.iter().cloned().collect());
        let running = s.lock().unwrap().state == RunState::Running;
        let count = lines.len();
        let is_server = inst.kind == Kind::Server;
        let all_text: String = lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("\n");
        let follow = self.console_follow;
        let logs_dir = inst.game_dir().join("logs");
        let id = inst.id.clone();

        let list: AnyElement = if count == 0 {
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child(muted(if is_server { "Start the server to see its console here." } else { "Game output will appear here when you play." }))
                .into_any_element()
        } else {
            let lines2 = lines.clone();
            uniform_list("console", count, move |range, _window, _cx| {
                range
                    .map(|i| {
                        let l = &lines2[i];
                        let color = match l.level {
                            Level::Error => rgb(0xff7b7f),
                            Level::Warn => rgb(WARN),
                            Level::Launcher => rgb(ACCENT),
                            Level::Info => rgb(0xc9ced8),
                        };
                        div().px(px(14.)).text_xs().font_family(MONO).text_color(color).whitespace_nowrap().child(l.text.clone())
                    })
                    .collect()
            })
            .track_scroll(&self.console_scroll)
            .size_full()
            .py(px(8.))
            .into_any_element()
        };

        div()
            .size_full()
            .v_flex()
            .gap(px(10.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_xs().text_color(rgb(FAINT)).child(format!("{count} lines")))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(Switch::new("follow").checked(follow).label("Follow output").on_click(cx.listener(|this, v: &bool, _, cx| {
                                this.console_follow = *v;
                                cx.notify();
                            })))
                            .child(small_button("copy-log", "Copy").icon(I::Copy).on_click(cx.listener(move |this, _, _, cx| this.copy(all_text.clone(), "Log", cx))))
                            .child(small_button("clear-log", "Clear").on_click(cx.listener(move |_, _, _, cx| {
                                crate::core::session::Reporter::new(&id).clear_log();
                                cx.notify();
                            })))
                            .child(small_button("logs-folder", "Logs folder").icon(I::FolderOpen).on_click(move |_, _, cx| {
                                let _ = std::fs::create_dir_all(&logs_dir);
                                cx.reveal_path(&logs_dir);
                            })),
                    ),
            )
            .child(div().flex_1().min_h_0().bg(rgb(0x0a0c0f)).border_2().border_color(rgb(BORDER)).shadow(pixel_shadow(4.)).overflow_hidden().child(list))
            .when(is_server, |d| {
                d.child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .child(div().flex_1().child(Input::new(&self.command_input).disabled(!running).prefix(div().text_color(rgb(ACCENT)).font_family(MONO).child(">"))))
                        .child(Button::new("send-cmd").primary().icon(I::Send).label("Send").disabled(!running).on_click(cx.listener(|this, _, window, cx| this.send_command(window, cx)))),
                )
            })
    }

    // ------------------------------------------------------------ options

    fn options_tab(&mut self, inst: &Instance, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(inputs) = &self.inst_inputs else { return div().into_any_element() };
        let is_server = inst.kind == Kind::Server;
        let id = inst.id.clone();
        let id_del = inst.id.clone();
        let id_reset = inst.id.clone();
        let dir = inst.dir();
        let confirming = self.confirm_delete.as_deref() == Some(inst.id.as_str());
        let online = inst.server.online_mode;
        let eula = inst.server.eula;
        let id_online = inst.id.clone();
        let id_eula = inst.id.clone();

        div()
            .id("options-scroll")
            .size_full()
            .overflow_y_scrollbar()
            .child(
                div()
                    .v_flex()
                    .gap(px(14.))
                    .pb(px(8.))
                    .child(
                        card()
                            .p(px(18.))
                            .v_flex()
                            .gap(px(14.))
                            .child(section_title("General"))
                            .child(field("Name", Input::new(&inputs.name)))
                            .child(div().flex().gap(px(14.)).child(div().flex_1().child(field("Memory (MB)", Input::new(&inputs.memory)))).child(div().flex_1().child(field("Extra JVM arguments", Input::new(&inputs.jvm_args)))))
                            .when(is_server, |d| {
                                d.child(section_title("Server"))
                                    .child(div().flex().gap(px(14.)).child(div().flex_1().child(field("Port", Input::new(&inputs.port)))).child(div().flex_1().child(field("Max players", Input::new(&inputs.max_players)))))
                                    .child(field("Message of the day", Input::new(&inputs.motd)))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .child(div().v_flex().child(div().text_sm().font_weight(FontWeight::MEDIUM).child("Online mode")).child(div().text_xs().text_color(rgb(MUTED)).child("Requires real Microsoft accounts. Leave off so offline (Lodestone) players can join.")))
                                            .child(Switch::new("online-mode").checked(online).on_click(cx.listener(move |this, v: &bool, _, cx| {
                                                let v = *v;
                                                this.update_instance(&id_online, |i| i.server.online_mode = v);
                                                cx.notify();
                                            }))),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .child(div().v_flex().child(div().text_sm().font_weight(FontWeight::MEDIUM).child("I agree to the Minecraft EULA")).child(div().id("eula-link").text_xs().text_color(rgb(INFO)).cursor_pointer().child("aka.ms/MinecraftEULA").on_click(|_, _, cx| cx.open_url("https://aka.ms/MinecraftEULA"))))
                                            .child(Switch::new("eula").checked(eula).on_click(cx.listener(move |this, v: &bool, _, cx| {
                                                let v = *v;
                                                this.update_instance(&id_eula, |i| i.server.eula = v);
                                                cx.notify();
                                            }))),
                                    )
                            })
                            .child(
                                div().flex().justify_end().child(Button::new("save-opts").primary().icon(I::Check).label("Save").on_click(cx.listener(move |this, _, _, cx| {
                                    this.save_instance_options(&id, cx);
                                }))),
                            ),
                    )
                    .child(
                        card()
                            .p(px(18.))
                            .v_flex()
                            .gap(px(12.))
                            .child(section_title("Manage"))
                            .child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .gap(px(8.))
                                    .child(Button::new("open-dir").icon(I::FolderOpen).label("Open instance folder").on_click(move |_, _, cx| cx.reveal_path(&dir)))
                                    .child(Button::new("reinstall").icon(I::RefreshCw).label(if is_server { "Reinstall server" } else { "Reinstall loader" }).on_click(cx.listener(move |this, _, _, cx| {
                                        this.update_instance(&id_reset, |i| {
                                            i.version_id = None;
                                            i.server.launch = None;
                                        });
                                        this.notify_ok("It will be reinstalled the next time you launch it");
                                        cx.notify();
                                    })))
                                    .child(if confirming {
                                        Button::new("delete").danger().icon(I::Trash).label("Click again to delete forever").on_click(cx.listener(move |this, _, _, cx| {
                                            if let Some(inst) = this.instance(&id_del).cloned() {
                                                if session::session(&inst.id).lock().unwrap().busy() {
                                                    this.notify_err("Stop it before deleting");
                                                } else if let Err(e) = inst.delete() {
                                                    this.notify_err(format!("{e:#}"));
                                                } else {
                                                    this.notify_ok(format!("Deleted “{}”", inst.name));
                                                    this.reload_instances();
                                                    let page = if inst.kind == Kind::Server { Page::Servers } else { Page::Library };
                                                    this.go(page, cx);
                                                }
                                            }
                                            cx.notify();
                                        }))
                                    } else {
                                        let id_c = inst.id.clone();
                                        Button::new("delete").icon(I::Trash).label("Delete").on_click(cx.listener(move |this, _, _, cx| {
                                            this.confirm_delete = Some(id_c.clone());
                                            cx.notify();
                                        }))
                                    }),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn save_instance_options(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(inputs) = &self.inst_inputs else { return };
        let name = inputs.name.read(cx).value().trim().to_string();
        let memory = inputs.memory.read(cx).value().trim().to_string();
        let jvm = inputs.jvm_args.read(cx).value().to_string();
        let port = inputs.port.read(cx).value().trim().to_string();
        let motd = inputs.motd.read(cx).value().to_string();
        let maxp = inputs.max_players.read(cx).value().trim().to_string();
        let memory_mb = if memory.is_empty() { None } else {
            match memory.parse::<u32>() {
                Ok(m) if m >= 512 => Some(m),
                _ => {
                    self.notify_err("Memory must be a number of megabytes (at least 512)");
                    cx.notify();
                    return;
                }
            }
        };
        let port: u16 = match port.parse() {
            Ok(p) if p > 0 => p,
            _ => {
                self.notify_err("Port must be a number between 1 and 65535");
                cx.notify();
                return;
            }
        };
        let maxp: u32 = maxp.parse().unwrap_or(20);
        self.update_instance(id, |i| {
            if !name.is_empty() {
                i.name = name;
            }
            i.memory_mb = memory_mb;
            i.jvm_args = jvm;
            i.server.port = port;
            i.server.motd = motd;
            i.server.max_players = maxp;
        });
        if let Some(i) = self.instance(id) {
            if i.kind == Kind::Server && i.game_dir().join("server.properties").exists() {
                let _ = instance::write_server_files(i);
            }
        }
        self.notify_ok("Saved");
        cx.notify();
    }
}

/// Button styled as a subtle icon button.
trait GhostLike {
    fn ghost_like(self) -> Self;
}
impl GhostLike for Button {
    fn ghost_like(self) -> Self {
        self
    }
}
