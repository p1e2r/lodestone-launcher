use gpui_kit::assets::IconName as I;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Sizable as _, StyledExt as _};
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;

use super::theme::*;
use super::{LodestoneApp, page_header};

impl LodestoneApp {
    pub fn accounts_view(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.accounts.selected().map(|a| a.username.clone());
        let accounts = self.accounts.accounts.clone();

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(page_header("Accounts", "Offline accounts. Pick any username for singleplayer and offline-mode servers.", vec![]))
            .child(
                div().id("accounts-scroll").flex_1().overflow_y_scrollbar().child(
                    div()
                        .px(px(32.))
                        .pb(px(32.))
                        .v_flex()
                        .gap(px(14.))
                        .child(
                            card()
                                .p(px(16.))
                                .flex()
                                .gap(px(8.))
                                .child(div().flex_1().child(Input::new(&self.new_account_input)))
                                .child(Button::new("add-account").primary().icon(I::UserPlus).label("Add account").on_click(cx.listener(|this, _, window, cx| this.add_account(window, cx)))),
                        )
                        .when(accounts.is_empty(), |d| {
                            d.child(card().p(px(24.)).v_flex().gap(px(6.)).child(div().font_weight(FontWeight::SEMIBOLD).child("No accounts yet")).child(muted("Add a username to play. Minecraft runs in offline mode, so any name works on singleplayer and offline-mode servers.")))
                        })
                        .children(accounts.into_iter().enumerate().map(|(i, a)| {
                            let is_sel = selected.as_deref() == Some(a.username.as_str());
                            let uuid = a.offline_uuid();
                            let uuid2 = uuid.clone();
                            let (n1, n2) = (a.username.clone(), a.username.clone());
                            card()
                                .p(px(18.))
                                .when(is_sel, |d| d.border_color(rgb(ACCENT_DIM)))
                                .flex()
                                .items_center()
                                .gap(px(14.))
                                .child(avatar(&a.username, 44.))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .v_flex()
                                        .gap(px(3.))
                                        .child(div().flex().items_center().gap(px(8.)).child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child(a.username.clone())).when(is_sel, |d| d.child(badge("Playing as", ACCENT))))
                                        .child(div().flex().items_center().gap(px(6.)).text_xs().text_color(rgb(MUTED)).child("UUID").child(div().font_family(MONO).text_color(rgb(TEXT)).truncate().child(uuid))),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .gap(px(6.))
                                        .child(small_button(("copy", i), "Copy UUID").icon(I::Copy).on_click(cx.listener(move |this, _, _, cx| this.copy(uuid2.clone(), "UUID", cx))))
                                        .when(!is_sel, |d| {
                                            d.child(Button::new(("use", i)).primary().small().label("Play as").on_click(cx.listener(move |this, _, _, cx| {
                                                this.accounts.selected = Some(n1.clone());
                                                let _ = this.accounts.save();
                                                cx.notify();
                                            })))
                                        })
                                        .child(small_button(("remove", i), "Remove").icon(I::Trash).on_click(cx.listener(move |this, _, _, cx| {
                                            match this.accounts.remove(&n2) {
                                                Ok(()) => this.notify_ok(format!("Removed {n2}")),
                                                Err(e) => this.notify_err(format!("{e:#}")),
                                            }
                                            cx.notify();
                                        }))),
                                )
                        })),
                ),
            )
    }
}
