//! Prototype probe for client inventory reads (`docs/mods.md`, "Client
//! inventory"): each tick it reads the player's own inventory and the open
//! container and shows them in chat when they change — the held item's
//! damage, how many armour pieces are worn, saturation, and the open
//! container's kind, slot count and data slots. Closing the container, or
//! dying with one open, clears the display, which is the stale-clearing
//! behaviour the snapshot promises.
use horizon_mod_sdk::{Mod, Registration, chat, inventory};

struct InventoryProbe {
    last: String,
}

impl Mod for InventoryProbe {
    fn init() -> Result<(Self, Registration), String> {
        let hooks = Registration::new().on_tick();
        Ok((
            InventoryProbe {
                last: String::new(),
            },
            hooks,
        ))
    }

    fn tick(&mut self, _tick: u64) {
        let inv = inventory::inventory();
        let held = inv
            .slots
            .iter()
            .find(|s| s.slot == inv.selected)
            .map(|s| format!("{} {}/{}", s.id, s.damage, s.max_damage))
            .unwrap_or_else(|| "none".into());
        let armour = inv
            .slots
            .iter()
            .filter(|s| (500..504).contains(&s.slot))
            .count();
        let container = match inventory::open_container() {
            Some(c) => format!(
                "container={} slots={} data={:?}",
                c.kind,
                c.slots.len(),
                c.data
            ),
            None => "container=none".into(),
        };
        let line = format!(
            "probe held={held} armour={armour} sat={:.1} {container}",
            inv.saturation
        );
        if line != self.last {
            self.last = line.clone();
            chat::show(&line);
        }
    }
}

horizon_mod_sdk::export_mod!(InventoryProbe);
