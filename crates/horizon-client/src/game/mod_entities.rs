//! Capability `entity` (docs/mods.md): the living mobs around the player
//! and the mob under the crosshair, copied into the mods' snapshot once
//! per tick, and only while a mod reads them (`Mods::wants_entities`).
//! With no entity mod on, this file does nothing.
//!
//! What the snapshot reads, and why it is the game's own truth:
//!
//! * The list reads `Game.mobs` — the mobs the client holds this tick,
//!   with their positions, velocities and hitboxes as the game itself
//!   simulates and renders them. Other players are mobs too, but v1
//!   keeps them out of the list: their exact positions through walls
//!   would be a player radar.
//! * Health comes from `Mob.health`, which the server synchs as
//!   LivingEntity DATA_HEALTH_ID (index 9) — see the regression tests
//!   in `game/remote_mob_data.rs` and `lan/entity.rs`. In
//!   singleplayer that is the server's truth, so v1 reports it; in
//!   multiplayer it is `None` in v1, until a server's health sync is
//!   measured there too. A player under the crosshair always reads
//!   `None`: another player's health is never the client's to vouch for.
//! * The target reuses the crosshair's own machinery: the mob raycast
//!   of `mob_hit` (one `ray_box` per mob, skipping the ridden mob and
//!   the camera entity) out to 128 blocks, and a block raycast along
//!   the same view — a mob past the first block is hidden, the way a
//!   wall hides it from the crosshair. Only living mobs answer in v1;
//!   the pickable non-living types (boats, armor stands, ...) are v2.
use super::Game;
use horizon_client::{mobs::Mob, text::Text};
use horizon_mods::{
    ENTITY_LIST_MAX, ENTITY_LIST_RANGE, ENTITY_TARGET_MAX, Entity, ModActionTarget, ModBlockTarget,
    ModEntityTarget, ModFace, Target,
};

/// A pick's face normal as capability `actions` names it (API 21).
fn mod_face(face: [i32; 3]) -> ModFace {
    match face {
        [0, -1, 0] => ModFace::Down,
        [0, 1, 0] => ModFace::Up,
        [0, 0, -1] => ModFace::North,
        [0, 0, 1] => ModFace::South,
        [-1, 0, 0] => ModFace::West,
        _ => ModFace::East,
    }
}

