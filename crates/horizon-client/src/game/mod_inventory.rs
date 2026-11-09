//! Capability `inventory` (docs/mods.md): the player's own inventory and
//! the open container's contents, copied into the mods' snapshot once per
//! tick, and only while a mod reads them (`Mods::wants_inventory`). With
//! no inventory mod on, this file does nothing.
//!
//! What the snapshot reads, and why it is the drawn truth:
//!
//! * The screens draw from the `Game` fields (`slot_view` over
//!   `inventory`, `armor`, `offhand`, `craft_grid`, `carried` and the
//!   container's slots), never from the click-prediction `Menu`. That
//!   engine is rebuilt fresh from these fields on every click
//!   (`menu_session`) and its result written back into them, so a
//!   snapshot of the fields shows exactly what the screen shows —
//!   including a click the server has not corrected yet, which the
//!   server's next packet overwrites here the same way it overwrites
//!   the screen. Reading the `Menu` instead would show clicks the
//!   server has not confirmed, from a copy that holds nothing between
//!   clicks.
//! * The open container is derived fresh from `remote_menu()` on every
//!   snapshot: there is no cached container state to go stale. Every
//!   close path funnels through `remote_close_local` — the server's
//!   `ContainerClose`, Esc or the inventory key (`close_container`),
//!   death and respawn and dimension changes (`enter_remote_dimension`),
//!   a broken container or an out-of-range crafting table — and clears
//!   `menu`, so the next tick reads no container. Two gaps the builder
//!   closes itself: a dead player still has `menu` set until they
//!   respawn (neither the client's death screen nor the server's death
//!   path closes it), so the death screen reads no container; and
//!   teleporting within a dimension does not close the menu in vanilla,
//!   so the snapshot keeps showing it there, correctly.
//! * In-frame singleplayer (deprecated) has no `remote`, so a chest open
//!   there reads no container; the inventory slots still read, from the
//!   game's own inventory. Multiplayer reads the same way as
//!   singleplayer: the client learns both from server packets.
use super::{Game, Screen};
use horizon_client::{containers::BlockEntity, items::ItemStack};
use horizon_mods::{Container, InvStack, PlayerInventory};
use horizon_world::nbt::Tag;

