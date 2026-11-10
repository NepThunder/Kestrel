//! Horizon Extras' dynamic lights (`horizon_client::dynamic_light`): the light sources near the
//! camera this frame. Nothing is gathered while the extra is off, no mod policy stands and no
//! mod has registered sources of its own (SDK API 28).
use super::{Game, minecarts::CartKind, shield};
use glam::DVec3;
use horizon_client::blocks::Fluid;
use horizon_client::dynamic_light::{self, BURNING, Luminance, Source};
use horizon_client::projectiles::ProjectileKind;

/// What a mod's own dynamic-light source is tied to (SDK API 28),
/// translated from the runtime's record in `game/mods.rs`.
pub(super) enum ModLightAnchor {
    /// The local player, at hand height.
    SelfPlayer,
    /// A drawn entity — a mob or another player — by id.
    Entity(u32),
    /// A fixed place in the world.
    Position(DVec3),
}

/// One dynamic-light source of a mod's own (SDK API 28): its level,
/// or the item whose glow the game's own [`Luminance`] table gives
/// it — the item wins, so the host's table is never second-guessed
/// by a stale number.
pub(super) struct ModLight {
    pub anchor: ModLightAnchor,
    pub level: u8,
    pub item: Option<String>,
}

/// A mod's policy over the game's own gather (SDK API 28): the
/// classes that shed light, the water rule, and the gathered
/// side's brightness, range and count — the Extras bool's
/// replacement while the policy stands.
pub(super) struct ModLightPolicy {
    pub held_items: bool,
    pub dropped_items: bool,
    pub burning: bool,
    /// (kind, level) pairs, matched against a mob's kind id.
    pub glowing: Vec<(String, u8)>,
    pub projectiles: bool,
    pub water_extinguishes: bool,
    pub brightness: u32,
    pub range: u32,
    pub max_lights: u32,
}

/// The mods' dynamic-light declarations for one frame (SDK API 28):
/// every running mod's own sources, and the one policy that stands.
#[derive(Default)]
pub(super) struct ModLights {
    pub sources: Vec<ModLight>,
    pub policy: Option<ModLightPolicy>,
}

