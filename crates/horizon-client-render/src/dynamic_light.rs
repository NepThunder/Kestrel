//! Horizon Extras' dynamic lights (docs/extras.md): a torch, lantern or other light source held by
//! a player or mob, dropped as an item, or an entity on fire lights the blocks and entities
//! around it. Not in vanilla; off by default.
//!
//! Client-only and at render time: the light engine, chunk meshes and saves never see it. Each
//! frame the game picks the [`MAX`] sources nearest the camera; terrain.wgsl and entity.wgsl
//! raise a vertex's block light to `level - distance` (one level per block, as block light falls
//! off) when that is brighter. Off, the list is empty and the shaders skip the loop.
use crate::blocks::BlockFacts;
use glam::DVec3;
use horizon_world::registry::BlockRegistry;
use std::collections::HashMap;

/// Lights the shaders read: `Globals.dynamic_lights` in gfx.rs, terrain.wgsl and entity.wgsl.
pub const MAX: usize = 16;
/// Camera-relative position and radius; RGB intensity in a second vec4.
pub type Colored = [[f32; 4]; 2];

/// Nearest validated native visual lights; never enters gameplay lighting.
pub fn select_colored(
    sources: impl Iterator<Item = ([f64; 3], u32, f32, f32)>,
    camera: DVec3,
) -> Vec<Colored> {
    let mut nearest: Vec<_> = sources.collect();
    nearest.sort_by(|a, b| {
        (DVec3::from(a.0) - camera)
            .length_squared()
            .total_cmp(&(DVec3::from(b.0) - camera).length_squared())
    });
    nearest
        .into_iter()
        .take(MAX)
        .map(|(at, rgb, radius, intensity)| {
            let at = (DVec3::from(at) - camera).as_vec3();
            [
                [at.x, at.y, at.z, radius],
                [
                    ((rgb >> 16) & 255) as f32 / 255.0,
                    ((rgb >> 8) & 255) as f32 / 255.0,
                    (rgb & 255) as f32 / 255.0,
                    intensity,
                ],
            ]
        })
        .collect()
}
/// What an entity on fire gives off, as a lit campfire or fire block does.
pub const BURNING: u8 = 15;
/// Sources farther than this from the camera light nothing it can see well.
pub const REACH: f64 = 96.0;

/// How brightly each item glows when held or dropped.
#[derive(Debug, Default)]
pub struct Luminance(HashMap<String, u8>);

impl Luminance {
    /// A block item glows like its block's default state (`getLightEmission`): torches 14,
    /// lanterns and glowstone 15, a lit redstone torch 7; a furnace's item, unlit, not at all.
    pub fn new(facts: &BlockFacts, registry: &BlockRegistry) -> Self {
        let mut map = HashMap::new();
        for (id, state) in facts.states.iter().enumerate() {
            if state.emission == 0 || *state.item != *state.name {
                continue;
            }
            if registry.default_state(&state.name).map(|d| d.0 as usize) == Some(id) {
                map.insert(state.item.to_string(), state.emission);
            }
        }
        // A bucket of lava glows as lava does.
        map.insert("minecraft:lava_bucket".into(), 15);
        Self(map)
    }

    pub fn of(&self, item: &str) -> u8 {
        self.0.get(item).copied().unwrap_or(0)
    }

    #[cfg(test)]
    fn from_pairs(pairs: &[(&str, u8)]) -> Self {
        Self(pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect())
    }
}

/// One light this frame: where it is (world space) and its level, 1 to 15.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Source {
    pub at: DVec3,
    pub level: u8,
}

/// The [`MAX`] sources nearest `camera` within reach, as the shaders read them: camera-relative
/// position and level. Overwrites `out`.
pub fn select(sources: &[Source], camera: DVec3, out: &mut Vec<[f32; 4]>) {
    select_with(sources, camera, REACH, MAX, out);
}

