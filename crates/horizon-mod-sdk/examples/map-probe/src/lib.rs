//! Prototype probe for map (API 25): on its key it reads the
//! game's map tiles of the 3x3 chunks around the player
//! (`map.tiles`), reports the count in chat, and shows a
//! two-colour checkerboard as its full-screen map
//! (`map.set-map-screen`), centred on the player's block with
//! a marker on it. Its `/survey` command does the same read
//! for the test harness, which cannot press keys, and answers
//! with the count as its feedback.
use horizon_mod_sdk::{
    ChunkPos, MapImage, MapMarker, MapScreen, Mod, Registration, chat, game,
    input::{self, Event},
    map,
};

const KEY: &str = "survey";

struct MapProbe;

/// The read the key and the command both make: the tiles of
/// the 3x3 chunks around the player's chunk, the count
/// reported, and the checkerboard shown at the player's
/// block. The answer is the report line, for the command's
/// feedback; the key path shows it in chat instead.
fn survey() -> Result<String, String> {
    let p = game::get_player().ok_or("no player")?;
    let (bx, bz) = (p.x.floor() as i32, p.z.floor() as i32);
    let (cx, cz) = (bx >> 4, bz >> 4);
    let ask: Vec<ChunkPos> = (-1..=1)
        .flat_map(|dz| {
            (-1..=1).map(move |dx| ChunkPos {
                x: cx + dx,
                z: cz + dz,
            })
        })
        .collect();
    let tiles = map::tiles(&ask);
    let pixels: Vec<u8> = (0..32 * 32)
        .map(|i| (((i % 32) + (i / 32)) % 2) as u8)
        .collect();
    map::set_map_screen(Some(&MapScreen {
        image: MapImage {
            center_x: bx,
            center_z: bz,
            blocks_per_pixel: 1.0,
            width: 32,
            height: 32,
            palette: vec![0xFF3B_6EA5, 0xFFD9_C27A],
            pixels,
        },
        markers: vec![MapMarker {
            x: bx,
            z: bz,
            color: 0xFFFF_3333,
            label: "You".into(),
        }],
    }));
    Ok(format!("{} tiles around {cx} {cz}", tiles.len()))
}

impl Mod for MapProbe {
    fn init() -> Result<(Self, Registration), String> {
        input::register_key(KEY, "Survey the map", "key.keyboard.m")?;
        Ok((MapProbe, Registration::new().on_tick().command("survey")))
    }

    fn tick(&mut self, _tick: u64) {
        for event in input::poll() {
            if let Event::KeyDown(k) = event
                && k == KEY
                && let Ok(line) = survey()
            {
                chat::show(&line);
            }
        }
    }

    fn command(&mut self, _name: &str, _args: &str) -> Result<Option<String>, String> {
        Ok(Some(survey()?))
    }
}

horizon_mod_sdk::export_mod!(MapProbe);
