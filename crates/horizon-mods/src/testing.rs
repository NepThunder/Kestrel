//! A mod run with no game: `cargo horizon-mod test` (docs/mods.md, "Testing a mod").
//!
//! [`TestGame`] loads one `.kmod` from a scratch copy (its `storage` lives and
//! dies with the run) into the same sandbox the game uses, and plays the game's
//! part: the snapshot, ticks, frames, chat lines and commands, and for a
//! server mod the players joining, leaving, chatting and breaking or placing
//! blocks. [`Script`] is a
//! test written in TOML, `tests/<name>.toml` in the mod's folder: steps, each
//! saying what happens and what the mod must have shown by then.
use crate::{
    BlockPos, Clock, Container, Corner, Entity, GameMode, InvStack, ItemModelIndex, MapTileData,
    MapView, ModAction, ModActionTarget, ModArmPose, ModBlockDisplay, ModBlockTarget, ModClickKind,
    ModDragButton, ModEffect, ModEntityPoseState, ModEntityTarget, ModFace, ModFlatDisplay,
    ModHand, ModItemUse, ModMapContext, ModMapLayer, ModPoseSwingAnim, ModPosition,
    ModPositionEvent, ModScreenEvent, ModScreenState, ModSelfState, ModSlotMove, ModTradeList,
    ModTradeOffer, ModTradeStack, ModWheelEvent, Mods, Output, Panel, Player, PlayerInventory,
    ServerPlayer, Snapshot, Status, Target, WorldDrawLine,
};
use serde::Deserialize;
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

/// One mod in the sandbox, driven by a test.
pub struct TestGame {
    mods: Mods,
    dir: PathBuf,
    kmod: PathBuf,
    /// What the mod reads with `game-read`; ticks move the clock as the game does.
    pub player: Option<Player>,
    pub clock: Option<Clock>,
    /// The players on the server, for a server mod, in join order.
    pub players: Vec<ServerPlayer>,
    /// Every name that joined, for their id.
    names: Vec<String>,
    tick: u64,
    /// What the mod reads with capability `inventory`: the player's own
    /// inventory fixture, and the open container's, if one is open. A
    /// script sets them with its `inventory` / `container` steps.
    pub inventory: PlayerInventory,
    pub open_container: Option<Container>,
    /// What the mod reads with capability `entity`: the living mobs
    /// around the player, as the game's snapshot builder lists them
    /// (other players excepted, multiplayer excepted — see
    /// [`TestGame::snapshot`]), and the mob under the crosshair, if
    /// any. A script sets the mobs with its `entities` steps; the
    /// target is worked out from them, the player's aim and the
    /// script's blocks.
    pub entities: Vec<Entity>,
    /// Multiplayer from a script's `multiplayer` step on: `entities()`
    /// reads empty and health reads None, while the crosshair target
    /// still answers.
    pub multiplayer: bool,
    /// The player is dead: the game reads no open container while the
    /// death screen is up, so neither does the sandbox.
    pub dead: bool,
    /// Capability `world`'s Overworld: the blocks a script set or the mod
    /// changed; every other block, and the other dimensions, are air. Every
    /// biome is the dimension's plainest: plains, nether wastes, the end.
    pub world: std::collections::BTreeMap<(i32, i32, i32), String>,
    /// Capability `world`'s Overworld light: the `(block, sky)` a script
    /// set, each 0 to 15; every other position reads `None` — unloaded,
    /// unlit, or outside the build range — as the game reads it.
    pub lights: std::collections::BTreeMap<(i32, i32, i32), (u8, u8)>,
    /// What the mod reads with `game.get-effects` (API 18): the local
    /// player's active effects. A script's mod reads them as the game
    /// filled them; the sandbox starts with none.
    pub effects: Vec<ModEffect>,
    /// What the mod reads with `game.get-self-state` (API 18): the
    /// local player's movement state, mount and toggle options. None
    /// until a test sets it, as on the title screen.
    pub self_state: Option<ModSelfState>,
    /// What the mod reads with `render.can-zoom` (API 19): a zoom
    /// would take effect. False until a test sets it, as outside a
    /// world.
    pub can_zoom: bool,
    /// What the mod reads with `render.get-spyglass-zoom` (API 19):
    /// the spyglass's factor while scoping. None until a test sets
    /// it, as while not scoping.
    pub spyglass_zoom: Option<f32>,
    /// What the mod reads with `input.last-input-ms` (API 19): the
    /// idle clock. 0 until a test sets it, as at the moment of input.
    pub last_input_ms: u64,
    /// What the mod reads with `entity-pose.get-pose-states`
    /// (API 20): the drawn players' pose states, the local player
    /// first. Empty until a test sets them, as on the title
    /// screen; copied into the mod's store at every snapshot, as
    /// the game pushes them every frame.
    pub pose_states: Vec<ModEntityPoseState>,
    /// What the mod reads with `actions.target` (API 21): the
    /// crosshair's target, as the game's snapshot builder picks
    /// it. None until a test sets it, as when nothing is hit;
    /// copied into the snapshot at every step, as the game fills
    /// it beside the raycast hit.
    pub action_target: Option<ModActionTarget>,
    /// What the mod reads with `screen.state` (API 22): the open
    /// screen, as the game's snapshot builder fills it beside
    /// the open container. The `playing` state, nothing hovered,
    /// until a test sets it; copied into the snapshot at every
    /// step.
    pub screen_state: ModScreenState,
    /// What the mod reads with `screen.trades` (API 22): the
    /// open merchant's offers. None until a test sets them, as
    /// while no merchant screen is open; copied into the
    /// snapshot with [`TestGame::screen_state`].
    pub trades: Option<ModTradeList>,
    /// Every action the mod queued, oldest first, in the
    /// `expect.actions` spelling: the log of what the game would
    /// have performed, for a test to read back.
    pub actions: Vec<String>,
    /// Every slot move the mod queued, oldest first, in the
    /// `expect.slot-moves` spelling (capability
    /// `inventory-actions`, API 24): the log of what the game
    /// would have performed, for a test to read back.
    pub slot_moves: Vec<String>,
    /// What the mod reads with capability `map` (API 25): the
    /// tiles the game's cache holds, lent to the mod's calls
    /// for a step. A script sets them with its `tiles` steps.
    pub map_tiles: std::collections::HashMap<(i32, i32), MapTileData>,
    /// What the mod reads with `map.context` (API 25): the
    /// place the player is in. None until a test sets it, as
    /// on the title screen.
    pub map_context: Option<ModMapContext>,
    /// What the mod reads with `map.last-death` (API 25): the
    /// place the player last died. None until a test sets it.
    pub last_death: Option<ModPosition>,
}

/// The world a script's mod reads: [`TestGame::world`] and
/// [`TestGame::lights`], copied for a step.
struct ScriptWorld {
    blocks: std::collections::BTreeMap<(i32, i32, i32), String>,
    lights: std::collections::BTreeMap<(i32, i32, i32), (u8, u8)>,
}

impl crate::WorldView for ScriptWorld {
    fn block(&self, dimension: &str, x: i32, y: i32, z: i32) -> Option<String> {
        let here = (dimension == "minecraft:overworld")
            .then(|| self.blocks.get(&(x, y, z)).cloned())
            .flatten();
        Some(here.unwrap_or_else(|| "minecraft:air".into()))
    }
    fn biome(&self, dimension: &str, _x: i32, _y: i32, _z: i32) -> Option<String> {
        let biome = match dimension {
            "minecraft:overworld" => "minecraft:plains",
            "minecraft:the_nether" => "minecraft:nether_wastes",
            "minecraft:the_end" => "minecraft:the_end",
            _ => return None,
        };
        Some(biome.into())
    }
    fn light(&self, dimension: &str, x: i32, y: i32, z: i32) -> Option<(u8, u8)> {
        // Only the lent Overworld answers, and only inside its build
        // range (-64..320); a position with no fixture light is
        // unloaded or unlit, and reads None like the game's.
        if dimension != "minecraft:overworld" || !(-64..320).contains(&y) {
            return None;
        }
        self.lights.get(&(x, y, z)).copied()
    }
    fn has_dimension(&self, dimension: &str) -> bool {
        matches!(
            dimension,
            "minecraft:overworld" | "minecraft:the_nether" | "minecraft:the_end"
        )
    }
    fn check_state(&self, state: &str) -> Result<(), String> {
        // No registry here: the shape only (the game checks the names).
        let name = state.split('[').next().unwrap_or_default();
        let plain = |t: &str| {
            !t.is_empty()
                && t.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-/".contains(&b))
        };
        let ok = match name.split_once(':') {
            Some((ns, path)) => plain(ns) && plain(path),
            None => plain(name),
        };
        let closed = !state.contains('[') || state.ends_with(']');
        (ok && closed)
            .then_some(())
            .ok_or_else(|| format!("{state} is not a block state"))
    }
}

