//! Prototype probe for inventory-actions (API 24): on its key
//! it queues a quick-move of the first main-inventory stack
//! (slot 9) and a left drag over the hotbar's first slots
//! (`inventory-actions.click`, `inventory-actions.drag`), which
//! the game performs as the player in the next tick, through
//! its own menu logic. Each tick it also polls the wheel turns
//! over the open screen (`inventory-actions.poll-wheel`) and
//! shows the latest in a HUD panel — observed, never consumed.
//! Its `/inv` command queues the same moves for the test
//! harness, which cannot press keys.
use horizon_mod_sdk::{
    ClickKind, DragButton, Mod, Registration,
    hud::{self, Corner, Line},
    input::{self, Event},
    inventory_actions,
};

const KEY: &str = "inv";

struct InventoryActionsProbe {
    note: String,
    last_panel: Vec<String>,
}

/// The moves the key and the command both queue: quick-move
/// the first main-inventory stack, then drag the hand's stack
/// over the hotbar's first three slots.
fn queue_moves() -> Result<(), String> {
    inventory_actions::click(Some(9), 0, ClickKind::QuickMove)?;
    inventory_actions::drag(DragButton::Left, &[0, 1, 2])?;
    Ok(())
}

impl Mod for InventoryActionsProbe {
    fn init() -> Result<(Self, Registration), String> {
        input::register_key(KEY, "Quick-move and drag", "key.keyboard.h")?;
        Ok((
            InventoryActionsProbe {
                note: "inventory actions probe".into(),
                last_panel: Vec::new(),
            },
            Registration::new().on_tick().command("inv"),
        ))
    }

    fn tick(&mut self, _tick: u64) {
        for event in input::poll() {
            if let Event::KeyDown(k) = event
                && k == KEY
                && queue_moves().is_ok()
            {
                self.note = "moved slot 9".into();
            }
        }
        let turns = inventory_actions::poll_wheel();
        let panel = match turns.last() {
            Some(turn) => {
                let slot = match turn.slot {
                    Some(slot) => slot.to_string(),
                    None => "none".into(),
                };
                vec![format!("wheel {slot} {}", turn.steps)]
            }
            None => vec![self.note.clone()],
        };
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
        queue_moves()?;
        self.note = "moved slot 9".into();
        Ok(Some("moved".into()))
    }
}

horizon_mod_sdk::export_mod!(InventoryActionsProbe);
