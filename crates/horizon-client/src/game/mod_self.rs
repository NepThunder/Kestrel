//! Capability `game-read`'s self reads (API 18, docs/api/mods-reference.md):
//! the local player's own active effects with their remaining ticks,
//! their movement state, the mount they ride and the movement toggle
//! options, copied into the mods' snapshot once per tick, and only
//! while a mod reads them (`Mods::wants_self_state`). With no such mod
//! on, this file does nothing.
//!
//! What the snapshot reads, and why it is the game's own truth:
//!
//! * The effects read `Game.potions.effects` — the client's own
//!   `PlayerEffects` list, in `getActiveEffects` order: the same list
//!   the HUD's effect icons read. On a server the list is what
//!   `ClientboundUpdateMobEffectPacket` set for the player, and the
//!   client counts each duration down itself
//!   (`EffectInstance::tick_client`), so the remaining ticks a mod
//!   reads are the ones the player's own screen counts down.
//! * Ids are namespaced as the save namespaces them
//!   (`potions::namespaced`): `speed` reads as `minecraft:speed`, a
//!   content mod's id keeps its namespace.
//! * The client's effect instance has no show-icon bit: an effect
//!   that arrives by packet always shows its icon (`PlayerEffects::
//!   flags` rebuilds the packet flags with the icon always shown),
//!   so `show_icon` is true, and `show_particles` is the instance's
//!   `visible`.
//! * The movement flags read the `PlayerMotion` the game moves:
//!   `sprinting`, `flying` and `fall_flying` (gliding, tracked apart
//!   from flying) directly, and sneaking as the sneak key down or a
//!   forced crouch.
//! * The mount is `Game.riding`'s entity looked up in `Game.mobs`,
//!   the same fields `mod_entities` reports for any mob; its health
//!   is the client's server-synched value. A vehicle that is not a
//!   living mob (a boat, a minecart) reads as no mount in v1.
//! * The toggles read `Settings` — and [`Game::mod_set_movement_toggles`]
//!   writes them there and saves options.txt, as an options-page
//!   change does. Only the options move; no key state is touched, so
//!   a latched sprint keeps its latch until the key is next pressed,
//!   exactly as when the player flips the option by hand.
use super::Game;
use horizon_mods::{ModEffect, ModMount, ModSelfState};

#[cfg(feature = "mods")]
impl Game<'_> {
    /// The effects and self-state for the mods' snapshot (API 18):
    /// `(Vec::new(), None)` when no running mod reads them, and the
    /// same on the title screen.
    pub(super) fn mod_self_reads(&self) -> (Vec<ModEffect>, Option<ModSelfState>) {
        if !self.mods.mods.wants_self_state() {
            return (Vec::new(), None);
        }
        let Some(player) = &self.player else {
            return (Vec::new(), None);
        };
        let effects = self
            .potions
            .effects
            .list
            .iter()
            .map(|e| ModEffect {
                id: horizon_client::potions::namespaced(e.id).to_owned(),
                amplifier: e.amplifier.max(0) as u32,
                duration: e.duration,
                ambient: e.ambient,
                show_particles: e.visible,
                show_icon: true,
            })
            .collect();
        let mount = self.riding.and_then(|id| {
            self.mobs
                .iter()
                .find(|m| !m.removed && m.id == id)
                .map(|m| ModMount {
                    id: m.id,
                    kind: m.kind.id.clone(),
                    health: m.health,
                    max_health: m.kind.max_health,
                })
        });
        let state = ModSelfState {
            sprinting: player.sprinting,
            sneaking: player.shift_key_down || player.local_crouching,
            flying: player.flying,
            gliding: player.fall_flying,
            mount,
            sprint_toggled: self.settings.toggle_sprint,
            sneak_toggled: self.settings.toggle_crouch,
        };
        (effects, Some(state))
    }

    /// `game.set-movement-toggles` (API 18), drained as a mods
    /// output: set the Toggle Sprint / Toggle Crouch options a mod
    /// asked for and save them, as the options page's change reaches
    /// options.txt. The touch settings mirror the sprint option (the
    /// game keeps the two in step itself). Nothing else moves: the
    /// options govern how the keys latch from their next press.
    pub(super) fn mod_set_movement_toggles(&mut self, sprint: Option<bool>, sneak: Option<bool>) {
        if let Some(on) = sprint {
            self.settings.toggle_sprint = on;
            self.settings.touch.sprint_toggle = on;
            self.settings
                .file
                .set("toggleSprint", horizon_client::options_file::V::Bool(on));
        }
        if let Some(on) = sneak {
            self.settings.toggle_crouch = on;
            self.settings
                .file
                .set("toggleCrouch", horizon_client::options_file::V::Bool(on));
        }
        if sprint.is_some() || sneak.is_some() {
            self.settings.save(&Self::options_path());
        }
    }
}