/// The map a script's mod reads (capability `map`, API 25):
/// [`TestGame::map_tiles`], copied for a step.
struct ScriptMap {
    tiles: std::collections::HashMap<(i32, i32), MapTileData>,
}

impl MapView for ScriptMap {
    fn tile(&self, cx: i32, cz: i32) -> Option<MapTileData> {
        self.tiles.get(&(cx, cz)).cloned()
    }
    fn tile_version(&self, cx: i32, cz: i32) -> Option<u64> {
        self.tiles.get(&(cx, cz)).map(|t| t.version)
    }
}

/// Where a script starts: standing at 0.5 64 0.5 in the Overworld, facing
/// south, full health and food, at the first tick of a new world's first morning.
pub fn default_player() -> Player {
    Player {
        x: 0.5,
        y: 64.0,
        z: 0.5,
        yaw: 0.0,
        pitch: 0.0,
        health: 20.0,
        food: 20,
        dimension: "minecraft:overworld".into(),
    }
}

impl TestGame {
    /// Starts the mod at `kmod`, as the game would with it turned on. Errs
    /// when it doesn't start, with the game's reason.
    pub fn load(kmod: &Path) -> Result<TestGame, String> {
        static RUN: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "horizon-mod-test-{}-{}",
            std::process::id(),
            RUN.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let copy = dir.join(kmod.file_name().unwrap_or("mod.kmod".as_ref()));
        let mut game = TestGame {
            mods: Mods::default(),
            kmod: copy.clone(),
            dir,
            player: Some(default_player()),
            clock: Some(Clock {
                game_time: 0,
                day_time: 0,
            }),
            players: Vec::new(),
            names: Vec::new(),
            tick: 0,
            inventory: PlayerInventory {
                selected: 0,
                saturation: 5.0,
                slots: Vec::new(),
                carried: None,
            },
            open_container: None,
            entities: Vec::new(),
            multiplayer: false,
            dead: false,
            world: Default::default(),
            lights: Default::default(),
            effects: Vec::new(),
            self_state: None,
            can_zoom: false,
            spyglass_zoom: None,
            last_input_ms: 0,
            pose_states: Vec::new(),
            action_target: None,
            // The `playing` state over the trial window's whole
            // GUI (427 x 240, as `snapshot`'s `gui_size`).
            screen_state: ModScreenState {
                id: "playing".into(),
                menu: None,
                title: None,
                rect: (0.0, 0.0, 427.0, 240.0),
                hovered_slot: None,
                debug_open: false,
            },
            trades: None,
            actions: Vec::new(),
            slot_moves: Vec::new(),
            map_tiles: Default::default(),
            map_context: None,
            last_death: None,
        };
        copy_mod(kmod, &copy)?;
        game.start()?;
        Ok(game)
    }

    fn start(&mut self) -> Result<(), String> {
        self.mods = Mods::load(std::slice::from_ref(&self.kmod), |_| true, |_| false);
        match &self.mods.entries()[0].status {
            Status::Failed(reason) => Err(format!("the mod didn't start: {reason}")),
            _ if !self.mods.is_running() => Err("the mod has no code to run".into()),
            _ => Ok(()),
        }
    }

    /// Stops the mod and starts it again, as a rebuild does in the game: its
    /// memory starts over, its `storage` stays.
    pub fn reload(&mut self) -> Result<(), String> {
        self.mods = Mods::default();
        self.start()
    }

    fn snapshot(&mut self) {
        // The list as the game's snapshot builder makes it: other
        // players are not listed, in multiplayer it is empty, and on
        // the title screen there is no player to stand near — and it is
        // nearest first, at most 256.
        let entities: Vec<Entity> = match (&self.player, self.multiplayer) {
            (None, _) | (_, true) => Vec::new(),
            (Some(p), false) => {
                let mut list: Vec<Entity> = self
                    .entities
                    .iter()
                    .filter(|e| e.kind != "minecraft:player")
                    .filter(|e| {
                        (e.x - p.x).powi(2) + (e.y - p.y).powi(2) + (e.z - p.z).powi(2)
                            <= 128.0 * 128.0
                    })
                    .cloned()
                    .collect();
                list.sort_by(|a, b| {
                    let d = |e: &Entity| {
                        (e.x - p.x).powi(2) + (e.y - p.y).powi(2) + (e.z - p.z).powi(2)
                    };
                    d(a).partial_cmp(&d(b)).unwrap_or(std::cmp::Ordering::Equal)
                });
                list.truncate(256);
                list
            }
        };
        self.mods.set_snapshot(Snapshot {
            player: self.player.clone(),
            clock: self.clock,
            players: self.players.clone(),
            // The GUI of a 1280 x 720 window at scale 3, as the trial run's.
            gui_size: (427.0, 240.0),
            inventory: Some(self.inventory.clone()),
            // Dead: no open container, as the game's snapshot builder
            // reads while the death screen is up. A script that revives
            // the player must close the container itself, as respawn does.
            open_container: (!self.dead).then(|| self.open_container.clone()).flatten(),
            entities,
            targeted: self.compute_targeted(),
            effects: self.effects.clone(),
            self_state: self.self_state.clone(),
            can_zoom: self.can_zoom,
            spyglass_zoom: self.spyglass_zoom,
            last_input_ms: self.last_input_ms,
            action_target: self.action_target.clone(),
            screen_state: self.screen_state.clone(),
            trades: self.trades.clone(),
            map_context: self.map_context.clone(),
            last_death: self.last_death.clone(),
            ..Snapshot::default()
        });
        // The game's per-frame pose push (API 20), played by the
        // harness at the same point: the fixture goes into the
        // mod's store whole, before whatever the step does next.
        self.mods.set_pose_states(self.pose_states.clone());
    }

    /// The mob under the crosshair, as the game's snapshot builder finds
    /// it: the nearest fixture mob along the player's view within 128
    /// blocks, before any block of the script's world. Player mobs may
    /// be named, with no health; in multiplayer health reads None.
    fn compute_targeted(&self) -> Option<Target> {
        let player = self.player.as_ref()?;
        let eye = [player.x, player.y + 1.62, player.z];
        // The game's view vector: from the head's yaw and pitch.
        let (yaw, pitch) = (
            f64::from(player.yaw).to_radians(),
            f64::from(player.pitch).to_radians(),
        );
        let dir = [
            -yaw.sin() * pitch.cos(),
            -pitch.sin(),
            yaw.cos() * pitch.cos(),
        ];
        let reach = 128.0;
        // The first non-air block along the view, if any within reach:
        // a mob past it is hidden from the crosshair.
        let mut block_at = f64::INFINITY;
        let mut t = 0.0;
        while t <= reach {
            let at = [
                eye[0] + dir[0] * t,
                eye[1] + dir[1] * t,
                eye[2] + dir[2] * t,
            ];
            let block = [
                at[0].floor() as i32,
                at[1].floor() as i32,
                at[2].floor() as i32,
            ];
            if self
                .world
                .get(&(block[0], block[1], block[2]))
                .is_some_and(|s| s != "minecraft:air")
            {
                block_at = t;
                break;
            }
            t += 0.02;
        }
        let mut best: Option<(usize, f64)> = None;
        for (i, e) in self.entities.iter().enumerate() {
            let half = f64::from(e.width) / 2.0;
            let min = [e.x - half, e.y, e.z - half];
            let max = [e.x + half, e.y + f64::from(e.height), e.z + half];
            let Some(hit) = ray_box(eye, dir, reach, min, max) else {
                continue;
            };
            if hit < block_at && best.is_none_or(|b| hit < b.1) {
                best = Some((i, hit));
            }
        }
        let (i, distance) = best?;
        let mut entity = self.entities[i].clone();
        if self.multiplayer || entity.kind == "minecraft:player" {
            entity.health = None;
        }
        Some(Target {
            entity,
            distance: distance as f32,
        })
    }
    /// Whether the mod is a server mod.
    pub fn server(&self) -> bool {
        self.mods.runs_server()
    }