/// [`select`] at another reach and count: a mod policy (SDK API 28) narrows the arena for the
/// sources the game gathers under it — never widens it; the game clamps the parameters to
/// 8..=96 blocks and 1..=[`MAX`] before they reach here. Overwrites `out`.
pub fn select_with(
    sources: &[Source],
    camera: DVec3,
    reach: f64,
    max: usize,
    out: &mut Vec<[f32; 4]>,
) {
    out.clear();
    let key = |s: &Source| s.at.distance_squared(camera);
    let mut near: Vec<&Source> = sources
        .iter()
        .filter(|s| s.level > 0 && key(s) <= reach * reach)
        .collect();
    near.sort_by(|a, b| key(a).total_cmp(&key(b)));
    out.extend(near.into_iter().take(max).map(|s| {
        let p = (s.at - camera).as_vec3();
        [p.x, p.y, p.z, f32::from(s.level.min(15))]
    }));
}

/// The level a gathered source sheds under a mod policy's brightness (SDK API 28): its own
/// level scaled by the percent (the caller clamps it to 10..=200), rounded, and clamped to
/// the arena's 0..=15. 100 is the level itself.
pub fn scale_level(level: u8, brightness: u32) -> u8 {
    ((u32::from(level) * brightness + 50) / 100).min(15) as u8
}

/// The level a mod policy's glowing table declares for `kind` (SDK API 28): the first
/// matching entry's level, 0 when the kind is not named — a kind the game does not know
/// matches nothing, as a name no policy declares does.
pub fn glowing_level(kinds: &[(String, u8)], kind: &str) -> u8 {
    kinds
        .iter()
        .find(|(named, _)| named == kind)
        .map_or(0, |(_, level)| *level)
}

/// Whether an item burns with an open flame, for a mod policy's water rule (SDK API 28):
/// torches and soul torches go out in water; lanterns, glowstone and the other sealed or
/// solid lights are not in this set and stay lit.
pub fn is_flame_item(item: &str) -> bool {
    matches!(item, "minecraft:torch" | "minecraft:soul_torch")
}

