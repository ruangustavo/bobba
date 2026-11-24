use std::sync::Arc;

use bobba_protocol::Direction;
use gpui_kit::TestSupportExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::*;

use crate::backend::Host;

pub const INJECTION_NAME: &str = "Injection";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendResult {
    Sent(Direction),
    Refused(String),
}

pub struct InjectionPanel {
    host: Arc<Host>,
    editor: Entity<TextareaState>,
    last: Option<SendResult>,
}

impl InjectionPanel {
    pub fn new(host: Arc<Host>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let editor = cx.new(|cx| {
            TextareaState::new(window, cx)
                .rows(6)
                .placeholder("{out:Chat}{s:\"hello\"}{i:0}{i:-1}")
        });
        Self {
            host,
            editor,
            last: None,
        }
    }

    pub fn load(&mut self, expression: String, window: &mut Window, cx: &mut Context<Self>) {
        self.editor
            .update(cx, |editor, cx| editor.set_value(expression, window, cx));
        self.last = None;
        cx.notify();
    }

    pub fn send(&mut self, direction: Direction, cx: &mut Context<Self>) {
        let text = self.editor.read(cx).value();
        self.last = Some(
            match self
                .host
                .send_expression(INJECTION_NAME, direction, text.trim())
            {
                Ok(()) => SendResult::Sent(direction),
                Err(error) => SendResult::Refused(error.to_string()),
            },
        );
        cx.notify();
    }

    pub fn last_result(&self) -> Option<&SendResult> {
        self.last.as_ref()
    }
}

impl Render for InjectionPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status = match &self.last {
            None => div(),
            Some(SendResult::Sent(Direction::ToServer)) => {
                div().text_color(cx.theme().green).child("Sent to server")
            }
            Some(SendResult::Sent(Direction::ToClient)) => {
                div().text_color(cx.theme().green).child("Sent to client")
            }
            Some(SendResult::Refused(reason)) => {
                div().text_color(cx.theme().red).child(reason.clone())
            }
        };
        v_flex()
            .size_full()
            .gap_3()
            .p_4()
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("Packet expression. Names come from Sulek once a hotel is connected."),
            )
            .child(
                div()
                    .id("injection-editor")
                    .child(Textarea::new(&self.editor).font_family("Menlo")),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("send-to-server")
                            .label("Send to server")
                            .primary()
                            .on_click(
                                cx.listener(|this, _, _, cx| this.send(Direction::ToServer, cx)),
                            ),
                    )
                    .child(
                        Button::new("send-to-client")
                            .label("Send to client")
                            .on_click(
                                cx.listener(|this, _, _, cx| this.send(Direction::ToClient, cx)),
                            ),
                    )
                    .child(status.id("injection-status").test_support().text_sm()),
            )
    }
}