    /// `n` game ticks; each moves the clock one tick on, as the game does.
    pub fn tick(&mut self, n: u64) {
        for _ in 0..n {
            self.tick += 1;
            if let Some(clock) = &mut self.clock {
                clock.game_time += 1;
                clock.day_time += 1;
            }
            self.snapshot();
            self.mods.tick(self.tick);
            self.mods.server_tick(self.tick);
        }
    }
    /// `n` frames, 1/60 s apart.
    pub fn frames(&mut self, n: u64) {
        self.snapshot();
        for _ in 0..n {
            self.mods.frame(1.0 / 60.0);
        }
    }
    /// A chat line the player receives.
    pub fn chat(&mut self, line: &str) {
        self.snapshot();
        self.mods.chat(line);
    }
    /// `/line` typed by the player (without the slash); false when the mod
    /// owns no such command.
    pub fn command(&mut self, line: &str) -> bool {
        self.snapshot();
        self.mods.command(line)
    }

    /// A player named `name` joins the server, standing where a script starts
    /// in survival. Their id is `00000000-0000-4000-8000-000000000001` for
    /// the first name that joined, `...0002` for the second, and so on.
    pub fn join(&mut self, name: &str) -> ServerPlayer {
        let n = match self.names.iter().position(|n| n == name) {
            Some(i) => i + 1,
            None => {
                self.names.push(name.to_owned());
                self.names.len()
            }
        };
        let p = default_player();
        let player = ServerPlayer {
            id: format!("00000000-0000-4000-8000-{n:012x}"),
            name: name.to_owned(),
            x: p.x,
            y: p.y,
            z: p.z,
            dimension: p.dimension,
            game_mode: GameMode::Survival,
            health: p.health,
        };
        self.players.push(player.clone());
        self.snapshot();
        self.mods.player_joined(&player);
        player
    }
    /// The player named `name` leaves; false when no such player is on.
    pub fn leave(&mut self, name: &str) -> bool {
        let Some(i) = self.players.iter().position(|p| p.name == name) else {
            return false;
        };
        let player = self.players.remove(i);
        self.snapshot();
        self.mods.player_left(&player);
        true
    }
    /// A chat message `player` sends, for a server mod.
    pub fn player_chat(&mut self, player: &ServerPlayer, line: &str) {
        self.snapshot();
        self.mods.player_chat(player, line);
    }
    /// `/line` typed by `player`, for a server mod; false when the mod owns
    /// no such command.
    pub fn player_command(&mut self, player: &ServerPlayer, line: &str) -> bool {
        self.snapshot();
        self.mods.server_command(player, line)
    }
    /// `player` breaks (or places) `state` at `pos`.
    pub fn block(&mut self, player: &ServerPlayer, pos: BlockPos, state: &str, placed: bool) {
        self.snapshot();
        if placed {
            self.mods.block_placed(player, pos, state);
        } else {
            self.mods.block_broken(player, pos, state);
        }
    }

    /// The scratch copy the mod runs from; its `storage` is beside it.
    pub fn kmod(&self) -> &Path {
        &self.kmod
    }
    /// The commands the mod registered.
    pub fn commands(&self) -> Vec<String> {
        let server = self.mods.server_commands().map(|c| c.name.as_str());
        self.mods
            .commands()
            .chain(server)
            .map(str::to_owned)
            .collect()
    }
    pub fn panel(&self) -> Option<&Panel> {
        self.mods.panels().next()
    }
    /// The running mod's world shapes as depth-tested lines (capability
    /// `render`): what the game would draw this frame.
    pub fn world_lines(&self) -> Vec<WorldDrawLine> {
        self.mods.world_lines()
    }
    pub fn status(&self) -> &Status {
        &self.mods.entries()[0].status
    }
    /// What the mod showed since the last call.
    pub fn drain(&mut self) -> Vec<Output> {
        self.mods.drain()
    }
}

impl Drop for TestGame {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A `.kmod` file, or a mod folder (`horizon-mod.toml` + `mod.wasm`).
fn copy_mod(from: &Path, to: &Path) -> Result<(), String> {
    let err = |e: std::io::Error| format!("{}: {e}", from.display());
    if !from.is_dir() {
        return std::fs::copy(from, to).map(drop).map_err(err);
    }
    std::fs::create_dir_all(to).map_err(err)?;
    for entry in std::fs::read_dir(from).map_err(err)? {
        let entry = entry.map_err(err)?;
        if entry.file_type().map_err(err)?.is_file() {
            std::fs::copy(entry.path(), to.join(entry.file_name())).map_err(err)?;
        }
    }
    Ok(())
}

/// A test in TOML: where a client mod's player starts, then steps run in order.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Script {
    #[serde(default)]
    pub player: PlayerSet,
    #[serde(default)]
    pub clock: ClockSet,
    #[serde(default, rename = "step")]
    pub steps: Vec<Step>,
}

/// Fields of the player to change; the others keep their values. A server
/// mod's players have no `yaw`, `pitch` or `food`, and a client mod's player
/// no `game-mode`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct PlayerSet {
    x: Option<f64>,
    y: Option<f64>,
    z: Option<f64>,
    yaw: Option<f32>,
    pitch: Option<f32>,
    health: Option<f32>,
    food: Option<u32>,
    dimension: Option<String>,
    game_mode: Option<Mode>,
}

/// `players.game-mode` as a script spells it.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Mode {
    Survival,
    Creative,
    Adventure,
    Spectator,
}
impl From<Mode> for GameMode {
    fn from(m: Mode) -> GameMode {
        match m {
            Mode::Survival => GameMode::Survival,
            Mode::Creative => GameMode::Creative,
            Mode::Adventure => GameMode::Adventure,
            Mode::Spectator => GameMode::Spectator,
        }
    }
}

/// A block a player breaks or places.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockSet {
    x: i32,
    y: i32,
    z: i32,
    state: String,
}

/// One position's light, as a step sets it (capability `world`).
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct LightSet {
    x: i32,
    y: i32,
    z: i32,
    block: u8,
    sky: u8,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct ClockSet {
    game_time: Option<u64>,
    day_time: Option<u64>,
}

/// One stack of a script's inventory fixture, in the client's internal
/// slot numbering (0-8 hotbar, 9-35 main, 100-108 crafting grid, 200
/// crafting result, 300+ open container, 500-503 armour, 504 offhand).
/// A stack without `count` is one item.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct StackSet {
    slot: u32,
    id: String,
    #[serde(default = "one_stack")]
    count: u32,
    #[serde(default)]
    damage: u32,
    #[serde(default)]
    max_damage: u32,
    #[serde(default)]
    components: String,
}

fn one_stack() -> u32 {
    1
}

impl StackSet {
    fn to_stack(&self) -> InvStack {
        InvStack {
            slot: self.slot,
            id: self.id.clone(),
            count: self.count,
            damage: self.damage,
            max_damage: self.max_damage,
            components: self.components.clone(),
        }
    }
}

/// The player's own inventory for a step (capability `inventory`):
/// replaces the fixture wholesale; a step without it keeps the last one.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct InventorySet {
    #[serde(default)]
    selected: u32,
    /// Full hunger is 5.0; a step without it keeps 5.0.
    #[serde(default = "full_saturation")]
    saturation: f32,
    #[serde(default)]
    slots: Vec<StackSet>,
    carried: Option<StackSet>,
}

fn full_saturation() -> f32 {
    5.0
}

impl InventorySet {
    fn to_inventory(&self) -> PlayerInventory {
        PlayerInventory {
            selected: self.selected,
            saturation: self.saturation,
            slots: self.slots.iter().map(StackSet::to_stack).collect(),
            carried: self.carried.as_ref().map(StackSet::to_stack),
        }
    }
}

/// The open container for a step (capability `inventory`): replaces the
/// fixture; `close-container` clears it instead.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct ContainerSet {
    kind: String,
    #[serde(default)]
    slots: Vec<StackSet>,
    #[serde(default)]
    data: Vec<i32>,
}

impl ContainerSet {
    fn to_container(&self) -> Container {
        Container {
            kind: self.kind.clone(),
            slots: self.slots.iter().map(StackSet::to_stack).collect(),
            data: self.data.clone(),
        }
    }
}

/// One mob of a script's entity fixture (capability `entity`), in the
/// WIT record's shape: the snapshot lists it (players excepted,
/// multiplayer excepted) and the crosshair may target it. A mob without
/// `health` reads full health; `name` defaults to the kind.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct EntitySet {
    id: u32,
    kind: String,
    #[serde(default)]
    name: String,
    x: f64,
    y: f64,
    z: f64,
    #[serde(default)]
    vx: f64,
    #[serde(default)]
    vy: f64,
    #[serde(default)]
    vz: f64,
    #[serde(default)]
    yaw: f32,
    #[serde(default)]
    health: Option<f32>,
    max_health: f32,
    width: f32,
    height: f32,
    #[serde(default = "on_ground_default")]
    on_ground: bool,
    #[serde(default)]
    custom_name: Option<String>,
    #[serde(default)]
    baby: bool,
}

