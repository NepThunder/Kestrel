//! Prototype probe for actions (API 21): each tick it shows the
//! crosshair's target (`actions.target`) in a HUD panel — the
//! block, its face, or the entity in front of it — and on its
//! key it queues one use (`actions.use`), which the game
//! performs as the player in the next tick's input step, through
//! the same path a real click takes. Its `/act` command queues
//! the same use for the test harness, which cannot press keys.
use horizon_mod_sdk::{
    ActionTarget, Face, Mod, Registration, actions,
    hud::{self, Corner, Line},
    input::{self, Event},
};

const KEY: &str = "act";

/// A face's plain name for the panel.
fn face_name(face: Face) -> &'static str {
    match face {
        Face::Down => "down",
        Face::Up => "up",
        Face::North => "north",
        Face::South => "south",
        Face::West => "west",
        Face::East => "east",
    }
}

struct ActionProbe {
    last_panel: Vec<String>,
}

/// The panel's lines for this tick: what the crosshair rests on.
fn target_lines(target: Option<&ActionTarget>) -> Vec<String> {
    match target {
        None => vec!["target none".into()],
        Some(ActionTarget::Block(b)) => vec![
            format!("target {} {} {}", b.block_x, b.block_y, b.block_z),
            format!("{} {}", b.block, face_name(b.face)),
        ],
        Some(ActionTarget::Entity(e)) => vec![
            format!("entity {}", e.kind),
            format!("id {} dist {:.1}", e.id, e.distance),
        ],
    }
}

impl Mod for ActionProbe {
    fn init() -> Result<(Self, Registration), String> {
        input::register_key(KEY, "Use the target", "key.keyboard.g")?;
        Ok((
            ActionProbe {
                last_panel: Vec::new(),
            },
            Registration::new().on_tick().command("act"),
        ))
    }

    fn tick(&mut self, _tick: u64) {
        for event in input::poll() {
            if let Event::KeyDown(k) = event
                && k == KEY
            {
                actions::use_();
            }
        }
        let panel = target_lines(actions::target().as_ref());
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

    fn command(&mut self, _name: &str, _args: &str) -> Result<Option<String>, String> {
        actions::use_();
        Ok(Some("used".into()))
    }
}

horizon_mod_sdk::export_mod!(ActionProbe);