impl Game<'_> {
    /// Fills `dynamic_lights` for the frame: the player's and mobs' held items and fire, and
    /// glowing dropped items, the nearest [`dynamic_light::MAX`] of them. A standing mod
    /// policy (SDK API 28) is the gather's whole input for those classes in place of the
    /// Extras bool — the setting on disk is never written — and adds the classes the built-in
    /// lacks (armor stands' held items, glowing kinds, fire projectiles, primed TNT, lit
    /// minecarts, the water rule). Mods' own registered sources join the same arena in every
    /// case, unscaled by any policy's brightness.
    pub(super) fn gather_dynamic_lights(&mut self, camera: DVec3, alpha: f64) {
        let mut out = std::mem::take(&mut self.dynamic_lights);
        out.clear();
        let mods = self.mod_dynamic_lights();
        let policy = mods.policy.as_ref();
        if policy.is_none() && !self.settings.extras.dynamic_lights && mods.sources.is_empty() {
            self.dynamic_lights = out;
            return;
        }
        if self.luminance.is_none() {
            self.luminance = Some(Luminance::new(self.res.facts(), self.pack.assets.registry));
        }
        let lum = self.luminance.as_ref().expect("built above");
        // The game's own water test, as the frame's camera check
        // makes it: the block at the point is water.
        let in_water = |at: DVec3| {
            let state = self.world.block(
                at.x.floor() as i32,
                at.y.floor() as i32,
                at.z.floor() as i32,
            );
            self.res.facts().get(state).fluid == Fluid::Water
        };
        // The classes' gates: with no policy standing, the built-in's
        // classes (held, dropped, burning) run whole, as they do today.
        let held_on = policy.is_none_or(|p| p.held_items);
        let dropped_on = policy.is_none_or(|p| p.dropped_items);
        let burning_on = policy.is_none_or(|p| p.burning);
        let water_out = policy.is_some_and(|p| p.water_extinguishes);
        // One item's light at `at`: its luminance, or nothing when the
        // policy's water rule puts its open flame out there.
        let hand = |item: Option<&str>, at: DVec3| -> u8 {
            let Some(item) = item else { return 0 };
            if water_out && dynamic_light::is_flame_item(item) && in_water(at) {
                return 0;
            }
            lum.of(item)
        };
        // A fire term at `at`: fire's level while the class stands and
        // the water rule leaves it lit.
        let fire = |burning: bool, at: DVec3| -> u8 {
            if burning && burning_on && !(water_out && in_water(at)) {
                BURNING
            } else {
                0
            }
        };
        let mut gathered: Vec<Source> = Vec::new();
        if policy.is_some() || self.settings.extras.dynamic_lights {
            if let Some(p) = &self.player
                && !p.spectator
            {
                let feet = self
                    .previous_position
                    .lerp(p.position, self.clock.camera_fraction());
                // About where the hands are.
                let at = DVec3::new(feet.x, feet.y + 1.2, feet.z);
                let level = if held_on {
                    hand(self.stack_in(self.selected).map(|s| &*s.item), at).max(hand(
                        self.stack_in(shield::OFFHAND_SLOT).map(|s| &*s.item),
                        at,
                    ))
                } else {
                    0
                }
                .max(fire(p.fire_ticks > 0, at));
                gathered.push(Source { at, level });
            }
            for m in self.mobs.iter().filter(|m| !m.removed) {
                let at = m.previous.lerp(m.position, alpha);
                let at = DVec3::new(at.x, at.y + f64::from(m.height()) * 0.6, at.z);
                let level = if held_on {
                    hand(m.extra.main_hand.as_deref(), at)
                        .max(hand(m.extra.off_hand.as_deref(), at))
                } else {
                    0
                }
                .max(fire(m.fire_ticks > 0, at))
                .max(policy.map_or(0, |p| dynamic_light::glowing_level(&p.glowing, &m.kind.id)));
                if level == 0 {
                    continue;
                }
                gathered.push(Source { at, level });
            }
            // Armor stands carry held items like a mob does (SDK API
            // 28): their own table, apart from `mobs`, read at its
            // render position. A class the policy adds — the built-in
            // gather has no stand loop, and without a policy standing
            // it still has none. Marker stands are not drawn and
            // carry nothing.
            if policy.is_some() && held_on {
                for stand in &self.armor_stands {
                    if stand.marker {
                        continue;
                    }
                    let at = stand.render_position(alpha);
                    let at = DVec3::new(at.x, at.y + 1.2, at.z);
                    let level = hand(stand.equipment[0].as_ref().map(|s| &*s.item), at)
                        .max(hand(stand.equipment[1].as_ref().map(|s| &*s.item), at));
                    if level == 0 {
                        continue;
                    }
                    gathered.push(Source { at, level });
                }
            }
            if dropped_on {
                for item in &self.dropped {
                    let at = item.previous.lerp(item.position, alpha);
                    let at = DVec3::new(at.x, at.y + 0.25, at.z);
                    let level = hand(Some(&item.stack.item), at).max(fire(item.fire_ticks > 0, at));
                    if level == 0 {
                        continue;
                    }
                    gathered.push(Source { at, level });
                }
            }
            // Fire projectiles, primed TNT and lit minecarts (SDK API
            // 28): a policy class over tables the game already keeps,
            // each read at its interpolated position like the dropped
            // items are. Every entry here is a fire-class source, so
            // the water rule puts the whole loop's light out at once.
            if policy.is_some_and(|p| p.projectiles) {
                let fire_source = |at: DVec3| -> Option<Source> {
                    if water_out && in_water(at) {
                        None
                    } else {
                        Some(Source { at, level: BURNING })
                    }
                };
                for p in &self.creatures.projectiles {
                    let is_fire = p.is_fireball()
                        || p.is_spectral_arrow()
                        || (matches!(p.kind, ProjectileKind::Arrow { .. })
                            && p.arrow.as_ref().is_some_and(|a| a.fire_ticks > 0));
                    if !is_fire {
                        continue;
                    }
                    let at = p.previous.lerp(p.position, alpha);
                    if let Some(source) = fire_source(DVec3::new(at.x, at.y, at.z)) {
                        gathered.push(source);
                    }
                }
                for b in &self.dragon_fireballs {
                    let at = b.previous.lerp(b.position, alpha);
                    if let Some(source) = fire_source(DVec3::new(at.x, at.y, at.z)) {
                        gathered.push(source);
                    }
                }
                for r in &self.rockets {
                    let at = r.previous.lerp(r.position, alpha);
                    if let Some(source) = fire_source(DVec3::new(at.x, at.y, at.z)) {
                        gathered.push(source);
                    }
                }
                for t in &self.tnts {
                    let at = t.previous.lerp(t.position, alpha);
                    if let Some(source) = fire_source(DVec3::new(at.x, at.y, at.z)) {
                        gathered.push(source);
                    }
                }
                for c in self
                    .carts
                    .iter()
                    .filter(|c| c.kind == CartKind::Furnace && c.fuel > 0)
                {
                    let at = c.previous.lerp(c.position, alpha);
                    if let Some(source) = fire_source(DVec3::new(at.x, at.y, at.z)) {
                        gathered.push(source);
                    }
                }
            }
        }
        // The policy's brightness scales the gathered side's levels as
        // they enter the arena; a mod's own sources are never scaled —
        // their levels are already their author's word.
        if let Some(p) = policy {
            for source in &mut gathered {
                source.level = dynamic_light::scale_level(source.level, p.brightness);
            }
            // The policy's range narrows the gathered side's reach; a
            // mod's own sources keep the game's 96, applied by the
            // selection below.
            let range = f64::from(p.range);
            gathered.retain(|s| s.at.distance_squared(camera) <= range * range);
        }
        // The mods' own sources: anchors resolved against the drawn
        // world at the gather's own placements. An item's level is the
        // game's table's; a resolution of 0 makes no source.
        let mut sources = gathered;
        for source in &mods.sources {
            let level = match &source.item {
                Some(item) => lum.of(item),
                None => source.level,
            };
            if level == 0 {
                continue;
            }
            let at = match &source.anchor {
                ModLightAnchor::Position(at) => *at,
                ModLightAnchor::SelfPlayer => {
                    let Some(p) = &self.player else { continue };
                    if p.spectator {
                        continue;
                    }
                    let feet = self
                        .previous_position
                        .lerp(p.position, self.clock.camera_fraction());
                    DVec3::new(feet.x, feet.y + 1.2, feet.z)
                }
                ModLightAnchor::Entity(id) => {
                    // The player model answers for its own id, as the
                    // particle anchors resolve it; any other id answers
                    // from the mob table (other players included).
                    let mob = if let Some(m) = &self.player_model
                        && m.id == *id
                    {
                        Some(m)
                    } else {
                        self.mobs.iter().find(|m| m.id == *id && !m.removed)
                    };
                    let Some(m) = mob else { continue };
                    let at = m.previous.lerp(m.position, alpha);
                    DVec3::new(at.x, at.y + f64::from(m.height()) * 0.6, at.z)
                }
            };
            sources.push(Source { at, level });
        }
        // One arena for the game's and the mods' sources together: the
        // policy's count caps the whole of it, its reach the game's 96.
        let max = policy.map_or(dynamic_light::MAX, |p| p.max_lights as usize);
        dynamic_light::select_with(&sources, camera, dynamic_light::REACH, max, &mut out);
        self.dynamic_lights = out;
    }
}
