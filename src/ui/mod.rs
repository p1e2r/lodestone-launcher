//! Lodestone's GPUI interface.

mod accounts;
pub mod art;
mod dialog;
mod discover;
mod instance_view;
mod library;
mod settings_view;
pub mod theme;

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::StyledExt as _;
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;

use crate::core::accounts::Accounts;
use crate::core::instance::{Instance, Kind};
use crate::core::model::VersionManifest;
use crate::core::net::RT;
use crate::core::session::{self, Reporter, RunState};
use crate::core::settings::Settings;
use crate::core::{instance, modrinth, versions};
use theme::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Library,
    Servers,
    Discover,
    Accounts,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Content,
    Console,
    Options,
}

pub struct Toast {
    pub text: String,
    pub error: bool,
    pub at: std::time::Instant,
}

pub struct LodestoneApp {
    pub page: Page,
    pub instances: Vec<Instance>,
    /// Instance shown in the detail view (id).
    pub open: Option<String>,
    pub tab: Tab,
    pub content_sub: &'static str,
    pub settings: Settings,
    pub accounts: Accounts,
    pub manifest: Option<VersionManifest>,
    pub manifest_error: Option<String>,
    pub dialog: Option<dialog::NewInstance>,
    pub discover: discover::DiscoverState,
    pub toast: Option<Toast>,
    pub icons: HashMap<String, PathBuf>,
    pub icon_requests: HashSet<String>,
    pub console_scroll: UniformListScrollHandle,
    pub console_follow: bool,
    pub confirm_delete: Option<String>,
    last_epoch: u64,
    last_log_len: usize,
    // Inputs
    pub new_account_input: Entity<InputState>,
    pub command_input: Entity<InputState>,
    pub inst_inputs: Option<instance_view::InstanceInputs>,
    pub settings_inputs: settings_view::SettingsInputs,
    _subs: Vec<Subscription>,
}