fn on_ground_default() -> bool {
    true
}

/// One of the local player's own effects for a step (API 18), in the
/// WIT record's shape.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct EffectSet {
    id: String,
    #[serde(default)]
    amplifier: u32,
    duration: i32,
    #[serde(default)]
    ambient: bool,
    #[serde(default = "show_default")]
    show_particles: bool,
    #[serde(default = "show_default")]
    show_icon: bool,
}

fn show_default() -> bool {
    true
}

impl EffectSet {
    fn to_effect(&self) -> ModEffect {
        ModEffect {
            id: self.id.clone(),
            amplifier: self.amplifier,
            duration: self.duration,
            ambient: self.ambient,
            show_particles: self.show_particles,
            show_icon: self.show_icon,
        }
    }
}

/// The local player's own state for a step (API 18), in the WIT
/// record's shape: the movement flags, the mount (id, kind, health,
/// max-health) and the two toggle options.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct SelfSet {
    #[serde(default)]
    sprinting: bool,
    #[serde(default)]
    sneaking: bool,
    #[serde(default)]
    flying: bool,
    #[serde(default)]
    gliding: bool,
    mount: Option<MountSet>,
    #[serde(default)]
    sprint_toggled: bool,
    #[serde(default)]
    sneak_toggled: bool,
}

/// A step's mount: the entity id, its kind and its health.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct MountSet {
    id: u32,
    kind: String,
    health: f32,
    max_health: f32,
}

/// One drawn player's pose state for a step (API 20), in the WIT
/// record's shape: the walk and head inputs, the swing, each
/// hand's arm pose (named as the WIT enum spells it), the held
/// items, the item in use and the pose flags.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct PoseSet {
    #[serde(default)]
    id: u32,
    #[serde(default)]
    kind: String,
    #[serde(default, rename = "self")]
    self_player: bool,
    #[serde(default)]
    walk_pos: f32,
    #[serde(default)]
    walk_speed: f32,
    #[serde(default)]
    head_yaw: f32,
    #[serde(default)]
    head_pitch: f32,
    #[serde(default)]
    swing_progress: f32,
    #[serde(default)]
    swing_anim: String,
    #[serde(default)]
    swing_off_hand: bool,
    #[serde(default)]
    main_arm_pose: String,
    #[serde(default)]
    off_arm_pose: String,
    main_hand: Option<String>,
    off_hand: Option<String>,
    using: Option<ItemUseSet>,
    #[serde(default)]
    crouching: bool,
    #[serde(default)]
    riding: bool,
    #[serde(default)]
    swim_amount: f32,
    #[serde(default)]
    gliding: bool,
    #[serde(default)]
    sleeping: bool,
    #[serde(default)]
    sprinting: bool,
    #[serde(default)]
    on_ground: bool,
}

/// A step's item in use (API 20): the hand, the item, the ticks.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct ItemUseSet {
    #[serde(default)]
    off_hand: bool,
    item: String,
    #[serde(default)]
    ticks: u32,
}

impl PoseSet {
    fn to_state(&self) -> ModEntityPoseState {
        let arm = |name: &str| match name {
            "item" => ModArmPose::Item,
            "block" => ModArmPose::Block,
            "bow" => ModArmPose::Bow,
            "crossbow-hold" => ModArmPose::CrossbowHold,
            "crossbow-charge" => ModArmPose::CrossbowCharge,
            "throw-trident" => ModArmPose::ThrowTrident,
            "toot-horn" => ModArmPose::TootHorn,
            "spear" => ModArmPose::Spear,
            "brush" => ModArmPose::Brush,
            _ => ModArmPose::Empty,
        };
        ModEntityPoseState {
            id: self.id,
            kind: if self.kind.is_empty() {
                "minecraft:player".into()
            } else {
                self.kind.clone()
            },
            self_: self.self_player,
            walk_pos: self.walk_pos,
            walk_speed: self.walk_speed,
            head_yaw: self.head_yaw,
            head_pitch: self.head_pitch,
            swing_progress: self.swing_progress,
            swing_anim: match self.swing_anim.as_str() {
                "whack" => ModPoseSwingAnim::Whack,
                "stab" => ModPoseSwingAnim::Stab,
                _ => ModPoseSwingAnim::None,
            },
            swing_off_hand: self.swing_off_hand,
            main_arm_pose: arm(&self.main_arm_pose),
            off_arm_pose: arm(&self.off_arm_pose),
            main_hand: self.main_hand.clone(),
            off_hand: self.off_hand.clone(),
            using: self.using.as_ref().map(|u| ModItemUse {
                off_hand: u.off_hand,
                item: u.item.clone(),
                ticks: u.ticks,
            }),
            crouching: self.crouching,
            riding: self.riding,
            swim_amount: self.swim_amount,
            gliding: self.gliding,
            sleeping: self.sleeping,
            sprinting: self.sprinting,
            on_ground: self.on_ground,
        }
    }
}

/// The crosshair's target for a step (API 21), in the WIT
/// variant's shape: a `block` (its position, state, face and
/// cursor) or an `entity` (its id, kind and distance).
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct ActionTargetSet {
    block: Option<ActionBlockSet>,
    entity: Option<ActionEntitySet>,
}

/// A step's block target (API 21): the position, the state
/// string, the face by its WIT name, and the cursor within the
/// block (each 0 to 1).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct ActionBlockSet {
    #[serde(default)]
    x: i32,
    #[serde(default)]
    y: i32,
    #[serde(default)]
    z: i32,
    #[serde(default)]
    block: String,
    #[serde(default)]
    face: String,
    #[serde(default = "default_cursor")]
    cursor: [f32; 3],
}

fn default_cursor() -> [f32; 3] {
    [0.5, 0.5, 0.5]
}

/// A step's entity target (API 21).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct ActionEntitySet {
    #[serde(default)]
    id: u32,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    distance: f32,
}

impl ActionTargetSet {
    fn to_target(&self) -> Option<ModActionTarget> {
        if let Some(b) = &self.block {
            return Some(ModActionTarget::Block(ModBlockTarget {
                block_x: b.x,
                block_y: b.y,
                block_z: b.z,
                block: if b.block.is_empty() {
                    "minecraft:stone".into()
                } else {
                    b.block.clone()
                },
                face: match b.face.as_str() {
                    "down" => ModFace::Down,
                    "north" => ModFace::North,
                    "south" => ModFace::South,
                    "west" => ModFace::West,
                    "east" => ModFace::East,
                    _ => ModFace::Up,
                },
                cursor_x: b.cursor[0],
                cursor_y: b.cursor[1],
                cursor_z: b.cursor[2],
            }));
        }
        self.entity.as_ref().map(|e| {
            ModActionTarget::Entity(ModEntityTarget {
                id: e.id,
                kind: e.kind.clone(),
                distance: e.distance,
            })
        })
    }
}

/// One queued action as a script's `expect.actions` names it
/// (API 21): the click the game would have performed.
fn action_line(action: &ModAction) -> String {
    match action {
        ModAction::Use => "use".into(),
        ModAction::Attack => "attack".into(),
        ModAction::SelectSlot(slot) => format!("select-slot {slot}"),
        ModAction::UseOnBlock { target, hand } => {
            let face = match target.face {
                ModFace::Down => "down",
                ModFace::Up => "up",
                ModFace::North => "north",
                ModFace::South => "south",
                ModFace::West => "west",
                ModFace::East => "east",
            };
            let hand = match hand {
                ModHand::Main => "main",
                ModHand::Off => "off",
            };
            format!(
                "use-on-block {} {} {} {face} {hand}",
                target.block_x, target.block_y, target.block_z
            )
        }
    }
}

/// One queued slot verb as a script's `expect.slot-moves` names
/// it (API 24): the click or drag the game would have
/// performed. A click is `click <slot|-> <button> <kind>`; a
/// drag is `drag <button> <slot>...`.
fn slot_move_line(verb: &ModSlotMove) -> String {
    match verb {
        ModSlotMove::Click(click) => {
            let kind = match click.kind {
                ModClickKind::Pickup => "pickup",
                ModClickKind::QuickMove => "quick-move",
                ModClickKind::Swap => "swap",
                ModClickKind::Clone => "clone",
                ModClickKind::Throw => "throw",
                ModClickKind::PickupAll => "pickup-all",
            };
            match click.slot {
                Some(slot) => format!("click {slot} {} {kind}", click.button),
                None => format!("click - {} {kind}", click.button),
            }
        }
        ModSlotMove::Drag(drag) => {
            let button = match drag.button {
                ModDragButton::Left => "left",
                ModDragButton::Right => "right",
                ModDragButton::Middle => "middle",
            };
            let slots: Vec<String> = drag.slots.iter().map(u32::to_string).collect();
            format!("drag {button} {}", slots.join(" "))
        }
    }
}

