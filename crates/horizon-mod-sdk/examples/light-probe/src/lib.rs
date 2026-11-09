//! Prototype probe for client light reads (`docs/mods.md`, "Client world"):
//! each tick it reads the light at the player's feet with `world.get-lights`
//! and shows it in chat when it changes, and `/probe-cap` proves a call over
//! the 4096-position cap returns no results. Singleplayer only in v1; on a
//! multiplayer server, and before a world loads, every read is `None`.
use horizon_mod_sdk::{
    Mod, Registration, chat,
    game::{self, Player},
    world::{self, BlockPos},
};

struct LightProbe {
    last: String,
}

fn show(light: &Option<world::LightValue>) -> String {
    match light {
        Some(v) => format!("{}/{}", v.block, v.sky),
        None => "none".into(),
    }
}

impl Mod for LightProbe {
    fn init() -> Result<(Self, Registration), String> {
        let hooks = Registration::new().command("probe-cap").on_tick();
        Ok((LightProbe { last: String::new() }, hooks))
    }

    fn tick(&mut self, _tick: u64) {
        let Some(p) = game::get_player() else {
            return;
        };
        let Player { x, y, z, dimension, .. } = p;
        let (fx, fy, fz) = (x.floor() as i32, y.floor() as i32, z.floor() as i32);
        let positions = vec![
            BlockPos { x: fx, y: fy, z: fz },
            BlockPos { x: fx, y: fy - 1, z: fz },
            BlockPos { x: 10000, y: 64, z: 10000 },
            BlockPos { x: fx, y: 400, z: fz },
        ];
        let read = world::get_lights(&dimension, &positions);
        let line = match read.as_slice() {
            [feet, below, far, high] => format!(
                "probe light feet={} below={} far={} high={}",
                show(&feet.light),
                show(&below.light),
                show(&far.light),
                show(&high.light)
            ),
            _ => "probe light: no read".to_string(),
        };
        if line != self.last {
            self.last = line.clone();
            chat::show(&line);
        }
    }

    fn command(&mut self, name: &str, _args: &str) -> Result<Option<String>, String> {
        if name != "probe-cap" {
            return Err("This command does nothing".into());
        }
        let Some(p) = game::get_player() else {
            return Ok(Some("probe cap: no player".into()));
        };
        let positions: Vec<BlockPos> = (0..4097)
            .map(|i| BlockPos { x: i, y: 64, z: 0 })
            .collect();
        let read = world::get_lights(&p.dimension, &positions);
        Ok(Some(format!("probe cap: {} results", read.len())))
    }
}

horizon_mod_sdk::export_mod!(LightProbe);
