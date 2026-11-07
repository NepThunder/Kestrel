//! Prototype probe for client world reads (`docs/mods.md`, "Client world"):
//! each tick it reads the blocks at the player's feet with `world.get-blocks`
//! and shows them in chat when they change, and `/probe-set` proves a client
//! mod's `set-blocks` is refused. Singleplayer only in v1; on a multiplayer
//! server, and before a world loads, every read is `None`.
use horizon_mod_sdk::{
    Mod, Registration, chat,
    game::{self, Player},
    world::{self, BlockChange, BlockPos},
};

struct WorldProbe {
    last: String,
}

fn show(state: &Option<String>) -> &str {
    state.as_deref().unwrap_or("none")
}

impl Mod for WorldProbe {
    fn init() -> Result<(Self, Registration), String> {
        let hooks = Registration::new().command("probe-set").on_tick();
        Ok((WorldProbe { last: String::new() }, hooks))
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
        ];
        let read = world::get_blocks(&dimension, &positions);
        let line = match read.as_slice() {
            [feet, below, far] => format!(
                "probe feet={} below={} far={}",
                show(&feet.state),
                show(&below.state),
                show(&far.state)
            ),
            _ => "probe: no read".to_string(),
        };
        if line != self.last {
            self.last = line.clone();
            chat::show(&line);
        }
    }

    fn command(&mut self, name: &str, _args: &str) -> Result<Option<String>, String> {
        if name != "probe-set" {
            return Err("This command does nothing".into());
        }
        let Some(p) = game::get_player() else {
            return Ok(Some("probe set: no player".into()));
        };
        let change = BlockChange {
            x: p.x.floor() as i32,
            y: p.y.floor() as i32,
            z: p.z.floor() as i32,
            state: "minecraft:stone".into(),
        };
        match world::set_blocks(&p.dimension, &[change]) {
            Ok(()) => Ok(Some("probe set: unexpectedly ok".into())),
            Err(e) => Ok(Some(format!("probe set: refused: {e}"))),
        }
    }
}

horizon_mod_sdk::export_mod!(WorldProbe);