/// One wheel turn over the open screen for a step (API 24), in
/// the WIT record's shape: the slot the pointer rested on
/// (absent: over no slot) and the whole steps turned.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct WheelSet {
    slot: Option<u32>,
    steps: i32,
}

/// The open screen for a step (API 22), in the WIT record's
/// shape: its id, the open menu's type and title, the panel's
/// rect, the hovered slot and the debug overlay's state, and
/// the merchant's offers when the screen is one.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct ScreenSet {
    #[serde(default = "default_screen_id")]
    id: String,
    menu: Option<String>,
    title: Option<String>,
    #[serde(default = "default_screen_rect")]
    rect: [f32; 4],
    hovered_slot: Option<u32>,
    #[serde(default)]
    debug_open: bool,
    trades: Option<TradeListSet>,
}

fn default_screen_id() -> String {
    "playing".into()
}

fn default_screen_rect() -> [f32; 4] {
    [0.0, 0.0, 427.0, 240.0]
}

/// A step's merchant offers (API 22).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct TradeListSet {
    selected: Option<u32>,
    #[serde(default)]
    offers: Vec<TradeOfferSet>,
}

/// One of a step's merchant offers (API 22): the stacks it
/// names at the counts the screen charges, its uses and
/// whether it reads as tradeable.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct TradeOfferSet {
    cost_a: TradeStackSet,
    cost_b: Option<TradeStackSet>,
    result: TradeStackSet,
    #[serde(default)]
    uses: u32,
    #[serde(default)]
    max_uses: u32,
    #[serde(default)]
    available: bool,
}

/// A stack a step's trade names (API 22).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct TradeStackSet {
    #[serde(default)]
    item: String,
    #[serde(default)]
    count: u32,
    #[serde(default)]
    components: String,
}

impl TradeStackSet {
    fn to_stack(&self) -> ModTradeStack {
        ModTradeStack {
            item: if self.item.is_empty() {
                "minecraft:air".into()
            } else {
                self.item.clone()
            },
            count: self.count,
            components: self.components.clone(),
        }
    }
}

impl ScreenSet {
    fn to_state(&self) -> ModScreenState {
        ModScreenState {
            id: self.id.clone(),
            menu: self.menu.clone(),
            title: self.title.clone(),
            rect: (self.rect[0], self.rect[1], self.rect[2], self.rect[3]),
            hovered_slot: self.hovered_slot,
            debug_open: self.debug_open,
        }
    }
    fn to_trades(&self) -> Option<ModTradeList> {
        self.trades.as_ref().map(|t| ModTradeList {
            offers: t
                .offers
                .iter()
                .map(|o| ModTradeOffer {
                    cost_a: o.cost_a.to_stack(),
                    cost_b: o.cost_b.as_ref().map(TradeStackSet::to_stack),
                    result: o.result.to_stack(),
                    uses: o.uses,
                    max_uses: o.max_uses,
                    available: o.available,
                })
                .collect(),
            selected: t.selected,
        })
    }
}

/// One tile of a step (capability `map`, API 25), in the
/// WIT record's shape: its chunk, its layer (`surface`, or
/// `cave` with its `cut`), its version, and its three arrays
/// — each short array zero-padded to the tile's 256, as a
/// fixture names only the columns it cares about.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct TileSet {
    cx: i32,
    cz: i32,
    #[serde(default = "default_tile_layer")]
    layer: String,
    cut: Option<i32>,
    #[serde(default = "default_tile_version")]
    version: u64,
    #[serde(default)]
    colors: Vec<u8>,
    #[serde(default)]
    heights: Vec<i16>,
    #[serde(default)]
    depths: Vec<u8>,
}

fn default_tile_layer() -> String {
    "surface".into()
}

fn default_tile_version() -> u64 {
    1
}

impl TileSet {
    fn to_tile(&self) -> MapTileData {
        let pad = |v: &[u8]| {
            let mut out = [0u8; 256];
            out[..v.len().min(256)].copy_from_slice(&v[..v.len().min(256)]);
            out
        };
        let pad_h = |v: &[i16]| {
            let mut out = [0i16; 256];
            out[..v.len().min(256)].copy_from_slice(&v[..v.len().min(256)]);
            out
        };
        let (layer, cut_y) = match self.layer.as_str() {
            "cave" => (ModMapLayer::Cave, self.cut),
            _ => (ModMapLayer::Surface, None),
        };
        MapTileData {
            layer,
            cut_y,
            version: self.version,
            colors: pad(&self.colors),
            heights: pad_h(&self.heights),
            depths: pad(&self.depths),
        }
    }
}

/// The place a step's player is in (capability `map`, API
/// 25), in the `map-context` record's shape.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct ContextSet {
    dimension: String,
    world: String,
}

/// A place in a step (capability `map`, API 25), in the
/// `position` record's shape: a last death to read, or an
/// event to push (`died-at` / `respawned-at`).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct PositionSet {
    dimension: String,
    x: i32,
    y: i32,
    z: i32,
}

impl PositionSet {
    fn to_position(&self) -> ModPosition {
        ModPosition {
            dimension: self.dimension.clone(),
            x: self.x,
            y: self.y,
            z: self.z,
        }
    }
}

impl SelfSet {
    fn to_self_state(&self) -> ModSelfState {
        ModSelfState {
            sprinting: self.sprinting,
            sneaking: self.sneaking,
            flying: self.flying,
            gliding: self.gliding,
            mount: self.mount.as_ref().map(|m| crate::ModMount {
                id: m.id,
                kind: m.kind.clone(),
                health: m.health,
                max_health: m.max_health,
            }),
            sprint_toggled: self.sprint_toggled,
            sneak_toggled: self.sneak_toggled,
        }
    }
}

impl EntitySet {
    fn to_entity(&self) -> Entity {
        Entity {
            id: self.id,
            kind: self.kind.clone(),
            name: if self.name.is_empty() {
                self.kind.clone()
            } else {
                self.name.clone()
            },
            x: self.x,
            y: self.y,
            z: self.z,
            vx: self.vx,
            vy: self.vy,
            vz: self.vz,
            yaw: self.yaw,
            health: self.health.or(Some(self.max_health)),
            max_health: self.max_health,
            width: self.width,
            height: self.height,
            on_ground: self.on_ground,
            custom_name: self.custom_name.clone(),
            baby: self.baby,
        }
    }
}

