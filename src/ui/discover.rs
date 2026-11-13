//! Discover: browse Modrinth and install into an instance.

use std::collections::HashSet;

use gpui_kit::assets::IconName as I;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Disableable as _, Sizable as _, StyledExt as _};
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;

use super::theme::*;
use super::{LodestoneApp, page_header};
use crate::core::instance::Kind;
use crate::core::modrinth::{self, ContentType, SearchHit};
use crate::core::session::{Reporter, RunState};

pub struct DiscoverState {
    pub query: Entity<InputState>,
    pub ctype: ContentType,
    /// Instance id to install into.
    pub target: Option<String>,
    pub results: Vec<SearchHit>,
    pub total: u64,
    pub loading: bool,
    pub error: Option<String>,
    pub installing: HashSet<String>,
    pub generation: u64,
    pub searched: bool,
}

impl DiscoverState {
    pub fn new(window: &mut Window, cx: &mut Context<LodestoneApp>) -> Self {
        DiscoverState {
            query: cx.new(|cx| InputState::new(window, cx).placeholder("Search Modrinth — e.g. sodium, create, faithful")),
            ctype: ContentType::Mod,
            target: None,
            results: vec![],
            total: 0,
            loading: false,
            error: None,
            installing: HashSet::new(),
            generation: 0,
            searched: false,
        }
    }
}

impl LodestoneApp {
    pub fn discover_search(&mut self, reset: bool, cx: &mut Context<Self>) {
        let q = self.discover.query.read(cx).value().to_string();
        let ct = self.discover.ctype;
        let target = self.discover.target.as_deref().and_then(|id| self.instance(id)).cloned();
        if reset {
            self.discover.results.clear();
        }
        let offset = self.discover.results.len() as u32;
        self.discover.loading = true;
        self.discover.error = None;
        self.discover.searched = true;
        self.discover.generation += 1;
        let generation = self.discover.generation;
        self.bg(cx, async move {
            // Recognise mods already in the target (from a modpack or dropped in) first,
            // so they show as "Installed".
            if let Some(inst) = &target {
                if offset == 0 {
                    let _ = modrinth::sync_installed(inst).await;
                }
            }
            modrinth::search(&q, ct, target.as_ref(), offset).await.map_err(|e| format!("{e:#}"))
        }, move |this, r, cx| {
            if this.discover.generation != generation {
                return;
            }
            this.discover.loading = false;
            match r {
                Ok(resp) => {
                    this.discover.total = resp.total_hits;
                    for h in &resp.hits {
                        if let Some(u) = &h.icon_url {
                            this.ensure_icon(u, cx);
                        }
                    }
                    this.discover.results.extend(resp.hits);
                }
                Err(e) => this.discover.error = Some(e),
            }
        });
        cx.notify();
    }

    fn install_hit(&mut self, hit: &SearchHit, cx: &mut Context<Self>) {
        let pid = hit.project_id.clone();
        let title = hit.title.clone();
        if self.discover.ctype == ContentType::Modpack {
            let kind = self.discover.target.as_deref().and_then(|id| self.instance(id)).map(|i| i.kind).unwrap_or(Kind::Client);
            self.discover.installing.insert(pid.clone());
            let rep = Reporter::new("import");
            rep.clear_log();
            rep.set_state(RunState::Working);
            self.notify_ok(format!("Installing modpack “{title}”…"));
            self.bg(
                cx,
                async move {
                    let r = modrinth::install_modpack(&pid, kind, &rep).await;
                    rep.set_state(RunState::Idle);
                    (pid, r.map(|i| i.name).map_err(|e| format!("{e:#}")))
                },
                |this, (pid, r), _| {
                    this.discover.installing.remove(&pid);
                    this.reload_instances();
                    match r {
                        Ok(name) => this.notify_ok(format!("Created “{name}” — find it in your Library")),
                        Err(e) => this.notify_err(e),
                    }
                },
            );
            cx.notify();
            return;
        }
        let Some(inst) = self.discover.target.as_deref().and_then(|id| self.instance(id)).cloned() else {
            self.notify_err("Choose an instance to install into first");
            cx.notify();
            return;
        };
        self.discover.installing.insert(pid.clone());
        let rep = Reporter::new(&inst.id);
        self.bg(
            cx,
            async move { (pid.clone(), modrinth::install_project(&inst, &pid, &rep).await.map_err(|e| format!("{e:#}"))) },
            move |this, (pid, r), _| {
                this.discover.installing.remove(&pid);
                match r {
                    Ok(names) if names.len() > 1 => this.notify_ok(format!("Installed {} (+ {} dependencies)", names[0], names.len() - 1)),
                    Ok(_) => this.notify_ok(format!("Installed {title}")),
                    Err(e) => this.notify_err(e),
                }
            },
        );
        cx.notify();
    }

