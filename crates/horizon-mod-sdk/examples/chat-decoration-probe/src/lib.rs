//! Prototype probe for chat-decoration (API 27): its
//! `/style` command declares the district's whole policy —
//! a player's own head beside the chat lines they wrote,
//! and the chat box animated as messages arrive and as it
//! opens, at the durations the chat-animation mods ship
//! (150 ms a message, fading in; 170 ms the input field;
//! the indicator's bar kept) — and `/style off` clears
//! both declarations, back to vanilla chat. The game does
//! all of the drawing: the probe names no player, no line
//! and no picture, and reads nothing back.
use horizon_mod_sdk::{ChatAnimation, LineDecoration, Mod, Registration, chat_decoration};

struct ChatDecorationProbe;

impl Mod for ChatDecorationProbe {
    fn init() -> Result<(Self, Registration), String> {
        Ok((ChatDecorationProbe, Registration::new().command("style")))
    }

    fn command(&mut self, _name: &str, args: &str) -> Result<Option<String>, String> {
        if args == "off" {
            chat_decoration::set_line_decoration(None);
            chat_decoration::set_animation(None);
            return Ok(Some("chat cleared".into()));
        }
        chat_decoration::set_line_decoration(Some(LineDecoration { sender_head: true }));
        chat_decoration::set_animation(Some(ChatAnimation {
            message_ms: 150,
            fade_in: true,
            input_ms: 170,
            hide_indicator: false,
        }));
        Ok(Some("chat styled".into()))
    }
}

horizon_mod_sdk::export_mod!(ChatDecorationProbe);
