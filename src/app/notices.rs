use super::*;

impl Folio {
    pub(super) fn error(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        self.announce(Notice::error(message), false, cx);
    }

    pub(super) fn toast(&mut self, message: &str, cx: &mut Context<Self>) {
        self.announce(Notice::success(message), true, cx);
    }

    pub(super) fn warn(&mut self, message: &str, cx: &mut Context<Self>) {
        self.announce(Notice::warning(message), true, cx);
    }

    pub(super) fn inform(&mut self, title: &'static str, message: &str, cx: &mut Context<Self>) {
        self.announce(Notice::info(title, message), true, cx);
    }

    pub(super) fn status(&mut self, title: &'static str, message: &str, cx: &mut Context<Self>) {
        self.announce(Notice::info(title, message), false, cx);
    }

    pub(super) fn announce(&mut self, notice: Notice, autohide: bool, cx: &mut Context<Self>) {
        let text = notice.text.clone();
        self.notice_id = self.notice_id.wrapping_add(1);
        self.notice_leaving = false;
        self.notice = Some(notice);
        cx.notify();
        if !autohide {
            return;
        }
        let id = self.notice_id;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(2)).await;
            let _ = this.update(cx, |this, cx| {
                if this.notice_id == id && this.notice.as_ref().is_some_and(|n| n.text == text) {
                    this.dismiss_notice(cx);
                }
            });
        })
        .detach();
    }

    pub(super) fn clear_notice(&mut self) {
        self.notice = None;
        self.notice_leaving = false;
    }

    pub(super) fn dismiss_notice(&mut self, cx: &mut Context<Self>) {
        if self.notice.is_none() || self.notice_leaving {
            return;
        }
        self.notice_leaving = true;
        let id = self.notice_id;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(NOTICE_LEAVE).await;
            let _ = this.update(cx, |this, cx| {
                if this.notice_id == id {
                    this.clear_notice();
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn render_notice(&self, notice: &Notice, cx: &mut Context<Self>) -> AnyElement {
        let leaving = self.notice_leaving;
        let mark = notice.mark(cx);
        let body_color = if notice.title.is_some() {
            cx.theme().muted_foreground
        } else {
            cx.theme().foreground
        };
        let card = div()
            .id("notice-card")
            .role(Role::Alert)
            .flex()
            .items_start()
            .w_full()
            .gap_2()
            .px_3()
            .py_2()
            .rounded(px(WORKSPACE_RADIUS))
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().sidebar)
            .shadow_md()
            .child(
                div()
                    .mt(px(1.))
                    .child(Icon::new(notice.icon()).small().text_color(mark)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .gap(px(2.))
                    .when_some(notice.title, |el, title| {
                        el.child(div().text_size(ui(12.)).child(title))
                    })
                    .child(
                        div()
                            .text_size(ui(12.))
                            .text_color(body_color)
                            .child(notice.text.clone()),
                    ),
            )
            .child(Self::icon_button(
                "dismiss-notice",
                IconName::Close,
                "Dismiss",
                |this, _, cx| this.dismiss_notice(cx),
                cx,
            ));
        let slide = NOTICE_WIDTH + NOTICE_INSET;
        div()
            .absolute()
            .top(px(34. + NOTICE_INSET))
            .right(px(NOTICE_INSET))
            .w(px(NOTICE_WIDTH))
            .child(card)
            .with_animation(
                (
                    "notice-slide",
                    self.notice_id.saturating_mul(2) + u64::from(leaving),
                ),
                Animation::new(if leaving { NOTICE_LEAVE } else { NOTICE_ENTER }).with_easing(
                    move |t| {
                        if leaving {
                            t * t * t
                        } else {
                            1. - (1. - t).powi(5)
                        }
                    },
                ),
                move |this, delta| {
                    let offset = if leaving {
                        NOTICE_INSET - delta * slide
                    } else {
                        NOTICE_INSET - (1. - delta) * slide
                    };
                    let opacity = if leaving { 1. - delta } else { delta };
                    this.right(px(offset)).opacity(opacity)
                },
            )
            .into_any_element()
    }
}