#[cfg(feature = "mods")]
impl Game<'_> {
    /// The entity snapshot for the mods' tick: the living mobs within
    /// 128 blocks, nearest first, at most 256, and the mob under the
    /// crosshair. `(Vec::new(), None)` when no running mod reads
    /// capability `entity`, and on the title screen.
    pub(super) fn mod_entities(&self) -> (Vec<Entity>, Option<Target>) {
        if !self.mods.mods.wants_entities() {
            return (Vec::new(), None);
        }
        if self.player.is_none() {
            return (Vec::new(), None);
        }
        // Singleplayer is the window's own integrated server, or the
        // in-frame world (no `remote` at all); a LAN guest and a server
        // join are multiplayer.
        let singleplayer = self.integrated.is_some() || self.remote.is_none();
        (
            self.mod_entity_list(singleplayer),
            self.mod_targeted(singleplayer),
        )
    }

    /// The living mobs within 128 blocks of the player, nearest first,
    /// at most 256. Other players are not listed.
    fn mod_entity_list(&self, singleplayer: bool) -> Vec<Entity> {
        let Some(player) = &self.player else {
            return Vec::new();
        };
        let center = player.position;
        let range = ENTITY_LIST_RANGE * ENTITY_LIST_RANGE;
        let mut mobs: Vec<&Mob> = self
            .mobs
            .iter()
            .filter(|m| !m.removed && m.kind.id != "minecraft:player")
            .filter(|m| m.distance_sqr(center) <= range)
            .collect();
        mobs.sort_by(|a, b| {
            a.distance_sqr(center)
                .partial_cmp(&b.distance_sqr(center))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        mobs.into_iter()
            .take(ENTITY_LIST_MAX)
            .map(|m| self.mod_entity(m, singleplayer))
            .collect()
    }

    /// The nearest pickable living mob along the player's view within
    /// 128 blocks, before any block. A wall between the two hides the
    /// mob; a player under the crosshair may be named, with no health.
    fn mod_targeted(&self, singleplayer: bool) -> Option<Target> {
        let (eye, dir) = self.aim()?;
        let reach = f64::from(ENTITY_TARGET_MAX);
        // The first block along the view, if any within reach: a mob
        // past it is hidden from the crosshair.
        let block_at = self.clip_ray(eye, dir, reach).map(|hit| {
            (0..3)
                .map(|i| (hit.point[i] - eye[i]).powi(2))
                .sum::<f64>()
                .sqrt()
        });
        let mut best: Option<(usize, f64)> = None;
        for (i, m) in self.mobs.iter().enumerate() {
            // As `mob_hit`: the mob the player rides shares its root
            // vehicle and is never the hit, nor is the camera entity.
            if m.removed || Some(m.id) == self.riding || Some(m.id) == self.remote_camera_id() {
                continue;
            }
            if let Some(t) = super::ray_box(eye, dir, reach, m.bounds())
                && block_at.is_none_or(|b| t < b)
                && best.is_none_or(|b| t < b.1)
            {
                best = Some((i, t));
            }
        }
        let (i, distance) = best?;
        Some(Target {
            entity: self.mod_entity(&self.mobs[i], singleplayer),
            distance: distance as f32,
        })
    }

    /// One mob as the WIT spells it. Health is the server's truth in
    /// singleplayer only, and never for another player.
    /// The crosshair's target for capability `actions` (API 21):
    /// what the game's own pick rests on, in its own order — an
    /// entity in front of the block is the target, the block
    /// behind it is not offered. The block carries the face and
    /// the cursor `game.raycast`'s hit lacks, from the pick's own
    /// `Hit`. None when no running mod reads it, on the title
    /// screen, or when nothing is hit.
    pub(super) fn mod_action_target(&self) -> Option<ModActionTarget> {
        if !self.mods.mods.wants_actions() || self.player.is_none() {
            return None;
        }
        let (eye, _) = self.aim()?;
        let block = self.pick();
        let block_distance = block.map(|hit| {
            (0..3)
                .map(|i| (hit.point[i] - eye[i]).powi(2))
                .sum::<f64>()
                .sqrt()
        });
        // The entity the pick meets first: the mob ray and the
        // object ray `remote_use` dispatches on, the nearer wins.
        let mut entity: Option<(u32, String, f64)> = None;
        if let Some((i, distance)) = self.mob_hit() {
            let mob = &self.mobs[i];
            entity = Some((mob.id, mob.kind.id.clone(), distance));
        }
        if let Some(hit) = self.remote_object_hit(false)
            && entity.as_ref().is_none_or(|(_, _, d)| hit.distance < *d)
            && let Some(kind) = self.mod_object_kind(hit.id)
        {
            entity = Some((hit.id as u32, kind, hit.distance));
        }
        if let Some((id, kind, distance)) = entity
            && block_distance.is_none_or(|b| distance < b)
        {
            return Some(ModActionTarget::Entity(ModEntityTarget {
                id,
                kind,
                distance: distance as f32,
            }));
        }
        let hit = block?;
        let state = self.world.block(hit.pos[0], hit.pos[1], hit.pos[2]);
        Some(ModActionTarget::Block(ModBlockTarget {
            block_x: hit.pos[0],
            block_y: hit.pos[1],
            block_z: hit.pos[2],
            block: self.res.facts().get(state).name.to_string(),
            face: mod_face(hit.face),
            cursor_x: (hit.point[0] - f64::from(hit.pos[0])) as f32,
            cursor_y: (hit.point[1] - f64::from(hit.pos[1])) as f32,
            cursor_z: (hit.point[2] - f64::from(hit.pos[2])) as f32,
        }))
    }

    /// The entity type of one of the object ray's hits, by its
    /// server id: the kind `actions.target` names it by. None for
    /// a hit no collection here claims (a dragon part, whose kind
    /// the part alone doesn't say).
    fn mod_object_kind(&self, id: i32) -> Option<String> {
        if self.paintings.iter().any(|p| p.id == id as u32) {
            return Some("minecraft:painting".into());
        }
        if let Some(frame) = self.item_frames.iter().find(|f| f.id == id as u32) {
            return Some(frame.kind().into());
        }
        if self.armor_stands.iter().any(|a| a.id == id as u32) {
            return Some("minecraft:armor_stand".into());
        }
        if let Some(cushion) = self.cushions.iter().find(|c| c.id == id as u32) {
            return Some(cushion.item());
        }
        if let Some(boat) = self.boats.iter().find(|b| b.id == id as u32) {
            return Some(boat.entity_type());
        }
        if let Some(cart) = self.carts.iter().find(|c| c.id == id as u32) {
            return Some(cart.kind.id().into());
        }
        if self.interactions.contains_key(&id) {
            return Some("minecraft:interaction".into());
        }
        if self.crystals.iter().any(|c| c.id as i32 == id) {
            return Some("minecraft:end_crystal".into());
        }
        None
    }

    fn mod_entity(&self, m: &Mob, singleplayer: bool) -> Entity {
        let is_player = m.kind.id == "minecraft:player";
        Entity {
            id: m.id,
            kind: m.kind.id.clone(),
            name: self.mod_entity_name(m, is_player),
            x: m.position.x,
            y: m.position.y,
            z: m.position.z,
            vx: m.velocity.x,
            vy: m.velocity.y,
            vz: m.velocity.z,
            yaw: m.yaw,
            health: (!is_player && singleplayer).then_some(m.health),
            max_health: m.kind.max_health,
            width: m.width(),
            height: m.height(),
            on_ground: m.on_ground,
            custom_name: m.custom_name.clone(),
            baby: m.is_baby(),
        }
    }

    /// The name as the game shows it: the custom name when set, else a
    /// player's profile name, else the type's translated display name.
    fn mod_entity_name(&self, m: &Mob, is_player: bool) -> String {
        if let Some(name) = &m.custom_name {
            return name.clone();
        }
        if is_player && let Some(name) = self.remote.as_ref().and_then(|r| r.player_name(m.id)) {
            return name.to_owned();
        }
        let key = format!("entity.{}", m.kind.id.replace(':', "."));
        Text::translate(&key, &[]).plain()
    }
}