/// One step. Its parts happen in this order: `reload`, `title-screen`,
/// `blocks`, `lights`, `join`, `player`, `clock`, `inventory`, `container`,
/// `close-container`, `dead`, `entities`, `effects`, `self`, `can-zoom`,
/// `spyglass`, `idle-ms`, `poses`, `target`, `screen`, `tiles`,
/// `map-context`, `last-death`, `press-button`,
/// `middle-click`, `middle-click-none`, `wheel`, `died-at`, `respawned-at`,
/// `multiplayer`, `ticks`,
/// `frames`, `chat`, `command`, `break`, `place`, `leave`; then the blocks the mod set are made, and `expect`
/// is checked against what the mod showed and changed during the step.
/// For a server mod, `player`, `chat`, `command`, `break` and `place` are
/// the doing of the player named by `as`, or of the first player on.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Step {
    #[serde(default)]
    reload: bool,
    /// Leave the world: the mod reads no player and no clock until a step sets them.
    #[serde(default)]
    title_screen: bool,
    player: Option<PlayerSet>,
    clock: Option<ClockSet>,
    /// The player's own inventory (capability `inventory`).
    inventory: Option<InventorySet>,
    /// The open container (capability `inventory`).
    container: Option<ContainerSet>,
    /// Close the open container (capability `inventory`).
    #[serde(default)]
    close_container: bool,
    /// The player dies (capability `inventory`): the open container
    /// reads as closed from this step on.
    #[serde(default)]
    dead: bool,
    /// The living mobs around the player (capability `entity`):
    /// replaces the fixture wholesale; a step without it keeps the
    /// last one.
    entities: Option<Vec<EntitySet>>,
    /// The local player's own active effects (API 18): replaces the
    /// fixture wholesale; a step without it keeps the last one.
    effects: Option<Vec<EffectSet>>,
    /// The local player's own state (API 18): replaces the fixture
    /// wholesale; a step without it keeps the last one.
    self_state: Option<SelfSet>,
    /// Whether a mod's zoom would take effect (API 19): replaces the
    /// fixture; a step without it keeps the last one.
    can_zoom: Option<bool>,
    /// The spyglass's zoom factor while scoping (API 19): replaces
    /// the fixture; a step without it keeps the last one. (A script
    /// cannot clear it back to none — no probe needs to.)
    spyglass: Option<f32>,
    /// Milliseconds since the player's last input (API 19): replaces
    /// the fixture; a step without it keeps the last one.
    idle_ms: Option<u64>,
    /// The drawn players' pose states (API 20): replaces the
    /// fixture wholesale; a step without it keeps the last one.
    poses: Option<Vec<PoseSet>>,
    /// The crosshair's target (API 21): replaces the fixture; a
    /// step without it keeps the last one. (A script cannot clear
    /// it back to none — no probe needs to.)
    target: Option<ActionTargetSet>,
    /// The open screen and the merchant's offers (API 22):
    /// replaces both fixtures wholesale; a step without it keeps
    /// the last ones. (A script cannot clear the offers back to
    /// none — no probe needs to.)
    screen: Option<ScreenSet>,
    /// The game's map tiles (capability `map`, API 25):
    /// replaces the fixture wholesale; a step without it keeps
    /// the last one.
    tiles: Option<Vec<TileSet>>,
    /// The place the player is in (capability `map`, API 25):
    /// replaces the fixture; a step without it keeps the last
    /// one.
    map_context: Option<ContextSet>,
    /// Where the player last died (capability `map`, API 25):
    /// replaces the fixture; a step without it keeps the last
    /// one.
    last_death: Option<PositionSet>,
    /// A button the mod declared is pressed (API 22): its id,
    /// queued for the mod's `screen.poll` before the step's
    /// ticks, as the game queues a real press.
    press_button: Option<u32>,
    /// The middle button goes down over this slot (API 22):
    /// queued for the mod's `screen.poll` before the step's
    /// ticks, as the game reports a real press.
    middle_click: Option<u32>,
    /// The middle button goes down over no slot (API 22).
    #[serde(default)]
    middle_click_none: bool,
    /// The wheel turns over the open screen (API 24): pushed
    /// to the mod's `inventory-actions.poll-wheel` queue before
    /// the step's ticks, as the game reports a real turn.
    #[serde(default)]
    wheel: Vec<WheelSet>,
    /// The player dies here (capability `map`, API 25): a
    /// `died` event at this place is pushed to the mod's
    /// `map.poll-events` queue before the step's ticks, as the
    /// game pushes it when the death screen opens.
    died_at: Option<PositionSet>,
    /// The player comes back here (capability `map`, API 25):
    /// a `respawned` event, as `died-at` pushes its own.
    respawned_at: Option<PositionSet>,
    /// Multiplayer from this step on (capability `entity`): the list
    /// reads empty and health reads None, while the crosshair target
    /// still answers.
    #[serde(default)]
    multiplayer: bool,
    #[serde(default)]
    ticks: u64,
    #[serde(default)]
    frames: u64,
    chat: Option<String>,
    command: Option<String>,
    /// Server mods: a player joins, leaves, breaks or places a block.
    join: Option<String>,
    leave: Option<String>,
    #[serde(rename = "as")]
    actor: Option<String>,
    #[serde(rename = "break")]
    broken: Option<BlockSet>,
    place: Option<BlockSet>,
    /// Blocks of the Overworld before the rest of the step (capability
    /// `world`); every other block is air.
    #[serde(default)]
    blocks: Vec<BlockSet>,
    /// Light of the Overworld before the rest of the step (capability
    /// `world`); every other position reads `None`.
    #[serde(default)]
    lights: Vec<LightSet>,
    #[serde(default)]
    expect: Expect,
}

/// What a step checks; a part left out is not checked.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Expect {
    /// The commands the mod owns.
    commands: Option<Vec<String>>,
    /// Every chat line shown during the step, in order.
    chat: Option<Vec<String>>,
    /// The command's feedback, and its red error.
    feedback: Option<String>,
    error: Option<String>,
    /// Server mods: every `players.tell` as `"Name: text"`, and every
    /// `players.broadcast`, in order.
    told: Option<Vec<String>>,
    broadcast: Option<Vec<String>>,
    /// The panel's lines (`[]`: no panel) and its corner.
    panel: Option<Vec<String>>,
    corner: Option<String>,
    /// The mod's world shapes as depth-tested lines (`render.set-world`),
    /// each as (start x, y, z, end x, y, z, colour, width): a box is its
    /// twelve edges, in `CuboidGizmo.emit`'s order.
    world_lines: Option<Vec<(f64, f64, f64, f64, f64, f64, u32, f32)>>,
    /// Blocks of the Overworld after the step, the mod's changes made
    /// (capability `world`).
    blocks: Option<Vec<BlockSet>>,
    /// The actions the mod queued during the step, in order, in
    /// the spelling `action_line` gives them (capability
    /// `actions`, API 21).
    actions: Option<Vec<String>>,
    /// The trade indices the mod chose during the step, in
    /// order (capability `screen`, API 22).
    select_trades: Option<Vec<u32>>,
    /// The slot moves the mod queued during the step, in
    /// order, in the spelling `slot_move_line` gives them
    /// (capability `inventory-actions`, API 24).
    slot_moves: Option<Vec<String>>,
    /// The running mods' model rules after the step, in
    /// evaluation order, each as `ItemModelIndex`'s rule
    /// spelling names it (capability `render`'s `item-model`,
    /// API 23).
    item_rules: Option<Vec<String>>,
    /// The running mods' slot marks after the step, in load
    /// order, each as `ItemDecoration`'s spelling names it
    /// (capability `render`'s `item-model`, API 23).
    decorations: Option<Vec<String>>,
    /// How dropped items display after the step, as
    /// `flat <..> blocks <..> shadows <..>` (capability
    /// `render`'s `item-model`, API 23).
    dropped_display: Option<String>,
    /// The mod's map screen after the step (capability `map`,
    /// API 25): `"none"`, or the stored image spelled
    /// `"{width}x{height} scale {bpp} at {cx} {cz} markers {n}"`.
    map_screen: Option<String>,
    /// The mod's minimap image after the step (capability
    /// `map`, API 25): `"none"`, or the stored image spelled
    /// `"{corner} {size} {width}x{height}"`.
    minimap_image: Option<String>,
    /// The mod is turned off during this step, for a reason holding this text.
    /// Without it, a mod that stops fails the test.
    stopped: Option<String>,
}

impl Script {
    pub fn parse(text: &str) -> Result<Script, String> {
        toml::from_str(text).map_err(|e| e.to_string())
    }

    /// Runs every step on `game`; the error names the first step that failed.
    pub fn run(&self, game: &mut TestGame) -> Result<(), String> {
        if let Some(p) = &mut game.player {
            self.player.apply(p);
        }
        if let Some(c) = &mut game.clock {
            self.clock.apply(c);
        }
        for (i, step) in self.steps.iter().enumerate() {
            step.run(game)
                .map_err(|e| format!("step {} ({}): {e}", i + 1, step.describe()))?;
        }
        Ok(())
    }
}

impl PlayerSet {
    fn apply(&self, p: &mut Player) {
        let set = |to: &mut f64, v: Option<f64>| *to = v.unwrap_or(*to);
        set(&mut p.x, self.x);
        set(&mut p.y, self.y);
        set(&mut p.z, self.z);
        p.yaw = self.yaw.unwrap_or(p.yaw);
        p.pitch = self.pitch.unwrap_or(p.pitch);
        p.health = self.health.unwrap_or(p.health);
        p.food = self.food.unwrap_or(p.food);
        if let Some(d) = &self.dimension {
            p.dimension.clone_from(d);
        }
    }
    fn apply_server(&self, p: &mut ServerPlayer) {
        let set = |to: &mut f64, v: Option<f64>| *to = v.unwrap_or(*to);
        set(&mut p.x, self.x);
        set(&mut p.y, self.y);
        set(&mut p.z, self.z);
        p.health = self.health.unwrap_or(p.health);
        p.game_mode = self.game_mode.map_or(p.game_mode, GameMode::from);
        if let Some(d) = &self.dimension {
            p.dimension.clone_from(d);
        }
    }
}

impl ClockSet {
    fn apply(&self, c: &mut Clock) {
        c.game_time = self.game_time.unwrap_or(c.game_time);
        c.day_time = self.day_time.unwrap_or(c.day_time);
    }
}