impl LodestoneApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let new_account_input = cx.new(|cx| InputState::new(window, cx).placeholder("Username (3–16 letters, numbers, _)"));
        let command_input = cx.new(|cx| InputState::new(window, cx).placeholder("Type a server command, e.g. say hello  ·  op Steve"));
        let settings = Settings::load();
        let settings_inputs = settings_view::SettingsInputs::new(&settings, window, cx);
        let discover = discover::DiscoverState::new(window, cx);

        let mut subs = Vec::new();
        subs.push(cx.subscribe_in(&new_account_input, window, |this, _, ev: &InputEvent, window, cx| {
            if matches!(ev, InputEvent::PressEnter { .. }) {
                this.add_account(window, cx);
            }
        }));
        subs.push(cx.subscribe_in(&command_input, window, |this, _, ev: &InputEvent, window, cx| {
            if matches!(ev, InputEvent::PressEnter { .. }) {
                this.send_command(window, cx);
            }
        }));
        subs.push(cx.subscribe_in(&discover.query, window, |this, _, ev: &InputEvent, _window, cx| {
            if matches!(ev, InputEvent::PressEnter { .. }) {
                this.discover_search(true, cx);
            }
        }));

        let mut app = LodestoneApp {
            page: Page::Library,
            instances: Instance::load_all(),
            open: None,
            tab: Tab::Console,
            content_sub: "mods",
            settings,
            accounts: Accounts::load(),
            manifest: None,
            manifest_error: None,
            dialog: None,
            discover,
            toast: None,
            icons: HashMap::new(),
            icon_requests: HashSet::new(),
            console_scroll: UniformListScrollHandle::new(),
            console_follow: true,
            confirm_delete: None,
            last_epoch: 0,
            last_log_len: 0,
            new_account_input,
            command_input,
            inst_inputs: None,
            settings_inputs,
            _subs: subs,
        };
        app.load_manifest(cx);
        app.start_polling(cx);
        app
    }

    /// Re-render whenever background jobs report progress.
    fn start_polling(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_millis(120)).await;
            let epoch = session::epoch();
            let alive = this
                .update(cx, |this, cx| {
                    if this.last_epoch != epoch {
                        this.last_epoch = epoch;
                        this.on_progress(cx);
                        cx.notify();
                    }
                    if let Some(t) = &this.toast {
                        if t.at.elapsed() > Duration::from_secs(5) {
                            this.toast = None;
                            cx.notify();
                        }
                    }
                })
                .is_ok();
            if !alive {
                break;
            }
        })
        .detach();
    }

    fn on_progress(&mut self, _cx: &mut Context<Self>) {
        if let Some(id) = &self.open {
            let s = session::session(id);
            let len = s.lock().unwrap().log.len();
            if self.console_follow && len != self.last_log_len && len > 0 {
                self.console_scroll.scroll_to_item(len - 1, ScrollStrategy::Bottom);
            }
            self.last_log_len = len;
        }
    }

    /// Run a future on the I/O runtime and deliver its result on the UI thread.
    pub fn bg<T, F>(&mut self, cx: &mut Context<Self>, fut: F, done: impl FnOnce(&mut Self, T, &mut Context<Self>) + 'static)
    where
        T: Send + 'static,
        F: Future<Output = T> + Send + 'static,
    {
        let (tx, rx) = futures::channel::oneshot::channel();
        RT.spawn(async move {
            let _ = tx.send(fut.await);
        });
        cx.spawn(async move |this, cx| {
            if let Ok(v) = rx.await {
                let _ = this.update(cx, |this, cx| {
                    done(this, v, cx);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub fn notify_ok(&mut self, text: impl Into<String>) {
        self.toast = Some(Toast { text: text.into(), error: false, at: std::time::Instant::now() });
    }

    pub fn notify_err(&mut self, text: impl Into<String>) {
        self.toast = Some(Toast { text: text.into(), error: true, at: std::time::Instant::now() });
    }

    fn load_manifest(&mut self, cx: &mut Context<Self>) {
        self.bg(cx, versions::manifest(), |this, r, _| match r {
            Ok(m) => this.manifest = Some(m),
            Err(e) => this.manifest_error = Some(format!("{e:#}")),
        });
    }

    pub fn reload_instances(&mut self) {
        self.instances = Instance::load_all();
    }

    pub fn instance(&self, id: &str) -> Option<&Instance> {
        self.instances.iter().find(|i| i.id == id)
    }

    pub fn open_instance(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.open = Some(id.to_string());
        self.confirm_delete = None;
        let inst = self.instance(id).cloned();
        if let Some(inst) = inst {
            self.tab = if inst.kind == Kind::Server { Tab::Console } else { Tab::Content };
            self.inst_inputs = Some(instance_view::InstanceInputs::new(&inst, window, cx));
            self.page = if inst.kind == Kind::Server { Page::Servers } else { Page::Library };
        }
        self.console_follow = true;
        cx.notify();
    }

    pub fn go(&mut self, page: Page, cx: &mut Context<Self>) {
        self.page = page;
        self.open = None;
        self.confirm_delete = None;
        cx.notify();
    }

    // ------------------------------------------------------------ actions

    pub fn play(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(inst) = self.instance(id).cloned() else { return };
        let rep = Reporter::new(&inst.id);
        if rep.session.lock().unwrap().busy() {
            return;
        }
        rep.clear_log();
        rep.set_state(RunState::Working);
        let settings = self.settings.clone();
        let account = self.accounts.selected().cloned();
        let kind = inst.kind;
        let fut = async move {
            let r = match kind {
                Kind::Client => instance::play(inst, settings, account, rep.clone()).await,
                Kind::Server => instance::start_server(inst, settings, rep.clone()).await,
            };
            match &r {
                Ok(()) => rep.set_state(RunState::Idle),
                Err(e) => rep.set_state(RunState::Failed(format!("{e:#}"))),
            }
            r.map_err(|e| format!("{e:#}"))
        };
        self.bg(cx, fut, |this, r: Result<(), String>, _| {
            this.reload_instances();
            if let Err(e) = r {
                this.notify_err(e);
            }
        });
        cx.notify();
    }

    pub fn stop(&mut self, id: &str, cx: &mut Context<Self>) {
        let s = session::session(id);
        let s = s.lock().unwrap();
        let is_server = self.instance(id).map(|i| i.kind == Kind::Server).unwrap_or(false);
        if is_server {
            if let Some(tx) = &s.stdin {
                let _ = tx.send("stop".into());
                return;
            }
        }
        if let Some(k) = &s.kill {
            k.notify_one();
        }
        cx.notify();
    }

    pub fn send_command(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.open.clone() else { return };
        let text = self.command_input.read(cx).value().to_string();
        if text.trim().is_empty() {
            return;
        }
        let s = session::session(&id);
        let tx = s.lock().unwrap().stdin.clone();
        match tx {
            Some(tx) => {
                Reporter::new(&id).log(session::Level::Launcher, format!("> {text}"));
                let _ = tx.send(text.trim_start_matches('/').to_string());
                self.command_input.update(cx, |s, cx| s.set_value("", window, cx));
            }
            None => self.notify_err("The server isn't running"),
        }
        cx.notify();
    }

    pub fn add_account(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.new_account_input.read(cx).value().trim().to_string();
        match self.accounts.add(&name) {
            Ok(a) => {
                let msg = format!("Added {}", a.username);
                self.notify_ok(msg);
                self.new_account_input.update(cx, |s, cx| s.set_value("", window, cx));
            }
            Err(e) => self.notify_err(format!("{e:#}")),
        }
        cx.notify();
    }

    pub fn copy(&mut self, text: String, what: &str, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.notify_ok(format!("{what} copied to the clipboard"));
        cx.notify();
    }

    pub fn ensure_icon(&mut self, url: &str, cx: &mut Context<Self>) {
        if self.icons.contains_key(url) || !self.icon_requests.insert(url.to_string()) {
            return;
        }
        let u = url.to_string();
        self.bg(cx, async move { modrinth::cached_icon(&u).await.ok() }, {
            let url = url.to_string();
            move |this, path, _| {
                if let Some(p) = path {
                    this.icons.insert(url, p);
                }
            }
        });
    }

    pub fn import_mrpack(&mut self, kind: Kind, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import modpack".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            let _ = this.update(cx, |this, cx| {
                let rep = Reporter::new("import");
                rep.clear_log();
                rep.set_state(RunState::Working);
                this.notify_ok("Importing modpack…");
                this.bg(
                    cx,
                    async move {
                        let r = modrinth::install_mrpack(&path, kind, None, &rep).await;
                        rep.set_state(RunState::Idle);
                        r.map(|i| i.name).map_err(|e| format!("{e:#}"))
                    },
                    |this, r, _| {
                        this.reload_instances();
                        match r {
                            Ok(name) => this.notify_ok(format!("Created “{name}”")),
                            Err(e) => this.notify_err(e),
                        }
                    },
                );
            });
        })
        .detach();
    }

    // ------------------------------------------------------------ render

    fn sidebar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let item = |id: &'static str, icon_name: gpui_kit::assets::IconName, label: &'static str, page: Page, active: bool, cx: &mut Context<Self>| {
            div()
                .id(id)
                .flex()
                .items_center()
                .gap(px(10.))
                .px(px(12.))
                .py(px(8.))
                .rounded(px(9.))
                .cursor_pointer()
                .text_sm()
                .relative()
                .when(active, |d| {
                    d.bg(linear_gradient(90., linear_color_stop(Hsla::from(rgb(ACCENT)).opacity(0.16), 0.), linear_color_stop(Hsla::from(rgb(ACCENT)).opacity(0.02), 1.)))
                        .text_color(rgb(TEXT))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(div().absolute().left_0().top(px(8.)).bottom(px(8.)).w(px(3.)).rounded_full().bg(rgb(ACCENT)))
                })
                .when(!active, |d| d.text_color(rgb(MUTED)).hover(|s| s.bg(rgb(PANEL_2)).text_color(rgb(TEXT))))
                .child(icon(icon_name).text_color(if active { rgb(ACCENT) } else { rgb(MUTED) }))
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| this.go(page, cx)))
        };
        use gpui_kit::assets::IconName as I;
        let p = self.page;
        let acc = self.accounts.selected().cloned();
        let running = self
            .instances
            .iter()
            .filter(|i| session::session(&i.id).lock().unwrap().state == RunState::Running)
            .count();

        div()
            .w(px(232.))
            .flex_none()
            .h_full()
            .bg(linear_gradient(180., linear_color_stop(rgb(0x171b22), 0.), linear_color_stop(rgb(0x121519), 1.)))
            .border_r_1()
            .border_color(rgb(BORDER))
            .flex()
            .flex_col()
            .p(px(14.))
            .gap(px(4.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(6.))
                    .pt(px(4.))
                    .pb(px(18.))
                    .child(img(art::logo()).size(px(30.)).rounded(px(6.)))
                    .child(div().v_flex().child(div().text_lg().font_weight(FontWeight::BOLD).child("Lodestone")).child(div().text_xs().text_color(rgb(FAINT)).child(format!("v{}", env!("CARGO_PKG_VERSION"))))),
            )
            .child(item("nav-library", I::Library, "Library", Page::Library, p == Page::Library && self.open.is_none() || (p == Page::Library), cx))
            .child(item("nav-servers", I::Server, "Servers", Page::Servers, p == Page::Servers, cx))
            .child(item("nav-discover", I::Compass, "Discover", Page::Discover, p == Page::Discover, cx))
            .child(item("nav-accounts", I::Users, "Accounts", Page::Accounts, p == Page::Accounts, cx))
            .child(item("nav-settings", I::Settings, "Settings", Page::Settings, p == Page::Settings, cx))
            .child(div().flex_1())
            .when(running > 0, |d| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .px(px(10.))
                        .py(px(8.))
                        .mb(px(8.))
                        .rounded(px(9.))
                        .bg(rgb(PANEL_2))
                        .text_xs()
                        .text_color(rgb(MUTED))
                        .child(div().size(px(8.)).rounded_full().bg(rgb(ACCENT)))
                        .child(format!("{running} running")),
                )
            })
            .child(
                div()
                    .id("account-card")
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .p(px(10.))
                    .rounded(px(10.))
                    .bg(rgb(PANEL_2))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .cursor_pointer()
                    .hover(|s| s.border_color(rgb(0x3a4252)))
                    .on_click(cx.listener(|this, _, _, cx| this.go(Page::Accounts, cx)))
                    .map(|d| match &acc {
                        Some(a) => d
                            .child(avatar(&a.username, 32.))
                            .child(
                                div()
                                    .v_flex()
                                    .min_w_0()
                                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).truncate().child(a.username.clone()))
                                    .child(div().text_xs().text_color(rgb(FAINT)).child("Offline account")),
                            ),
                        None => d
                            .child(div().size(px(32.)).rounded(px(6.)).bg(rgb(PANEL_3)).flex().items_center().justify_center().child(icon(I::UserPlus).text_color(rgb(MUTED))))
                            .child(div().v_flex().child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("Add account")).child(div().text_xs().text_color(rgb(FAINT)).child("Offline mode"))),
                    }),
            )
    }

    fn toast_view(&self) -> Option<impl IntoElement> {
        let t = self.toast.as_ref()?;
        let col = if t.error { DANGER } else { ACCENT };
        Some(
            div().absolute().bottom(px(20.)).right(px(20.)).max_w(px(460.)).child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(10.))
                    .px(px(14.))
                    .py(px(11.))
                    .rounded(px(10.))
                    .bg(rgb(PANEL_2))
                    .border_1()
                    .border_color(rgb(col))
                    .shadow_lg()
                    .text_sm()
                    .child(div().mt(px(5.)).size(px(8.)).flex_none().rounded_full().bg(rgb(col)))
                    .child(div().text_color(rgb(TEXT)).child(t.text.clone())),
            ),
        )
    }
}

