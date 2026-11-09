//! Prototype probe for world-space drawing (`docs/mods.md`, "World
//! shapes"): each tick it draws a box round the nearest living mob (from
//! `%entity`) and a line from the player's feet to the mob under the
//! crosshair, and clears both when there is nothing to draw.
use horizon_mod_sdk::{
    Mod, Registration,
    entity::{self, Entity},
    game,
    render::{self, WorldBox, WorldLine, WorldPoint, WorldShape},
};

struct DrawProbe;

impl DrawProbe {
    /// The mob's hitbox as a world box: its feet, its width either side,
    /// its height up.
    fn mob_box(m: &Entity) -> WorldShape {
        let half = f64::from(m.width) / 2.0;
        WorldShape::Box(WorldBox {
            min: WorldPoint {
                x: m.x - half,
                y: m.y,
                z: m.z - half,
            },
            max: WorldPoint {
                x: m.x + half,
                y: m.y + f64::from(m.height),
                z: m.z + half,
            },
            color: 0xFF00_FF00,
            width: 2.0,
        })
    }
}

impl Mod for DrawProbe {
    fn init() -> Result<(Self, Registration), String> {
        Ok((DrawProbe, Registration::new().on_tick()))
    }

    fn tick(&mut self, _tick: u64) {
        let Some(player) = game::get_player() else {
            render::set_world(&[]);
            return;
        };
        let mut shapes = Vec::new();
        if let Some(mob) = entity::entities().first() {
            shapes.push(Self::mob_box(mob));
        }
        if let Some(target) = entity::targeted(128.0) {
            shapes.push(WorldShape::Line(WorldLine {
                start: WorldPoint {
                    x: player.x,
                    y: player.y,
                    z: player.z,
                },
                end: WorldPoint {
                    x: target.entity.x,
                    y: target.entity.y,
                    z: target.entity.z,
                },
                color: 0xFFFF_FF00,
                width: 1.0,
            }));
        }
        render::set_world(&shapes);
    }
}

horizon_mod_sdk::export_mod!(DrawProbe);