impl Step {
    fn describe(&self) -> String {
        let mut parts = Vec::new();
        if self.reload {
            parts.push("reload".to_string());
        }
        let count = |n: u64, what: &str| format!("{n} {what}{}", if n == 1 { "" } else { "s" });
        if self.inventory.is_some() {
            parts.push("inventory".to_string());
        }
        if self.container.is_some() {
            parts.push("container".to_string());
        }
        if self.close_container {
            parts.push("close container".to_string());
        }
        if self.dead {
            parts.push("dies".to_string());
        }
        if self.entities.is_some() {
            parts.push("entities".to_string());
        }
        if self.effects.is_some() {
            parts.push("effects".to_string());
        }
        if self.self_state.is_some() {
            parts.push("self".to_string());
        }
        if self.can_zoom.is_some() {
            parts.push("can-zoom".to_string());
        }
        if self.spyglass.is_some() {
            parts.push("spyglass".to_string());
        }
        if self.idle_ms.is_some() {
            parts.push("idle-ms".to_string());
        }
        if self.poses.is_some() {
            parts.push("poses".to_string());
        }
        if self.target.is_some() {
            parts.push("target".to_string());
        }
        if self.screen.is_some() {
            parts.push("screen".to_string());
        }
        if self.tiles.is_some() {
            parts.push("tiles".to_string());
        }
        if self.map_context.is_some() {
            parts.push("map-context".to_string());
        }
        if self.last_death.is_some() {
            parts.push("last-death".to_string());
        }
        if let Some(id) = self.press_button {
            parts.push(format!("press button {id}"));
        }
        if self.middle_click.is_some() || self.middle_click_none {
            parts.push("middle click".to_string());
        }
        if !self.wheel.is_empty() {
            parts.push(count(self.wheel.len() as u64, "wheel turn"));
        }
        if self.died_at.is_some() {
            parts.push("died".to_string());
        }
        if self.respawned_at.is_some() {
            parts.push("respawned".to_string());
        }
        if self.multiplayer {
            parts.push("multiplayer".to_string());
        }
        if self.ticks > 0 {
            parts.push(count(self.ticks, "tick"));
        }
        if self.frames > 0 {
            parts.push(count(self.frames, "frame"));
        }
        if let Some(line) = &self.chat {
            parts.push(format!("chat {line:?}"));
        }
        if let Some(line) = &self.command {
            parts.push(format!("/{line}"));
        }
        if let Some(name) = &self.join {
            parts.insert(0, format!("{name} joins"));
        }
        for (what, b) in [("breaks", &self.broken), ("places", &self.place)] {
            if let Some(b) = b {
                parts.push(format!("{what} {} at {} {} {}", b.state, b.x, b.y, b.z));
            }
        }
        if let Some(name) = &self.leave {
            parts.push(format!("{name} leaves"));
        }
        if parts.is_empty() {
            parts.push("checks".into());
        }
        parts.join(", ")
    }

