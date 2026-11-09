//! Prototype probe for client self reads (`game.get-effects` and
//! `game.get-self-state`, API 18): each tick it shows the local
//! player's active effects — name, level and the time left, in the
//! game's own order — and one line of movement state with the
//! ridden mount's health, in a HUD panel.
use horizon_mod_sdk::{
    game::{self, Effect},
    hud::{self, Corner, Line},
    Mod, Registration,
};

struct SelfProbe {
    last_panel: Vec<String>,
}

/// One effect's line: the id's path, its level in Roman numerals
/// past I, and the time left as m:ss — or `inf` for an infinite one.
fn effect_line(e: &Effect) -> String {
    let name = e.id.rsplit(':').next().unwrap_or(&e.id);
    let level = match e.amplifier {
        0 => String::new(),
        1 => " II".into(),
        2 => " III".into(),
        3 => " IV".into(),
        4 => " V".into(),
        n => format!(" {}", n + 1),
    };
    let time = if e.duration < 0 {
        "inf".into()
    } else {
        let seconds = e.duration / 20;
        format!("{}:{:02}", seconds / 60, seconds % 60)
    };
    format!("{name}{level} {time}")
}

/// The panel's lines for this tick: the effects, then how the
/// player moves and what they ride.
fn panel_lines() -> Vec<String> {
    let mut lines: Vec<String> = game::get_effects().iter().map(effect_line).collect();
    match game::get_self_state() {
        Some(s) => {
            let moving = if s.gliding {
                "gliding"
            } else if s.flying {
                "flying"
            } else if s.sprinting {
                "sprinting"
            } else if s.sneaking {
                "sneaking"
            } else {
                "standing"
            };
            let mut line = String::from(moving);
            if s.sprint_toggled {
                line.push_str(" [sprint toggled]");
            }
            if s.sneak_toggled {
                line.push_str(" [sneak toggled]");
            }
            if let Some(m) = &s.mount {
                line.push_str(&format!(
                    " on {} {:.1}/{:.1}",
                    m.kind, m.health, m.max_health
                ));
            }
            lines.push(line);
        }
        None => lines.push("no self".into()),
    }
    lines
}

impl Mod for SelfProbe {
    fn init() -> Result<(Self, Registration), String> {
        Ok((
            SelfProbe {
                last_panel: Vec::new(),
            },
            Registration::new().on_tick(),
        ))
    }

    fn tick(&mut self, _tick: u64) {
        let panel = panel_lines();
        if panel != self.last_panel {
            let lines: Vec<Line> = panel
                .iter()
                .map(|text| Line {
                    text: text.clone(),
                    color: 0xFFFFFF,
                })
                .collect();
            hud::set_panel(Corner::TopLeft, &lines);
            self.last_panel = panel;
        }
    }
}

horizon_mod_sdk::export_mod!(SelfProbe);