    pub fn discover_view(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.discover.searched {
            self.discover_search(true, cx);
        }
        let ct = self.discover.ctype;
        let entity = cx.entity();
        let type_picker = segmented(
            "ctype",
            ContentType::ALL.iter().map(|c| (*c, SharedString::from(c.label()))).collect(),
            &ct,
            move |c, _, cx| {
                entity.update(cx, |this, cx| {
                    this.discover.ctype = c;
                    this.discover_search(true, cx);
                })
            },
        );

        // Install target chips
        let target = self.discover.target.clone();
        let targets: Vec<(String, String, u32)> = self
            .instances
            .iter()
            .map(|i| (i.id.clone(), format!("{} · {} {}", i.name, i.mc_version, if i.loader == crate::core::loaders::Loader::Vanilla { String::new() } else { i.loader.name().to_string() }), loader_color(i.loader)))
            .collect();
        let target_inst = target.as_deref().and_then(|id| self.instance(id)).cloned();
        let target_row = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.))
            .child(div().text_xs().text_color(rgb(MUTED)).mr(px(4.)).child(if ct == ContentType::Modpack { "Filter by:" } else { "Install into:" }))
            .child(
                div()
                    .id("t-none")
                    .px(px(10.))
                    .py(px(4.))
                    
                    .text_xs()
                    .cursor_pointer()
                    .border_1()
                    .border_color(rgb(BORDER))
                    .when(target.is_none(), |d| d.bg(rgb(PANEL_3)).text_color(rgb(TEXT)))
                    .when(target.is_some(), |d| d.text_color(rgb(MUTED)))
                    .child("Everything")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.discover.target = None;
                        this.discover_search(true, cx);
                    })),
            )
            .children(targets.into_iter().enumerate().map(|(i, (id, label, col))| {
                let active = target.as_deref() == Some(id.as_str());
                div()
                    .id(("t", i))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .px(px(10.))
                    .py(px(4.))
                    
                    .text_xs()
                    .cursor_pointer()
                    .border_1()
                    .border_color(if active { rgb(col) } else { rgb(BORDER) })
                    .when(active, |d| d.bg(rgb(PANEL_3)).text_color(rgb(TEXT)))
                    .when(!active, |d| d.text_color(rgb(MUTED)))
                    .child(div().size(px(7.)).bg(rgb(col)))
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.discover.target = Some(id.clone());
                        this.discover_search(true, cx);
                    }))
            }));

        let index = target_inst.as_ref().map(modrinth::InstalledIndex::load);
        let results = self.discover.results.clone();
        let loading = self.discover.loading;
        let error = self.discover.error.clone();
        let more = (self.discover.results.len() as u64) < self.discover.total;

        let modpack_note = (ct == ContentType::Modpack).then(|| {
            card()
                .p(px(14.))
                .flex()
                .items_start()
                .gap(px(10.))
                .child(icon(I::Info).text_color(rgb(INFO)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .v_flex()
                        .gap(px(4.))
                        .text_sm()
                        .child(div().font_weight(FontWeight::SEMIBOLD).child("Modpacks create a new instance"))
                        .child(muted("Installing a modpack never changes your existing instances. Its mods are marked as installed only in the new instance it creates, not anywhere else.")),
                )
        });
        let list = div()
            .v_flex()
            .gap(px(10.))
            .children(modpack_note)
            .when_some(error, |d, e| d.child(card().p(px(16.)).text_sm().text_color(rgb(DANGER)).child(format!("Couldn't search Modrinth: {e}"))))
            .when(!loading && results.is_empty() && self.discover.error.is_none(), |d| d.child(card().p(px(24.)).child(muted("No results. Try another search, or pick “Everything” to search all versions."))))
            .children(results.iter().enumerate().map(|(i, hit)| {
                let installed = match (&index, &target_inst) {
                    (Some(idx), Some(inst)) => idx.has(inst, &hit.project_id),
                    _ => false,
                };
                let installing = self.discover.installing.contains(&hit.project_id);
                let icon_path = hit.icon_url.as_ref().and_then(|u| self.icons.get(u)).cloned();
                let hit2 = hit.clone();
                let slug = hit.slug.clone();
                let ptype = hit.project_type.clone();
                let action_label = if ct == ContentType::Modpack {
                    if target_inst.as_ref().map(|i| i.kind == Kind::Server).unwrap_or(false) { "Install as server" } else { "Install" }
                } else if installed {
                    "Installed"
                } else {
                    "Install"
                };
                let cats: Vec<String> = hit.categories.iter().take(4).cloned().collect();
                card()
                    .id(("hit", i))
                    .p(px(14.))
                    .flex()
                    .gap(px(14.))
                    .child(match icon_path {
                        Some(p) => img(p).size(px(56.)).flex_none().into_any_element(),
                        None => div().size(px(56.)).flex_none().bg(rgb(PANEL_3)).flex().items_center().justify_center().child(icon(I::Package).text_color(rgb(FAINT))).into_any_element(),
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .v_flex()
                            .gap(px(4.))
                            .child(
                                div()
                                    .flex()
                                    .items_baseline()
                                    .gap(px(8.))
                                    .child(div().id(("title", i)).text_base().font_weight(FontWeight::SEMIBOLD).cursor_pointer().hover(|s| s.text_color(rgb(ACCENT))).child(hit.title.clone()).on_click(move |_, _, cx| cx.open_url(&format!("https://modrinth.com/{ptype}/{slug}"))))
                                    .child(div().text_xs().text_color(rgb(FAINT)).child(format!("by {}", hit.author))),
                            )
                            .child(div().text_sm().text_color(rgb(MUTED)).line_clamp(2).child(hit.description.clone()))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(10.))
                                    .mt(px(2.))
                                    .child(div().flex().items_center().gap(px(4.)).text_xs().text_color(rgb(FAINT)).child(icon(I::Download).size(px(12.))).child(fmt_count(hit.downloads)))
                                    .children(cats.into_iter().map(|c| div().text_xs().text_color(rgb(FAINT)).child(c))),
                            ),
                    )
                    .child(
                        div().flex_none().flex().items_center().child(
                            Button::new(("install", i))
                                .map(|b| if installed { b } else { b.primary() })
                                .icon(if installed { I::Check } else { I::Download })
                                .label(if installing { "Installing…" } else { action_label })
                                .disabled(installing || (installed && ct != ContentType::Modpack))
                                .on_click(cx.listener(move |this, _, _, cx| this.install_hit(&hit2, cx))),
                        ),
                    )
            }))
            .when(loading, |d| d.child(div().p(px(20.)).flex().justify_center().child(muted("Searching…"))))
            .when(more && !loading, |d| d.child(div().flex().justify_center().pt(px(6.)).child(Button::new("more").label("Load more").on_click(cx.listener(|this, _, _, cx| this.discover_search(false, cx))))));

        let subtitle = match (&target_inst, ct) {
            (_, ContentType::Modpack) => "Every modpack installs as its own new instance. Pick a server to install a pack as a server.".to_string(),
            (Some(i), _) => format!("Showing {} compatible with {} ({} {}).", ct.label().to_lowercase(), i.name, i.mc_version, i.loader.name()),
            (None, _) => "Browse Modrinth. Pick an instance to see only compatible content and install it.".to_string(),
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(page_header("Discover", subtitle, vec![]))
            .child(
                div()
                    .px(px(32.))
                    .pb(px(14.))
                    .v_flex()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(div().flex_1().child(Input::new(&self.discover.query).prefix(icon(I::Search).text_color(rgb(FAINT))).cleanable(true)))
                            .child(Button::new("search").primary().label("Search").on_click(cx.listener(|this, _, _, cx| this.discover_search(true, cx)))),
                    )
                    .child(type_picker)
                    .child(target_row),
            )
            .child(div().id("results-scroll").flex_1().overflow_y_scrollbar().child(div().px(px(32.)).pb(px(32.)).child(list)))
            .child(div().px(px(32.)).py(px(8.)).border_t_1().border_color(rgb(BORDER)).text_xs().text_color(rgb(FAINT)).child("Content from Modrinth (modrinth.com). Lodestone is not affiliated with Modrinth."))
    }
}

#[allow(unused)]
fn _sizable_check(b: Button) -> Button {
    b.small()
}