#[cfg(feature = "mods")]
impl Game<'_> {
    /// The inventory snapshot for the mods' tick, and the open container's,
    /// from the `Game` fields. `(None, None)` when no running mod reads
    /// capability `inventory`.
    pub(super) fn mod_inventory(&mut self) -> (Option<PlayerInventory>, Option<Container>) {
        if !self.mods.mods.wants_inventory() {
            return (None, None);
        }
        let mut slots = Vec::new();
        for (i, stack) in self.inventory.iter().enumerate() {
            if let Some(stack) = stack {
                slots.push(self.inv_stack(i, stack));
            }
        }
        for (i, stack) in self.craft_grid.iter().enumerate() {
            if let Some(stack) = stack {
                slots.push(self.inv_stack(100 + i, stack));
            }
        }
        if let Some(result) = self.craft_result() {
            slots.push(self.inv_stack(200, &result));
        }
        for (i, stack) in self.armor.iter().enumerate() {
            if let Some(stack) = stack {
                slots.push(self.inv_stack(500 + i, stack));
            }
        }
        if let Some(stack) = &self.offhand {
            slots.push(self.inv_stack(504, stack));
        }
        let inventory = PlayerInventory {
            selected: self.selected as u32,
            saturation: self
                .player
                .as_ref()
                .map_or(0.0, |p| p.health.food.saturation),
            slots,
            // The carried stack has no slot of its own; its slot number
            // is never read.
            carried: self.carried.as_ref().map(|c| self.inv_stack(0, c)),
        };
        let (id, kind) = self.remote_menu();
        if id == 0 || self.screen == Screen::Dead {
            return (Some(inventory), None);
        }
        // The container's own slots, in menu order: every slot the menu
        // maps that is not one of the player's own (0-35, 100-108, 200,
        // 500-504). A chest-like menu's are 300+; a workstation's are its
        // input/result slots; a mount's are its saddle, armour and chest.
        let mut container_slots = Vec::new();
        let mut s = 0;
        while let Some(internal) = Self::menu_slot(&kind, s) {
            if !is_player_slot(internal)
                && let Some(stack) = self.slot_view(internal)
            {
                container_slots.push(self.inv_stack(internal, &stack));
            }
            s += 1;
        }
        let data = self.mod_container_data(&kind);
        (
            Some(inventory),
            Some(Container {
                kind,
                slots: container_slots,
                data,
            }),
        )
    }

    /// One slot's stack as the WIT spells it: the id, count and damage
    /// as stored, the maximum damage from the item registry, and the
    /// components beyond those in vanilla item syntax.
    fn inv_stack(&self, slot: usize, stack: &ItemStack) -> InvStack {
        let max_damage = self
            .res
            .items()
            .get(&stack.item)
            .map_or(0, |i| i.max_damage);
        InvStack {
            slot: slot as u32,
            id: stack.item.to_string(),
            count: stack.count,
            damage: stack.damage,
            max_damage,
            components: components(stack),
        }
    }

    /// The open container's numeric data slots, from the same `Game`
    /// fields the screens read, in the order the screen shows them (see
    /// `docs/mods.md` for each kind's layout). Empty for a kind with
    /// none — a plain chest, a merchant (offers are deferred), a mount.
    fn mod_container_data(&self, kind: &str) -> Vec<i32> {
        let station = self.station.as_ref();
        match kind {
            "minecraft:furnace" | "minecraft:blast_furnace" | "minecraft:smoker" => {
                match self
                    .container
                    .and_then(|key| self.world.block_entities.get(&key))
                {
                    Some(BlockEntity::Furnace(f)) => vec![
                        f.lit_remaining,
                        f.lit_total,
                        f.cooking_timer,
                        f.cooking_total,
                    ],
                    _ => Vec::new(),
                }
            }
            "minecraft:brewing_stand" => {
                match self
                    .container
                    .and_then(|key| self.world.block_entities.get(&key))
                {
                    Some(BlockEntity::Brewing(b)) => {
                        vec![b.brew_time, b.fuel, b.total_brew_time, b.total_fuel]
                    }
                    _ => Vec::new(),
                }
            }
            "minecraft:crafter_3x3" => {
                match self
                    .container
                    .and_then(|key| self.world.block_entities.get(&key))
                {
                    Some(BlockEntity::Crafter(c)) => {
                        let mut data: Vec<i32> = c.disabled.iter().map(|d| i32::from(*d)).collect();
                        data.push(i32::from(c.triggered));
                        data
                    }
                    _ => Vec::new(),
                }
            }
            "minecraft:enchantment" => {
                let Some(s) = station else {
                    return Vec::new();
                };
                let mut data = s.offers.costs.to_vec();
                data.push(self.level.xp_seed);
                data.extend(
                    s.offers
                        .clues
                        .iter()
                        .map(|c| c.as_ref().map_or(-1, |(_, level)| *level as i32)),
                );
                data
            }
            "minecraft:beacon" => {
                let Some(s) = station else {
                    return Vec::new();
                };
                vec![
                    self.beacon_levels(),
                    s.beacon[0].map_or(-1, |i| i as i32),
                    s.beacon[1].map_or(-1, |i| i as i32),
                ]
            }
            "minecraft:loom" => station.map_or(Vec::new(), |s| {
                vec![s.loom.selected.map_or(-1, |i| i as i32)]
            }),
            "minecraft:stonecutter" => {
                station.map_or(Vec::new(), |s| vec![s.selected.map_or(-1, |i| i as i32)])
            }
            "minecraft:anvil" => station.map_or(Vec::new(), |s| vec![s.anvil.cost]),
            "minecraft:lectern" => match &self.book {
                Some(super::books::BookScreen::View {
                    page,
                    lectern: Some(_),
                    ..
                }) => vec![*page as i32],
                _ => Vec::new(),
            },
            _ => Vec::new(),
        }
    }
}

/// A slot of the player's own inventory: hotbar and main (0-35), the
/// crafting grid (100-108) and its computed result (200), armour and
/// offhand (500-504). Everything else the menu maps is the container's.
fn is_player_slot(i: usize) -> bool {
    matches!(i, 0..36 | 100..109 | 200 | 500..505)
}

/// A stack's components beyond its item's, as vanilla's item syntax writes
/// them (`[minecraft:potion_contents={potion:"minecraft:swiftness"}]`), or "".
/// Same helper as the recipes host's (`game/native_recipes.rs`); shared
/// with the merchant's trade stacks (`screen.trades`, API 22), which
/// spell a stack the same way.
pub(super) fn components(s: &ItemStack) -> String {
    let Some(Tag::Compound(entries)) = horizon_client::item_data::encode_components(s) else {
        return String::new();
    };
    let parts: Vec<String> = entries
        .iter()
        .map(|(k, v)| {
            format!(
                "{}={}",
                k.to_string_lossy(),
                horizon_client::snbt::to_snbt(v)
            )
        })
        .collect();
    format!("[{}]", parts.join(","))
}