impl Render for LodestoneApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        theme::ensure(cx);
        let main: AnyElement = match (self.page, self.open.clone()) {
            (Page::Library | Page::Servers, Some(id)) => self.instance_view(&id, window, cx).into_any_element(),
            (Page::Library, None) => self.library_view(Kind::Client, cx).into_any_element(),
            (Page::Servers, None) => self.library_view(Kind::Server, cx).into_any_element(),
            (Page::Discover, _) => self.discover_view(cx).into_any_element(),
            (Page::Accounts, _) => self.accounts_view(cx).into_any_element(),
            (Page::Settings, _) => self.settings_view(cx).into_any_element(),
        };
        let dialog = self.dialog.is_some().then(|| self.dialog_view(window, cx).into_any_element());
        div()
            .size_full()
            .relative()
            .flex()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .child(self.sidebar(cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .bg(linear_gradient(165., linear_color_stop(Hsla::from(rgb(ACCENT)).opacity(0.045), 0.), linear_color_stop(Hsla::from(rgb(BG)).opacity(0.0), 0.45)))
                    .child(main),
            )
            .children(dialog)
            .children(self.toast_view())
    }
}

/// Header row used at the top of each page.
pub fn page_header(title: impl Into<SharedString>, subtitle: impl Into<SharedString>, actions: Vec<AnyElement>) -> Div {
    div()
        .flex()
        .items_end()
        .justify_between()
        .gap(px(16.))
        .px(px(32.))
        .pt(px(28.))
        .pb(px(20.))
        .child(
            div()
                .v_flex()
                .gap(px(4.))
                .child(div().text_2xl().font_weight(FontWeight::BOLD).child(title.into()))
                .child(div().text_sm().text_color(rgb(MUTED)).child(subtitle.into())),
        )
        .child(div().flex().gap(px(8.)).children(actions))
}