/// The block light a vertex at `pos` (camera-relative) gets from `lights`, 0 to 15: the WGSL
/// `dynamic_block_light` in Rust, for tests.
pub fn level_at(pos: [f32; 3], lights: &[[f32; 4]]) -> f32 {
    lights.iter().fold(0.0f32, |level, l| {
        let d =
            ((pos[0] - l[0]).powi(2) + (pos[1] - l[1]).powi(2) + (pos[2] - l[2]).powi(2)).sqrt();
        level.max(l[3] - d)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colored_sources_are_camera_relative_and_bounded() {
        let lights = select_colored(
            (0..20).map(|x| ([x as f64, 64.0, 0.0], 0xff8040, 5.0, 0.5)),
            DVec3::new(2.0, 64.0, 0.0),
        );
        assert_eq!(lights.len(), MAX);
        assert_eq!(lights[0][0][0], 0.0);
        assert_eq!(lights[0][1], [1.0, 128.0 / 255.0, 64.0 / 255.0, 0.5]);
    }

    #[test]
    fn nearest_sources_win() {
        let camera = DVec3::new(100.0, 64.0, 100.0);
        let mut sources: Vec<Source> = (0..40)
            .map(|i| Source {
                at: camera + DVec3::new(f64::from(i), 0.0, 0.0),
                level: 14,
            })
            .collect();
        sources.push(Source {
            at: camera + DVec3::new(0.5, 0.0, 0.0),
            level: 0,
        });
        sources.push(Source {
            at: camera + DVec3::new(0.0, 0.0, 200.0),
            level: 15,
        });
        let mut out = Vec::new();
        select(&sources, camera, &mut out);
        assert_eq!(out.len(), MAX);
        assert_eq!(out[0], [0.0, 0.0, 0.0, 14.0]);
        assert_eq!(out[MAX - 1][0], (MAX - 1) as f32);
        select(&[], camera, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn light_falls_off_a_level_per_block() {
        let torch = [[0.0, 0.0, 0.0, 14.0]];
        assert_eq!(level_at([0.0, 0.0, 0.0], &torch), 14.0);
        assert_eq!(level_at([3.0, 4.0, 0.0], &torch), 9.0);
        assert_eq!(level_at([20.0, 0.0, 0.0], &torch), 0.0);
        let two = [[0.0, 0.0, 0.0, 14.0], [10.0, 0.0, 0.0, 15.0]];
        assert_eq!(level_at([8.0, 0.0, 0.0], &two), 13.0);
        assert_eq!(level_at([0.0; 3], &[]), 0.0);
    }

    #[test]
    fn item_luminance() {
        let l = Luminance::from_pairs(&[("minecraft:torch", 14)]);
        assert_eq!(l.of("minecraft:torch"), 14);
        assert_eq!(l.of("minecraft:stone"), 0);
    }

    #[test]
    fn select_with_narrows_reach_and_count() {
        let camera = DVec3::new(0.0, 64.0, 0.0);
        let sources: Vec<Source> = (0..20)
            .map(|i| Source {
                at: camera + DVec3::new(f64::from(i) + 1.0, 0.0, 0.0),
                level: 14,
            })
            .collect();
        let mut out = Vec::new();
        // A reach of 8 keeps the eight sources within it; a max of 3
        // keeps the nearest three of those.
        select_with(&sources, camera, 8.0, MAX, &mut out);
        assert_eq!(out.len(), 8);
        select_with(&sources, camera, 8.0, 3, &mut out);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0], [1.0, 0.0, 0.0, 14.0]);
        assert_eq!(out[2], [3.0, 0.0, 0.0, 14.0]);
        // The built-in's parameters are the widest the arena allows.
        select_with(&sources, camera, REACH, MAX, &mut out);
        assert_eq!(out.len(), MAX);
    }

    #[test]
    fn one_arena_for_gathered_and_mod_sources() {
        // The gather and the mods' own sources enter one candidate
        // list (SDK API 28): the nearest sixteen overall win, not
        // sixteen of each — a near mod source beats a far gathered
        // one, and the other way round.
        let camera = DVec3::new(0.0, 64.0, 0.0);
        let mut sources: Vec<Source> = (0..12)
            .map(|i| Source {
                at: camera + DVec3::new(10.0 + f64::from(i), 0.0, 0.0),
                level: 15,
            })
            .collect();
        sources.extend((0..12).map(|i| Source {
            at: camera + DVec3::new(1.0 + f64::from(i), 0.0, 0.0),
            level: 14,
        }));
        let mut out = Vec::new();
        select(&sources, camera, &mut out);
        assert_eq!(out.len(), MAX);
        // The twelve near ones (level 14) all stand; only the four
        // nearest of the far group (level 15) take the rest.
        assert_eq!(out.iter().filter(|l| l[3] == 14.0).count(), 12);
        assert_eq!(out.iter().filter(|l| l[3] == 15.0).count(), 4);
        assert_eq!(out[0], [1.0, 0.0, 0.0, 14.0]);
    }

    #[test]
    fn brightness_scales_gathered_levels() {
        assert_eq!(scale_level(14, 100), 14, "100% is the level itself");
        assert_eq!(scale_level(0, 100), 0);
        assert_eq!(scale_level(10, 200), 15, "200% doubles, clamped at 15");
        assert_eq!(scale_level(7, 200), 14);
        assert_eq!(scale_level(15, 10), 2, "10% floors a level, rounded");
        assert_eq!(scale_level(14, 10), 1);
        assert_eq!(scale_level(5, 50), 3, "halves round to nearest");
    }

    #[test]
    fn glowing_kinds_match_by_name() {
        let kinds = vec![
            ("minecraft:blaze".to_string(), 15),
            ("minecraft:glow_squid".to_string(), 8),
        ];
        assert_eq!(glowing_level(&kinds, "minecraft:blaze"), 15);
        assert_eq!(glowing_level(&kinds, "minecraft:glow_squid"), 8);
        assert_eq!(glowing_level(&kinds, "minecraft:zombie"), 0);
        assert_eq!(glowing_level(&[], "minecraft:blaze"), 0);
    }

    #[test]
    fn flame_items_go_out_in_water() {
        assert!(is_flame_item("minecraft:torch"));
        assert!(is_flame_item("minecraft:soul_torch"));
        assert!(!is_flame_item("minecraft:lantern"));
        assert!(!is_flame_item("minecraft:soul_lantern"));
        assert!(!is_flame_item("minecraft:glowstone"));
        assert!(!is_flame_item("minecraft:stone"));
    }
}