    fn run(&self, game: &mut TestGame) -> Result<(), String> {
        if self.reload {
            game.reload()?;
        }
        if self.title_screen {
            game.player = None;
            game.clock = None;
        }
        for b in &self.blocks {
            game.world.insert((b.x, b.y, b.z), b.state.clone());
        }
        for l in &self.lights {
            game.lights.insert((l.x, l.y, l.z), (l.block, l.sky));
        }
        game.mods.lend_world(Box::new(ScriptWorld {
            blocks: game.world.clone(),
            lights: game.lights.clone(),
        }));
        game.mods.lend_map(Box::new(ScriptMap {
            tiles: game.map_tiles.clone(),
        }));
        if let Some(name) = &self.join {
            game.join(name);
        }
        let server = game.server();
        let actor = |game: &TestGame| match &self.actor {
            Some(name) => game
                .players
                .iter()
                .position(|p| p.name == *name)
                .ok_or_else(|| format!("{name} is not on the server")),
            None => (!game.players.is_empty())
                .then_some(0)
                .ok_or_else(|| "no player is on the server: add join = \"Name\"".to_string()),
        };
        let acts = self.player.is_some()
            || self.chat.is_some()
            || self.command.is_some()
            || self.broken.is_some()
            || self.place.is_some();
        let actor = if server && acts {
            Some(actor(game)?)
        } else {
            None
        };
        if let Some(set) = &self.player {
            match actor {
                Some(i) => set.apply_server(&mut game.players[i]),
                None => set.apply(game.player.get_or_insert_with(default_player)),
            }
        }
        if let Some(set) = &self.clock {
            set.apply(game.clock.get_or_insert(Clock {
                game_time: 0,
                day_time: 0,
            }));
        }
        if let Some(set) = &self.inventory {
            game.inventory = set.to_inventory();
        }
        if self.close_container {
            game.open_container = None;
        } else if let Some(set) = &self.container {
            game.open_container = Some(set.to_container());
        }
        if self.dead {
            game.dead = true;
        }
        if let Some(set) = &self.entities {
            game.entities = set.iter().map(EntitySet::to_entity).collect();
        }
        if let Some(set) = &self.effects {
            game.effects = set.iter().map(EffectSet::to_effect).collect();
        }
        if let Some(set) = &self.self_state {
            game.self_state = Some(set.to_self_state());
        }
        if let Some(can) = self.can_zoom {
            game.can_zoom = can;
        }
        if let Some(factor) = self.spyglass {
            game.spyglass_zoom = Some(factor);
        }
        if let Some(ms) = self.idle_ms {
            game.last_input_ms = ms;
        }
        if let Some(set) = &self.poses {
            game.pose_states = set.iter().map(PoseSet::to_state).collect();
        }
        if let Some(set) = &self.target {
            game.action_target = set.to_target();
        }
        if let Some(set) = &self.screen {
            game.screen_state = set.to_state();
            game.trades = set.to_trades();
        }
        if let Some(set) = &self.tiles {
            game.map_tiles = set.iter().map(|t| ((t.cx, t.cz), t.to_tile())).collect();
            // The step's calls read this step's tiles: re-lend.
            game.mods.take_map();
            game.mods.lend_map(Box::new(ScriptMap {
                tiles: game.map_tiles.clone(),
            }));
        }
        if let Some(set) = &self.map_context {
            game.map_context = Some(ModMapContext {
                dimension: set.dimension.clone(),
                world: set.world.clone(),
            });
        }
        if let Some(set) = &self.last_death {
            game.last_death = Some(set.to_position());
        }
        if let Some(id) = self.press_button {
            game.mods.screen_event(0, ModScreenEvent::Button(id));
        }
        if let Some(slot) = self.middle_click {
            game.mods.screen_middle_click(Some(slot));
        }
        if self.middle_click_none {
            game.mods.screen_middle_click(None);
        }
        for turn in &self.wheel {
            game.mods.push_wheel(ModWheelEvent {
                slot: turn.slot,
                steps: turn.steps,
            });
        }
        if let Some(at) = &self.died_at {
            game.mods
                .push_position_event(ModPositionEvent::Died(at.to_position()));
        }
        if let Some(at) = &self.respawned_at {
            game.mods
                .push_position_event(ModPositionEvent::Respawned(at.to_position()));
        }
        if self.multiplayer {
            game.multiplayer = true;
        }
        game.tick(self.ticks);
        game.frames(self.frames);
        let who = actor.map(|i| game.players[i].clone());
        if let Some(line) = &self.chat {
            match &who {
                Some(p) => game.player_chat(p, line),
                None => game.chat(line),
            }
        }
        let owned = self.command.as_ref().map(|line| match &who {
            Some(p) => game.player_command(p, line),
            None => game.command(line),
        });
        for (b, placed) in [(&self.broken, false), (&self.place, true)] {
            if let (Some(b), Some(p)) = (b, &who) {
                let pos = BlockPos {
                    x: b.x,
                    y: b.y,
                    z: b.z,
                };
                game.block(p, pos, &b.state, placed);
            }
        }
        if let Some(name) = &self.leave
            && !game.leave(name)
        {
            return Err(format!("{name} is not on the server"));
        }
        game.mods.take_world();
        game.mods.take_map();
        let out = game.drain();
        for o in &out {
            if let Output::SetBlocks {
                dimension, blocks, ..
            } = o
                && dimension == "minecraft:overworld"
            {
                for b in blocks {
                    game.world.insert((b.x, b.y, b.z), b.state.clone());
                }
            }
            if let Output::Action { action, .. } = o {
                game.actions.push(action_line(action));
            }
            if let Output::SlotClick { click, .. } = o {
                game.slot_moves
                    .push(slot_move_line(&ModSlotMove::Click(click.clone())));
            }
            if let Output::SlotDrag { drag, .. } = o {
                game.slot_moves
                    .push(slot_move_line(&ModSlotMove::Drag(drag.clone())));
            }
        }
        let stopped = out.iter().find_map(|o| match o {
            Output::Stopped { reason, .. } => Some(reason.as_str()),
            _ => None,
        });
        let e = &self.expect;
        match (stopped, &e.stopped) {
            (Some(reason), Some(want)) if !reason.contains(want.as_str()) => {
                return Err(format!("the mod stopped ({reason}), expected {want:?}"));
            }
            (Some(_), Some(_)) => return Ok(()),
            (Some(reason), None) => return Err(format!("the mod stopped: {reason}")),
            (None, Some(want)) => {
                return Err(format!(
                    "the mod kept running, expected it to stop ({want:?})"
                ));
            }
            (None, None) => {}
        }
        if owned == Some(false) {
            let line = self.command.as_deref().unwrap_or_default();
            let name = line.split(' ').next().unwrap_or_default();
            return Err(format!("the mod owns no command /{name}"));
        }
        if let Some(want) = &e.commands {
            same("commands", want, &game.commands())?;
        }
        if let Some(want) = &e.chat {
            let got: Vec<String> = out
                .iter()
                .filter_map(|o| match o {
                    Output::Chat { text, .. } => Some(text.clone()),
                    _ => None,
                })
                .collect();
            same("chat", want, &got)?;
        }
        if let Some(want) = &e.actions {
            let got: Vec<String> = out
                .iter()
                .filter_map(|o| match o {
                    Output::Action { action, .. } => Some(action_line(action)),
                    _ => None,
                })
                .collect();
            same("actions", want, &got)?;
        }
        if let Some(want) = &e.select_trades {
            let got: Vec<u32> = out
                .iter()
                .filter_map(|o| match o {
                    Output::SelectTrade { index, .. } => Some(*index),
                    _ => None,
                })
                .collect();
            same("select-trades", want, &got)?;
        }
        if let Some(want) = &e.slot_moves {
            let got: Vec<String> = out
                .iter()
                .filter_map(|o| match o {
                    Output::SlotClick { click, .. } => {
                        Some(slot_move_line(&ModSlotMove::Click(click.clone())))
                    }
                    Output::SlotDrag { drag, .. } => {
                        Some(slot_move_line(&ModSlotMove::Drag(drag.clone())))
                    }
                    _ => None,
                })
                .collect();
            same("slot-moves", want, &got)?;
        }
        if let Some(want) = &e.item_rules {
            let index = game.mods.item_model_index();
            let got: Vec<String> = index.rules().map(ItemModelIndex::rule_spelling).collect();
            same("item-rules", want, &got)?;
        }
        if let Some(want) = &e.decorations {
            let got: Vec<String> = game
                .mods
                .decorations()
                .iter()
                .map(|d| d.spelling())
                .collect();
            same("decorations", want, &got)?;
        }
        if let Some(want) = &e.dropped_display {
            let d = game.mods.dropped_display();
            let got = format!(
                "flat {} blocks {} shadows {}",
                match d.flat {
                    ModFlatDisplay::Vanilla => "vanilla",
                    ModFlatDisplay::Sprite => "sprite",
                },
                match d.blocks {
                    ModBlockDisplay::Vanilla => "vanilla",
                    ModBlockDisplay::Billboard => "billboard",
                },
                if d.shadows { "on" } else { "off" },
            );
            same("dropped-display", &Some(want.clone()), &Some(got))?;
        }
        if let Some(want) = &e.map_screen {
            let got = match game.mods.map_screen() {
                Some((_, _, screen, _)) => format!(
                    "{}x{} scale {} at {} {} markers {}",
                    screen.image.width,
                    screen.image.height,
                    screen.image.blocks_per_pixel,
                    screen.image.center_x,
                    screen.image.center_z,
                    screen.markers.len()
                ),
                None => "none".to_string(),
            };
            same("map-screen", &Some(want.clone()), &Some(got))?;
        }
        if let Some(want) = &e.minimap_image {
            let got = match game.mods.minimap_image() {
                Some((_, _, image)) => {
                    let corner = match image.corner {
                        crate::ModMapCorner::TopLeft => "top-left",
                        crate::ModMapCorner::TopRight => "top-right",
                        crate::ModMapCorner::BottomLeft => "bottom-left",
                        crate::ModMapCorner::BottomRight => "bottom-right",
                    };
                    format!(
                        "{corner} {} {}x{}",
                        image.size, image.image.width, image.image.height
                    )
                }
                None => "none".to_string(),
            };
            same("minimap-image", &Some(want.clone()), &Some(got))?;
        }
        let feedback = |error: bool| {
            out.iter().rev().find_map(|o| match o {
                Output::Feedback { text, error: e } | Output::Reply { text, error: e, .. }
                    if *e == error =>
                {
                    Some(text.clone())
                }
                _ => None,
            })
        };
        if let Some(want) = &e.told {
            let name = |id: &str| {
                let p = game.players.iter().find(|p| p.id == id);
                p.map_or(id.to_owned(), |p| p.name.clone())
            };
            let got: Vec<String> = out
                .iter()
                .filter_map(|o| match o {
                    Output::Tell { player, text, .. } => Some(format!("{}: {text}", name(player))),
                    _ => None,
                })
                .collect();
            same("told", want, &got)?;
        }
        if let Some(want) = &e.broadcast {
            let got: Vec<String> = out
                .iter()
                .filter_map(|o| match o {
                    Output::Broadcast { text, .. } => Some(text.clone()),
                    _ => None,
                })
                .collect();
            same("broadcast", want, &got)?;
        }
        if let Some(want) = &e.feedback {
            same("feedback", &Some(want.clone()), &feedback(false))?;
        }
        if let Some(want) = &e.error {
            same("error", &Some(want.clone()), &feedback(true))?;
        }
        if let Some(want) = &e.panel {
            let got: Vec<String> = game
                .panel()
                .map(|p| p.lines.iter().map(|l| l.text.clone()).collect())
                .unwrap_or_default();
            same("panel", want, &got)?;
        }
        if let Some(want) = &e.world_lines {
            let got: Vec<(f64, f64, f64, f64, f64, f64, u32, f32)> = game
                .world_lines()
                .into_iter()
                .map(|(s, e, c, w)| (s[0], s[1], s[2], e[0], e[1], e[2], c, w))
                .collect();
            same("world-lines", want, &got)?;
        }
        if let Some(want) = &e.blocks {
            for b in want {
                let got = game.world.get(&(b.x, b.y, b.z));
                let got = got.map_or("minecraft:air", String::as_str);
                if got != b.state {
                    return Err(format!(
                        "block at {} {} {}: expected {}, got {got}",
                        b.x, b.y, b.z, b.state
                    ));
                }
            }
        }
        if let Some(want) = &e.corner {
            let got = game.panel().map(|p| corner_name(p.corner).to_string());
            same("corner", &Some(want.clone()), &got)?;
        }
        Ok(())
    }
}

fn corner_name(c: Corner) -> &'static str {
    match c {
        Corner::TopLeft => "top-left",
        Corner::TopRight => "top-right",
        Corner::BottomLeft => "bottom-left",
        Corner::BottomRight => "bottom-right",
    }
}

/// The distance along `dir` from `eye` to the box, if the ray hits it
/// within `reach` — the slab test the game's crosshair raycast uses.
fn ray_box(eye: [f64; 3], dir: [f64; 3], reach: f64, min: [f64; 3], max: [f64; 3]) -> Option<f64> {
    let mut t0: f64 = 0.0;
    let mut t1: f64 = reach;
    for a in 0..3 {
        if dir[a].abs() < f64::EPSILON {
            if eye[a] < min[a] || eye[a] > max[a] {
                return None;
            }
        } else {
            let (mut near, mut far) = ((min[a] - eye[a]) / dir[a], (max[a] - eye[a]) / dir[a]);
            if near > far {
                std::mem::swap(&mut near, &mut far);
            }
            t0 = t0.max(near);
            t1 = t1.min(far);
            if t0 > t1 {
                return None;
            }
        }
    }
    Some(t0.max(0.0))
}

fn same<T: PartialEq + std::fmt::Debug>(what: &str, want: &T, got: &T) -> Result<(), String> {
    if want == got {
        return Ok(());
    }
    Err(format!(
        "{what}\n    expected: {want:?}\n    got:      {got:?}"
    ))
}
