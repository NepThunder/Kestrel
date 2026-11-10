//! Loading and calling mods with wasmtime's component model.
use crate::manifest::{Capability, Manifest, Package, Side};
use server_bindings::{
    HorizonServerMod,
    exports::horizon::mods::server_events::Registration as ServerRegistration,
    horizon::mods::{players, world as mod_world},
};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime},
};
use wasmtime::{
    Config, Engine, Store, StoreLimits, StoreLimitsBuilder, Trap,
    component::{Component, HasSelf, Linker},
};

mod bindings {
    wasmtime::component::bindgen!({
        path: "../horizon-mod-sdk/wit",
        world: "horizon-mod",
        // `Output::SetBlocks` derives `PartialEq` over `world::BlockChange`.
        additional_derives: [PartialEq],
    });
}
/// The server mods' world: `log` and `storage` are the client world's.
mod server_bindings {
    wasmtime::component::bindgen!({
        path: "../horizon-mod-sdk/wit",
        world: "horizon-server-mod",
        additional_derives: [PartialEq],
        with: {
            "horizon:mods/log": super::bindings::horizon::mods::log,
            "horizon:mods/storage": super::bindings::horizon::mods::storage,
            // `world` is the client world's too now (client mods import it
            // read-only): one generated module, one `Host` impl, and one
            // `add_to_linker` defining it, or the linker sees `get-block`
            // twice and no mod starts.
            "horizon:mods/world": super::bindings::horizon::mods::world,
        },
    });
}
/// Capability `ui`'s world beyond `horizon-mod` (docs/mods.md): a second
/// world, so mods built before it (no `ui-events` export) still instantiate
/// as `horizon-mod` and simply hear nothing of their widgets.
mod ui_bindings {
    wasmtime::component::bindgen!({
        path: "../horizon-mod-sdk/wit",
        world: "ui-host",
    });
}
pub use bindings::horizon::mods::{
    entity::{Entity, Target},
    game::{Clock, Effect as ModEffect, Hit, Mount as ModMount, Player, SelfState as ModSelfState},
    hud::{Corner, Line},
    inventory::{Container, InvStack, PlayerInventory},
};
use bindings::{
    HorizonMod,
    exports::horizon::mods::events::Registration,
    horizon::mods::{chat, command, entity, game, hud, inventory, log as mod_log, storage},
};
pub use server_bindings::{
    exports::horizon::mods::server_events::{
        Argument, ArgumentKind, ArgumentValue, BlockPos, ServerCommand,
    },
    horizon::mods::players::{GameMode, Player as ServerPlayer},
    horizon::mods::world::BlockChange,
};
type ClientEvents = bindings::exports::horizon::mods::events::Guest;
type ServerEvents = server_bindings::exports::horizon::mods::server_events::Guest;
/// The widget model a `ui` screen or settings page shows (WIT `interface
/// ui`), for the game to draw with its menus' own widgets.
pub use ui_bindings::horizon::mods::ui::{
    ButtonWidget, ItemWidget, ListWidget, Screen as UiScreen, TextWidget, Widget as UiWidget,
};
use ui_bindings::{UiHost, horizon::mods::ui};
/// Capability `audio`'s world beyond `horizon-mod` (API 15): a second
/// world, so mods built before it (no `audio-events` export) still
/// instantiate as `horizon-mod` and simply never hear sounds.
mod audio_bindings {
    wasmtime::component::bindgen!({
        path: "../horizon-mod-sdk/wit",
        world: "audio-host",
        additional_derives: [PartialEq],
    });
}
/// What the game asks a sound mod (WIT `interface audio-events`).
pub use audio_bindings::exports::horizon::mods::audio_events::{
    SoundAdjustment as ModSoundAdjustment, SoundPosition as ModSoundPosition,
    SoundQuery as ModSoundQuery,
};
use audio_bindings::{AudioHost, horizon::mods::audio};
/// Capability `render`'s imports (docs/mods.md): the game draws the shapes.
mod render_bindings {
    wasmtime::component::bindgen!({
        path: "../horizon-mod-sdk/wit",
        world: "render-host",
        // As the other worlds: `Mods::hand_offset` and the tests
        // compare the hand's transform by value.
        additional_derives: [PartialEq],
    });
}
use render_bindings::horizon::mods::actions;
/// The crosshair's target and the clicks a mod asks the game to
/// make (WIT `interface actions`, API 21): its own capability,
/// hosted with the render world as the hand and the poses are.
/// `ModHand` here is THIS interface's `hand` enum — which hand an
/// action uses — not the hand district's types (`ModHandState`,
/// `ModHandTransform`); WIT interfaces cannot share types, and
/// the name collision is the entity-pose swing-anim one again.
pub use render_bindings::horizon::mods::actions::{
    BlockTarget as ModBlockTarget, CrosshairTarget as ModActionTarget,
    EntityTarget as ModEntityTarget, Face as ModFace, Hand as ModHand,
};
use render_bindings::horizon::mods::chat_decoration;
/// The game's own chat, restyled (WIT `interface
/// chat-decoration`, API 27): its own capability, hosted with
/// the render world as the actions and the screens are. The
/// mod declares two policies and nothing else — no name, no
/// line, no picture crosses — and the game draws.
pub use render_bindings::horizon::mods::chat_decoration::{
    ChatAnimation as ModChatAnimation, LineDecoration as ModLineDecoration,
};
use render_bindings::horizon::mods::dynamic_light;
/// Light of the mod's own in the game's dynamic lights, and
/// its policy over the game's own gather (WIT `interface
/// dynamic-light`, API 28): a `render` extension, hosted
/// with the render world, as the particles are.
/// `ModLightPoint` is the interface's own `world-point`
/// record — `render`'s type of that name is already
/// [`WorldPoint`] here, and the particles' is
/// [`ModParticlePoint`].
pub use render_bindings::horizon::mods::dynamic_light::{
    DynamicLightPolicy as ModDynamicLightPolicy, GlowingKind as ModGlowingKind,
    LightSource as ModLightSource, SourceAnchor as ModLightAnchor, WorldPoint as ModLightPoint,
};
use render_bindings::horizon::mods::entity_pose;
/// Player models' pose states and the offsets a mod sets on them
/// (WIT `interface entity-pose`, API 20): a `render` extension,
/// hosted with the render world, as the hand is. The swing
/// animation here is the entity-pose interface's own copy of the
/// hand enum (WIT interfaces cannot share types).
pub use render_bindings::horizon::mods::entity_pose::{
    ArmPose as ModArmPose, EntityPoseState as ModEntityPoseState, ItemUse as ModItemUse,
    PartOffset as ModPartOffset, PoseOffset as ModPoseOffset, PoseTarget as ModPoseTarget,
    SwingAnim as ModPoseSwingAnim,
};
use render_bindings::horizon::mods::hand;
/// The first-person hand's state and the offset a mod sets on it
/// (WIT `interface hand`, API 16), and the swing animation the game
/// pushes with it (API 17): a `render` extension, hosted with the
/// render world.
pub use render_bindings::horizon::mods::hand::{
    HandState as ModHandState, HandTransform as ModHandTransform, SwingAnim as ModSwingAnim,
    SwingKind as ModSwingKind,
};
use render_bindings::horizon::mods::input;
/// Capability `input`'s records (WIT `interface input`).
pub use render_bindings::horizon::mods::input::{Event as InputEvent, HeldItem, ItemUse};
use render_bindings::horizon::mods::inventory_actions;
/// The player's own slot clicks (WIT `interface
/// inventory-actions`, API 24): its own capability, hosted with
/// the render world as the actions and the screens are — never
/// folded into `inventory`, whose consent line is seeing the
/// inventory, not moving its stacks.
pub use render_bindings::horizon::mods::inventory_actions::{
    ClickKind as ModClickKind, DragButton as ModDragButton, WheelEvent as ModWheelEvent,
};
use render_bindings::horizon::mods::item_model;
/// How items themselves are shown (WIT `interface item-model`,
/// API 23): a `render` extension, hosted with the render world,
/// as the hand and the poses are. `ModDisplayContext` here is
/// THIS interface's grouping of the game's display contexts —
/// not the assets' enum of the same name, which the game maps
/// from at its one swap point.
pub use render_bindings::horizon::mods::item_model::{
    BlockDisplay as ModBlockDisplay, ComponentMatch as ModComponentMatch,
    Decoration as ModDecoration, DecorationPlace as ModDecorationPlace,
    DecorationTarget as ModDecorationTarget, DisplayContext as ModDisplayContext,
    DroppedDisplay as ModDroppedDisplay, FlatDisplay as ModFlatDisplay,
    ModelMatch as ModModelMatch, ModelRule as ModModelRule,
};
use render_bindings::horizon::mods::map;
/// The game's own mapping (WIT `interface map`, API 25): its
/// own capability, hosted with the render world as the actions
/// and the screens are. `ModMapView` here is the interface's
/// `map-view` record — the fullscreen map's view — not the
/// lent [`MapView`] trait the tile reads answer through; the
/// name collision is the actions hand one again.
pub use render_bindings::horizon::mods::map::{
    ChunkPos as ModChunkPos, Corner as ModMapCorner, MapClick as ModMapClick,
    MapContext as ModMapContext, MapEvent as ModMapEvent, MapImage as ModMapImage,
    MapLayer as ModMapLayer, MapMarker as ModMapMarker, MapScreen as ModMapScreen,
    MapTile as ModMapTile, MapView as ModMapView, MinimapImage as ModMinimapImage,
    Position as ModPosition, PositionEvent as ModPositionEvent, TileVersion as ModTileVersion,
};
use render_bindings::horizon::mods::particles;
/// Particles of the mod's own (WIT `interface particles`,
/// API 26): a `render` extension, hosted with the render
/// world, as the hand and the item models are.
/// `ModParticlePoint` is the interface's own `world-point`
/// record — `render`'s type of that name is already
/// [`WorldPoint`] here.
pub use render_bindings::horizon::mods::particles::{
    Burst as ModBurst, Emitter as ModEmitter, EmitterAnchor as ModEmitterAnchor,
    ParticleParams as ModParticleParams, WorldPoint as ModParticlePoint,
};
/// What capability `render` asks the game to draw (WIT `interface render`).
pub use render_bindings::horizon::mods::render::{
    CameraOverride as ModCameraOverride, CameraPose as ModCameraPose, Corner as RenderCorner,
    FarTerrain as ModFarTerrain, FillShape, FrameRate, HandFov as ModHandFov,
    HandFovMode as ModHandFovMode, ItemShape, Minimap as ModMinimap, Point as RenderPoint,
    QuadShape, Rect as RenderRect, ScreenPoint as ModScreenPoint, Shape, SpriteShape, TextShape,
    WorldBox, WorldLine, WorldPoint, WorldShape, Zoom as ModZoom,
};
use render_bindings::horizon::mods::screen;
/// The game's own screens as a mod sees and decorates them
/// (WIT `interface screen`, API 22): its own capability, hosted
/// with the render world as the hand, the poses and the actions
/// are. `ModScreenState` is the interface's `screen-state`
/// record; the name collision with the game's own screens is
/// the interface's, kept for the same reason the actions types
/// keep theirs.
pub use render_bindings::horizon::mods::screen::{
    Anchor as ModAnchor, Highlight as ModHighlight, ScreenButton as ModScreenButton,
    ScreenEvent as ModScreenEvent, ScreenState as ModScreenState, TradeList as ModTradeList,
    TradeOffer as ModTradeOffer, TradeStack as ModTradeStack,
};
use render_bindings::{RenderHost, horizon::mods::render};
/// Capability `recipes`'s imports (docs/mods.md): the game's recipes and items.
mod recipes_bindings {
    wasmtime::component::bindgen!({
        path: "../horizon-mod-sdk/wit",
        world: "recipes-host",
    });
}
/// Capability `ui`'s recipe viewer panel (WIT `interface browser`).
pub use recipes_bindings::horizon::mods::browser::{
    Click as BrowserClick, Event as BrowserEvent, ItemKey as BrowserItemKey,
    Layout as BrowserLayout, RecipeView as BrowserRecipe, SlotClick as BrowserSlotClick,
    Stack as BrowserStack,
};
/// What capability `recipes` reads (WIT `interface recipes`).
pub use recipes_bindings::horizon::mods::recipes::{
    Item as RecipeItem, Kind as RecipeKind, Recipe, Stack as RecipeStack,
};
use recipes_bindings::{RecipesHost, horizon::mods::browser, horizon::mods::recipes};

/// A mod's linear memory cap.
const MEMORY: usize = 64 << 20;
/// Fuel (about one unit per WebAssembly instruction) per call: a bound on
/// runaway loops, far above the hooks' time budgets.
const FUEL_INIT: u64 = 2_000_000_000;
const FUEL_TICK: u64 = 50_000_000;
const FUEL_FRAME: u64 = 10_000_000;
const FUEL_EVENT: u64 = 100_000_000;
/// A sound call (API 15): sounds are frequent and the math is small.
const FUEL_SOUND: u64 = 5_000_000;
/// Blocks a mod may set per `world.set-blocks` call.
pub const BLOCKS_PER_CALL: usize = 4096;
/// Chat lines a mod may show per call, HUD lines and their length.
const CHAT_PER_CALL: usize = 8;
/// `command.run-quiet` lines one call of a mod may queue (a quiet payload's
/// cap, `horizon_protocol::quiet::MAX_LINES`).
const QUIET_PER_CALL: usize = 256;
/// Actions one hook call of a mod may queue (capability `actions`,
/// API 21): a held button's own cadence bounds what a tick can
/// spend, and the queue is a click queue, not a script.
pub const ACTIONS_PER_CALL: usize = 8;
/// Buttons one mod may declare across the game's screens
/// (capability `screen`, API 22), and the marks one mod may set
/// on the open screen: a declaration is a page of the screen's
/// furniture, not a second screen.
pub const SCREEN_BUTTONS: usize = 16;
pub const SCREEN_HIGHLIGHTS: usize = 512;
/// Screen events kept for one mod's `screen.poll` (API 22): the
/// `browser` panel's queue discipline — a press the mod never
/// polls for is not kept forever.
pub const SCREEN_EVENTS: usize = 256;
/// Model rules one mod may declare (WIT `item-model`, API 23),
/// and the component matches one rule may key on: a declaration
/// is a page of swaps, each keyed as narrowly as a renamed item
/// — never a database.
pub const MODEL_RULES: usize = 64;
pub const MODEL_MATCHES: usize = 4;
/// Slot marks one mod may declare (WIT `item-model`, API 23):
/// the `screen` highlights' cap, for the same reason — a page
/// of the game's slots, not a second inventory.
pub const ITEM_DECORATIONS: usize = 256;
/// Weighed slot verbs one hook call of a mod may queue
/// (capability `inventory-actions`, API 24): a click weighs 1,
/// a drag weighs 8 (four drags fill a call). The district's own
/// constant, not `ACTIONS_PER_CALL`'s: the actions queue is hot
/// (mid-look), this one is cold (between ticks, in menus), and
/// the two budgets mean different things. The game caps the
/// pending queue per mod on the same weighed count
/// ([`SLOT_VERBS_PER_MOD`]).
pub const SLOT_VERBS_PER_CALL: usize = 32;
/// Weighed slot verbs one mod may have pending in the game at
/// once (API 24): two hook calls' worth between ticks — a mod
/// one tick behind is fine, a mod scripting a long sequence is
/// not what the surface is for.
pub const SLOT_VERBS_PER_MOD: usize = 64;
/// Wheel events kept for one mod's `inventory-actions.poll-wheel`
/// (API 24): the `screen` events' cap, for the same reason — a
/// turn the mod never polls for is not kept forever.
pub const WHEEL_EVENTS: usize = 256;
/// Tiles one `map.tiles` call may read (capability `map`, API
/// 25): the `world.get-blocks` discipline — over the cap, no
/// results — at a tile-sized cap (a tile is 768 bytes; 64 of
/// them are about 49 KB a call). Versions are the cheap poll:
/// 256 chunks a `map.tile-versions` call.
pub const TILES_PER_CALL: usize = 64;
pub const TILE_VERSIONS_PER_CALL: usize = 256;
/// Events kept for one mod's `map.poll-events` and
/// `map.poll-map` (API 25): the `screen` events' cap, twice
/// over — a death or a click the mod never polls for is not
/// kept forever.
pub const MAP_EVENTS: usize = 256;
/// Markers one mod may put on its map screen (API 25): a page
/// of named places, as `screen`'s buttons are a page of a
/// screen's furniture.
pub const MAP_MARKERS: usize = 128;
/// A map image's largest side and pixel count (API 25): a
/// screen-sized picture crosses as one byte per pixel, and
/// 2048 by 2048 of them is the whole allowance.
pub const MAP_IMAGE_MAX_SIDE: u32 = 2048;
pub const MAP_IMAGE_MAX_PIXELS: u64 = 4_194_304;
/// Particles one burst may ask for (capability `particles`,
/// API 26): the `/particle` command's own count discipline —
/// a larger count is cut to it at the door.
pub const PARTICLE_COUNT_MAX: u32 = 256;
/// Particles one mod may spawn in one tick, weighed across
/// its bursts and its emitters together (API 26): past it,
/// the rest of the tick's asks spawn nothing — drop, never
/// defer. The engine's own funnel (its distance cull, the
/// Particles option, its hard cap) prices what the game
/// draws; this prices what a mod asks.
pub const PARTICLE_BUDGET_PER_TICK: usize = 1024;
/// Emitters one mod may hold (API 26), and the most spawns
/// a second one may ask for: a standing stream is a wingtip
/// or a campfire, not a weather system.
pub const PARTICLE_EMITTERS: usize = 8;
pub const PARTICLE_EMITTER_RATE_MAX: f32 = 200.0;
/// The longest chat animation a mod may ask for, in
/// milliseconds (capability `chat-decoration`, API 27): a
/// larger duration is cut to it at the door — an arrival or
/// an opening may ease for a second, never longer.
pub const CHAT_ANIMATION_MS_MAX: u32 = 1000;
/// Sources one mod may register in the game's dynamic
/// lights (capability `render`'s `dynamic-light`, API 28),
/// and the glowing kinds one policy may name: a mod's own
/// lights are landmarks and held things, not a second
/// lighting system — and the arena they join is the game's
/// own sixteen.
pub const DYNAMIC_LIGHT_SOURCES: usize = 32;
pub const DYNAMIC_LIGHT_KINDS: usize = 32;
/// An item or kind name's length cap (API 28): no loaded
/// item's or entity type's name is near it, and a longer
/// one names nothing.
const DYNAMIC_LIGHT_NAME_CHARS: usize = 128;
/// A particle type name's length cap (API 26): no loaded
/// type's name is near it, and a longer one spawns nothing.
const PARTICLE_NAME_CHARS: usize = 128;
/// A burst's spread on one axis, in blocks (API 26), and
/// its speed in blocks a tick: the ranges the game's own
/// particle packet keeps to.
const PARTICLE_SPREAD_MAX: f64 = 16.0;
const PARTICLE_SPEED_MAX: f64 = 20.0;
/// An emitter's offset from its anchor on one axis, in
/// blocks (API 26): an emitter is tied to its anchor, not
/// a second burst position.
const PARTICLE_OFFSET_MAX: f64 = 8.0;
/// How far `game.raycast` sees, in blocks.
pub const RAYCAST_MAX: f32 = 1024.0;
/// How far `entity.targeted` sees, in blocks.
pub const ENTITY_TARGET_MAX: f32 = 128.0;
/// How far `entity.entities` lists, in blocks, and how many it lists at most.
pub const ENTITY_LIST_RANGE: f64 = 128.0;
pub const ENTITY_LIST_MAX: usize = 256;
const HUD_LINES: usize = 16;
const LINE_CHARS: usize = 256;
/// Total bytes a mod's `storage` may hold on disk, across all its keys.
const STORAGE_QUOTA: u64 = 4 << 20;
/// Distinct keys a mod's `storage` may hold: `STORAGE_QUOTA` alone doesn't
/// bound a flood of zero- or near-zero-byte files (each still a file name,
/// a directory entry and an inode).
const STORAGE_MAX_KEYS: usize = 64;
/// Bytes `storage::set` may write across one hook call: fuel bounds a
/// mod's CPU time per call, not the time a synchronous disk write takes, so
/// a cheap-in-fuel loop replacing one key with large payloads needs its own
/// cap, separate from the on-disk quota it would otherwise stay under.
const STORAGE_WRITE_BUDGET: u64 = 1 << 20;

/// What mods may read of the game, copied once per tick by the game.
/// The default is the vanilla one: brightness 0.5, fog on.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub player: Option<Player>,
    pub clock: Option<Clock>,
    /// The screen in GUI pixels (capability `render`'s `gui-size`).
    pub gui_size: (f32, f32),
    /// The main hand's stack (capability `input`'s `held`).
    pub held: Option<HeldItem>,
    /// The game's frame counter: fps, min, 1% low (`render.frame-stats`).
    pub frame: (u32, u32, u32),
    /// The movement and click mappings `input.mapping` answers for:
    /// (mapping, the bound key's label, held).
    pub mappings: Vec<(&'static str, String, bool)>,
    /// Attack and use clicks in the last second.
    pub cps: (u32, u32),
    /// The HUD font's advance per character from U+0000
    /// (`render.text-width`); empty: 6 each.
    pub font_widths: Arc<[f32]>,
    /// The players on the server, for server mods.
    pub players: Vec<ServerPlayer>,
    /// The first block along the player's view within [`RAYCAST_MAX`] and
    /// its distance from the eye, for `game.raycast`; the game works it out
    /// only while a mod calls it ([`Mods::wants_raycast`]).
    pub view_hit: Option<(f32, Hit)>,
    /// Every item and recipe, for capability `recipes`; the game makes them
    /// once, and only while a mod reads them ([`Mods::wants_recipes`]).
    pub recipes: Option<Arc<Recipes>>,
    /// The `book-id`s of the recipes the player's book holds.
    pub known_recipes: Arc<[i32]>,
    /// The open screen takes recipes (`recipes.place`).
    pub placing: bool,
    /// The player is in creative mode (`recipes.give`).
    pub creative: bool,
    /// The player's own inventory, armour, offhand, crafting grid and
    /// carried stack (capability `inventory`); the game fills it once
    /// per tick, and only while a mod reads it
    /// ([`Mods::wants_inventory`]). None before any inventory mod runs.
    pub inventory: Option<PlayerInventory>,
    /// The open container's kind, slots and data slots (capability
    /// `inventory`); None when no container screen is open, so a mod
    /// can never read a container after closing it.
    pub open_container: Option<Container>,
    /// The living mobs within 128 blocks, nearest first, at most 256
    /// (capability `entity`); the game fills it once per tick, and only
    /// while a mod reads it ([`Mods::wants_entities`]). Empty in
    /// multiplayer in v1, and on the title screen.
    pub entities: Vec<Entity>,
    /// The mob under the crosshair, if any, with its distance from the
    /// eye (capability `entity`); None when nothing is hit, on the title
    /// screen, or while no mod reads it.
    pub targeted: Option<Target>,
    /// The player's stored Brightness setting (`render.get-gamma`'s
    /// fallback when no running mod sets one).
    pub gamma: f32,
    /// The player's stored Fog setting (`render.get-fog`'s fallback).
    pub fog: bool,
    /// The camera the last frame rendered from (capability `render`'s
    /// `get-camera` / `project`): the mod override when one is active,
    /// else the player's own camera, with the effective field of view.
    /// None before the first frame renders, and on the title screen
    /// until the game fills in the panorama's pose.
    pub camera: Option<SnapshotCamera>,
    /// The local player's active effects, in the game's order
    /// (capability `game-read`, API 18); the game fills it once per
    /// tick, and only while a mod reads it
    /// ([`Mods::wants_self_state`]). Empty on the title screen.
    pub effects: Vec<ModEffect>,
    /// The local player's movement state, ridden mount and movement
    /// toggle options (capability `game-read`, API 18); filled with
    /// [`Snapshot::effects`]. None on the title screen.
    pub self_state: Option<ModSelfState>,
    /// Whether a mod's `set-zoom` would take effect now (capability
    /// `render`, API 19): the player is in the world with no screen
    /// open, and not scoping — the gate the game's own zoom applies,
    /// filled where the mappings are.
    pub can_zoom: bool,
    /// The zoom the spyglass is showing (capability `render`,
    /// API 19): while the player scopes in first person, 1 / the
    /// spyglass's field-of-view modifier, from the game's own arm —
    /// never a second constant. None while not scoping.
    pub spyglass_zoom: Option<f32>,
    /// Milliseconds since the player's last input (capability
    /// `input`, API 19): the game's own inactivity clock, the one
    /// its idle framerate limit reads, filled where the mappings are.
    pub last_input_ms: u64,
    /// What the crosshair rests on, in the game's own pick order
    /// (capability `actions`, API 21): the block within block
    /// reach — with the face and cursor `game.raycast`'s hit
    /// lacks — or the entity in front of it; the game fills it
    /// beside the raycast hit, and only while a mod reads it
    /// ([`Mods::wants_actions`]). None when nothing is hit, and
    /// on the title screen.
    pub action_target: Option<ModActionTarget>,
    /// The open screen (capability `screen`, API 22): its id,
    /// the open menu's type and title, its panel's rect, the
    /// hovered slot and whether the debug overlay is open; the
    /// game fills it beside [`Snapshot::open_container`], and
    /// only while a mod reads it ([`Mods::wants_screen`]). The
    /// `playing` state, nothing hovered, until a game fills it.
    pub screen_state: ModScreenState,
    /// The open merchant's offers (capability `screen`, API
    /// 22), filled with [`Snapshot::screen_state`]; the
    /// container record deliberately carries none. None while
    /// no merchant screen is open.
    pub trades: Option<ModTradeList>,
    /// Where the player is playing (capability `map`, API
    /// 25): the dimension and the world or server's name, for
    /// keying a mod's own stored maps; the game fills it once
    /// per tick, and only while a mod reads it
    /// ([`Mods::wants_map`]). None on the title screen.
    pub map_context: Option<ModMapContext>,
    /// Where the player last died (capability `map`, API 25),
    /// the game's own record of it, filled with
    /// [`Snapshot::map_context`]. None when the player has not
    /// died in this world.
    pub last_death: Option<ModPosition>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            player: None,
            clock: None,
            gui_size: (0.0, 0.0),
            held: None,
            frame: (0, 0, 0),
            mappings: Vec::new(),
            cps: (0, 0),
            font_widths: Arc::from([]),
            players: Vec::new(),
            view_hit: None,
            recipes: None,
            known_recipes: Arc::from([]),
            placing: false,
            creative: false,
            inventory: None,
            open_container: None,
            entities: Vec::new(),
            targeted: None,
            gamma: 0.5,
            fog: true,
            camera: None,
            effects: Vec::new(),
            self_state: None,
            can_zoom: false,
            spyglass_zoom: None,
            last_input_ms: 0,
            action_target: None,
            screen_state: ModScreenState {
                id: "playing".into(),
                menu: None,
                title: None,
                rect: (0.0, 0.0, 0.0, 0.0),
                hovered_slot: None,
                debug_open: false,
            },
            trades: None,
            map_context: None,
            last_death: None,
        }
    }
}

/// The camera a frame renders from, as the game fills it into the
/// [`Snapshot`] for `render.get-camera` / `render.project`: the position
/// and look in blocks and degrees, and the effective field of view
/// (the zoomed one, as the player sees it).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SnapshotCamera {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
    pub fov: f32,
}

/// Capability `recipes`'s tables: `items` in the registry's order, which
/// each [`RecipeStack::item`] indexes.
#[derive(Clone, Debug, Default)]
pub struct Recipes {
    pub items: Vec<RecipeItem>,
    pub recipes: Vec<Recipe>,
}

/// Which mods a game starts: the window's game the client mods, the game
/// simulating the world the server mods, single player in the frame both.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Runs {
    Client,
    Server,
    Both,
}
impl Runs {
    fn starts(self, side: Side) -> bool {
        match self {
            Runs::Both => true,
            Runs::Client => side != Side::Server,
            Runs::Server => side == Side::Server,
        }
    }
}

/// One click a mod asked the game to make (capability `actions`,
/// API 21), as an [`Output::Action`]: performed as the player in
/// the next tick's input step, through the same path a real click
/// takes, and dropped — never retried — when the game cannot
/// perform it then. Held buttons are not here: they are state
/// ([`Mods::held_buttons`]), not requests.
#[derive(Clone, Debug, PartialEq)]
pub enum ModAction {
    /// `actions.use`: one click of the use button.
    Use,
    /// `actions.use-on-block`: one use of `hand` on this block
    /// target, against a fresh pick when it is performed.
    UseOnBlock {
        target: ModBlockTarget,
        hand: ModHand,
    },
    /// `actions.attack`: one click of the attack button.
    Attack,
    /// `actions.select-slot`: hotbar slot 0 to 8, validated by
    /// the host call.
    SelectSlot(u32),
}

/// One slot click a mod asked the game to make (capability
/// `inventory-actions`, API 24), as an [`Output::SlotClick`]:
/// performed as the player in the next tick, through the game's
/// own menu logic, and dropped — never retried — when the game
/// cannot perform it then. The arguments are the mod's own, in
/// `inventory`'s slot numbering; the host call validated the
/// malformed ones (a slot past 999, a button the kind does not
/// take) and nothing else — whether the click means anything in
/// the menu that is open is the menu's own judgement, made when
/// it is performed.
#[derive(Clone, Debug, PartialEq)]
pub struct ModSlotClick {
    /// The slot in `inventory`'s numbering, or `None` for a
    /// click outside the window.
    pub slot: Option<u32>,
    /// The click's button, as the kind takes it.
    pub button: u32,
    /// The click's kind.
    pub kind: ModClickKind,
}

/// One drag of the hand's stack a mod asked the game to make
/// (capability `inventory-actions`, API 24), as an
/// [`Output::SlotDrag`]: performed whole, as one gesture,
/// because a drag's state lives inside one menu session and
/// cannot be split across ticks.
#[derive(Clone, Debug, PartialEq)]
pub struct ModSlotDrag {
    /// The button the drag is made with: it also chooses how
    /// the stack is shared out.
    pub button: ModDragButton,
    /// The slots dragged over, in the order given, 1 to 64.
    pub slots: Vec<u32>,
}

/// A queued slot verb (API 24): a click or a drag, with the
/// weighed count both queue caps charge for it — a click
/// weighs 1, a drag weighs 8 (four drags fill a call's 32).
#[derive(Clone, Debug, PartialEq)]
pub enum ModSlotMove {
    Click(ModSlotClick),
    Drag(ModSlotDrag),
}

impl ModSlotMove {
    /// The weighed count this verb costs against the per-call
    /// and per-mod caps.
    pub fn weight(&self) -> usize {
        match self {
            ModSlotMove::Click(_) => 1,
            ModSlotMove::Drag(_) => 8,
        }
    }

    /// Whether the verb names a slot of the open menu itself —
    /// a container's own slots (300 to 499) or a workstation's
    /// inputs and result (600 up) — rather than one of the
    /// player's own slots (0 to 35, 100 to 108, 200, 500 to
    /// 504) or the outside of the window. The game's window
    /// guard drops a verb naming the open menu's slots when the
    /// window it was collected against is no longer the window
    /// open at the drain; the player's own slots exist in every
    /// menu, so those verbs still perform.
    pub fn names_open_menu(&self) -> bool {
        fn open_menu(slot: u32) -> bool {
            (300..500).contains(&slot) || slot >= 600
        }
        match self {
            ModSlotMove::Click(click) => click.slot.is_some_and(open_menu),
            ModSlotMove::Drag(drag) => drag.slots.iter().any(|s| open_menu(*s)),
        }
    }
}

/// What mods asked the game to show since the last [`Mods::drain`].
#[derive(Clone, Debug, PartialEq)]
pub enum Output {
    /// `chat.show`: a line after the mod's name.
    Chat { name: String, text: String },
    /// A mod command's feedback, red when `error`.
    Feedback { text: String, error: bool },
    /// `command.run`: a vanilla command line, without its slash, to run as
    /// the player.
    Command { line: String },
    /// `command.run-quiet`: vanilla command lines to run as the player with
    /// nothing in the chat (`horizon_protocol::quiet` on a server).
    QuietCommands { mod_id: String, lines: Vec<String> },
    /// The mod was turned off: it trapped, overran or failed to start.
    Stopped { name: String, reason: String },
    /// `players.tell`: a line after the mod's name, to the player with this id.
    Tell {
        name: String,
        player: String,
        text: String,
    },
    /// `players.broadcast`: a line after the mod's name, to every player.
    Broadcast { name: String, text: String },
    /// A server mod command's feedback, to the player who typed it.
    Reply {
        player: String,
        text: String,
        error: bool,
    },
    /// `world.set-blocks`: changes to make in `dimension`, in order, each
    /// state checked by the [`WorldView`] lent for the call.
    SetBlocks {
        name: String,
        dimension: String,
        blocks: Vec<BlockChange>,
    },
    /// `ui.open-screen`: the mod's entry in `entries()`; what it shows is
    /// [`Mods::screen`].
    OpenScreen { owner: usize },
    /// `ui.close-screen`: the mod closed its own screen (no `closed` back).
    CloseScreen { owner: usize },
    /// `render.shake`: strength 0 to 1, fading out over `seconds` (≤ 10).
    Shake { strength: f32, seconds: f32 },
    /// `recipes.place`: the recipe book's click on this display id, all of
    /// it when `all`.
    PlaceRecipe { book_id: i32, all: bool },
    /// `recipes.give`: a stack for a creative player.
    Give {
        item: String,
        count: u32,
        components: String,
    },
    /// `game.set-movement-toggles` (API 18): the Toggle Sprint /
    /// Toggle Crouch options to set; a `None` half leaves that
    /// option as it is. The game applies and saves them as an
    /// options-page change.
    SetMovementToggles {
        sprint: Option<bool>,
        sneak: Option<bool>,
    },
    /// `actions` (API 21): one click for the game to perform as
    /// the player in the next tick's input step. The game collects
    /// these instead of acting on them where they drain: actions
    /// may only perform inside the input step.
    Action { mod_id: String, action: ModAction },
    /// `screen.select-trade` (API 22): the merchant screen's own
    /// trade choice, for the game to perform in the next tick
    /// through the screen's own function — the dedicated packet,
    /// never a slot click. The game collects these beside the
    /// actions and drops one whose merchant has closed, silently.
    SelectTrade { mod_id: String, index: u32 },
    /// `inventory-actions.click` (API 24): one slot click for
    /// the game to perform as the player in the next tick,
    /// through the game's own menu logic. The game collects
    /// these instead of acting on them where they drain: slot
    /// moves may only perform in the drain beside the actions,
    /// under their own gate.
    SlotClick { mod_id: String, click: ModSlotClick },
    /// `inventory-actions.drag` (API 24): one drag of the hand's
    /// stack for the game to perform whole in the next tick, as
    /// one menu session's QuickCraft run.
    SlotDrag { mod_id: String, drag: ModSlotDrag },
    /// `particles.emit` (API 26): one burst for the game to
    /// draw with its own particle engine after the call
    /// returns, sanitized and weighed at the door. The game
    /// collects these beside the actions: a burst only draws
    /// in the tick's output step.
    ParticleBurst { mod_id: String, burst: ModBurst },
}

/// The game's world as capability `world` reads it during a call: the
/// game lends one with [`Mods::lend_world`] and takes it back after.
pub trait WorldView: std::any::Any + Send {
    /// The block state at a position, `minecraft:oak_log[axis=y]`; `None`
    /// where no chunk of that dimension is loaded.
    fn block(&self, dimension: &str, x: i32, y: i32, z: i32) -> Option<String>;
    /// The biome at a position, `minecraft:plains`; `None` as for blocks.
    fn biome(&self, dimension: &str, x: i32, y: i32, z: i32) -> Option<String>;
    /// The light at a position, `(block, sky)`, each 0 to 15; `None`
    /// where no chunk of that dimension is loaded, its light has not
    /// been computed yet, or the position is outside the column's
    /// build range. Never a guessed value: an unlit chunk would read
    /// as full sky light, so it reads as `None` instead.
    fn light(&self, dimension: &str, x: i32, y: i32, z: i32) -> Option<(u8, u8)>;
    /// Whether `dimension` is one the server has.
    fn has_dimension(&self, dimension: &str) -> bool;
    /// `Err` (why) for a state string that names no block state.
    fn check_state(&self, state: &str) -> Result<(), String>;
}

/// The view lent for the current calls, shared by every mod's store.
type WorldSlot = Arc<Mutex<Option<Box<dyn WorldView>>>>;

/// One chunk's tile as the game's map holds it (capability
/// `map`, API 25): the arrays verbatim, in the game's column
/// order (`z * 16 + x`), the layer it was scanned at, its cut
/// on a cave tile, and the version the host keeps beside the
/// cache. The lent [`MapView`] answers with these; the host
/// calls assemble the WIT record from one.
#[derive(Clone, Debug, PartialEq)]
pub struct MapTileData {
    pub layer: ModMapLayer,
    /// A cave tile's cut: the head height its floor and walls
    /// were scanned at. None on a surface tile.
    pub cut_y: Option<i32>,
    pub version: u64,
    /// The vanilla map colour of each column, the wall bit
    /// (0x80) intact on a cave tile's cut walls.
    pub colors: [u8; 256],
    /// Each column's height: the y its colour was scanned at.
    pub heights: [i16; 256],
    /// The fluid depth over each column.
    pub depths: [u8; 256],
}

/// The game's map cache as capability `map` reads it during a
/// call: the game lends one with [`Mods::lend_map`] and takes
/// it back after, as it lends the [`WorldView`]. A read never
/// triggers a scan: the view is the cache exactly as it stands.
pub trait MapView: std::any::Any + Send {
    /// The tile of the chunk at `(cx, cz)`, or None where the
    /// game's map holds none (an unloaded chunk, or one not
    /// scanned yet).
    fn tile(&self, cx: i32, cz: i32) -> Option<MapTileData>;
    /// The version of the chunk's tile, without the tile;
    /// None as for [`MapView::tile`].
    fn tile_version(&self, cx: i32, cz: i32) -> Option<u64>;
}

/// The map view lent for the current calls, shared by every
/// mod's store, as [`WorldSlot`] is.
type MapSlot = Arc<Mutex<Option<Box<dyn MapView>>>>;

/// What capability `screen` pushed since the hooks last ran (API 22).
#[derive(Default)]
struct ScreenStore {
    /// The buttons this mod declared on the game's screens
    /// (`screen.set-buttons`), replaced wholesale by each call
    /// and clamped by it ([`SCREEN_BUTTONS`]); the game resolves
    /// them per open context ([`Mods::screen_buttons`]).
    buttons: Vec<ModScreenButton>,
    /// The slots this mod marks on the open screen
    /// (`screen.set-highlights`), replaced wholesale by each
    /// call and clamped by it ([`SCREEN_HIGHLIGHTS`]).
    highlights: Vec<ModHighlight>,
    /// What the player did on this mod's part of a screen,
    /// for `screen.poll`: the game pushes, the mod drains,
    /// capped at [`SCREEN_EVENTS`], oldest dropped.
    events: std::collections::VecDeque<ModScreenEvent>,
    /// This mod stands in for the game's debug overlay
    /// (`screen.claim-debug`): the aggregate is any-mod
    /// ([`Mods::debug_claimed`]), and a stopped mod's claim
    /// dies with its store, as its own-body ask does.
    debug_claimed: bool,
}
impl ScreenStore {
    fn push(&mut self, event: ModScreenEvent) {
        if self.events.len() == SCREEN_EVENTS {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }
}

/// What capability `map` holds for one mod (API 25): its
/// full-screen map and its corner minimap image, and the two
/// poll queues — the deaths and respawns the game pushed for
/// `poll-events`, and what the player did on its map screen
/// for `poll-map`. All of it is dropped with the mod's store
/// on stop, as every stateful surface's state is.
#[derive(Default)]
struct MapStore {
    /// The mod's full-screen map (`map.set-map-screen`), with
    /// the order it was set in and the game's view of it: the
    /// running mods' newest set is the one screen
    /// ([`Mods::map_screen`]), and the game writes its pans
    /// and zooms back into the winner's view
    /// ([`Mods::set_map_view`]). Markers and their labels are
    /// clamped by the host call ([`MAP_MARKERS`]).
    screen: Option<(u64, ModMapScreen, ModMapView)>,
    /// The mod's own minimap (`map.set-minimap-image`), with
    /// the order it was set in: the first loaded mod's shows
    /// ([`Mods::minimap_image`]), and the stamp tells the game
    /// when the shown one changed, for its upload.
    minimap_image: Option<(u64, ModMinimapImage)>,
    /// Deaths and respawns since the mod's last
    /// `map.poll-events`, capped at [`MAP_EVENTS`], oldest
    /// dropped, as `screen`'s events are.
    position_events: std::collections::VecDeque<ModPositionEvent>,
    /// What the player did on this mod's map screen since its
    /// last `map.poll-map`, capped as the position events.
    map_events: std::collections::VecDeque<ModMapEvent>,
}
impl MapStore {
    fn push_position(&mut self, event: ModPositionEvent) {
        if self.position_events.len() == MAP_EVENTS {
            self.position_events.pop_front();
        }
        self.position_events.push_back(event);
    }
    fn push_map(&mut self, event: ModMapEvent) {
        if self.map_events.len() == MAP_EVENTS {
            self.map_events.pop_front();
        }
        self.map_events.push_back(event);
    }
}

/// What capability `particles` holds for one mod (API 26):
/// its emitters, stepped by the game each tick, and the
/// tick's weighed spawn count — the mod's budget, shared by
/// its bursts and its emitters. All of it is dropped with
/// the mod's store on stop, as every stateful surface's
/// state is: a stopped mod's streams end with it.
#[derive(Default)]
struct ParticleStore {
    /// The mod's standing streams (`particles.set-emitters`),
    /// replaced wholesale by each call and sanitized by it
    /// ([`PARTICLE_EMITTERS`]).
    emitters: Vec<StoredEmitter>,
    /// (tick, weighed spawns spent in it): the first weigh of
    /// a new tick resets the count
    /// ([`State::weigh_particles`]).
    spent: (u64, usize),
}

/// One emitter a mod declared (API 26), with the game's step
/// of it: the fraction of a spawn the accumulator has earned
/// but not yet spent ([`advance_emitter`]).
struct StoredEmitter {
    emitter: ModEmitter,
    accumulator: f32,
}

/// What capability `chat-decoration` holds for one mod (API
/// 27): its two policies, each stamped with the order its set
/// happened in ([`CHAT_DECORATION_SEQ`]) so the aggregates
/// can run the latest-writer rule per record. No outputs, no
/// queues, no budget — a policy set is not an action; the
/// call replaces state, in the `set-hud` discipline, and the
/// store drops with the mod on stop, as every stateful
/// surface's state does.
#[derive(Default)]
struct ChatDecorationStore {
    /// The mod's line decoration (`chat-decoration.
    /// set-line-decoration`), sanitized by the call.
    line_decoration: Option<(u64, ModLineDecoration)>,
    /// The mod's chat animation (`chat-decoration.
    /// set-animation`), its durations clamped by the call
    /// ([`CHAT_ANIMATION_MS_MAX`]).
    animation: Option<(u64, ModChatAnimation)>,
}

/// What capability `render`'s `dynamic-light` holds for one
/// mod (API 28): its own registered sources, replaced
/// wholesale by each `set-sources` call, and its policy over
/// the game's own gather, stamped with the order its set
/// happened in ([`DYNAMIC_LIGHT_SEQ`]) so the aggregate can
/// run the tenancy rule. No outputs, no queues — a source is
/// standing state and a policy set is not an action; both
/// drop with the mod's store on stop, as every stateful
/// surface's state does.
#[derive(Default)]
struct DynamicLightStore {
    /// The mod's own sources (`dynamic-light.set-sources`),
    /// sanitized by the call ([`DYNAMIC_LIGHT_SOURCES`]).
    sources: Vec<ModLightSource>,
    /// The mod's policy (`dynamic-light.set-policy`),
    /// sanitized by the call: its glowing kinds cut and
    /// clamped, its three numbers clamped to their ranges.
    policy: Option<(u64, ModDynamicLightPolicy)>,
}

/// A stack match with its values parsed (WIT `item-model`, API
/// 23): a declaration's SNBT, run through the core SNBT parser
/// once, at set time, into the JSON shape the game's component
/// sets already have — so a hold is a value comparison against
/// a stack's merged components, never a re-parse at draw time.
/// A value that did not parse (or trailed past its value) is
/// `None`: it never holds, the sanitize-silently discipline of
/// `clamp_pose_offset` — the rule stands, its match is inert.
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedMatch {
    pub item: String,
    /// (component name, its parsed value) pairs, in declared order.
    pub components: Vec<(String, Option<serde_json::Value>)>,
}
impl ParsedMatch {
    fn parse(m: &ModModelMatch) -> ParsedMatch {
        ParsedMatch {
            item: m.item.clone(),
            components: m
                .components
                .iter()
                .map(|c| {
                    let value = horizon_client_core::component::parse_snbt(&c.value)
                        .ok()
                        .filter(|(_, used)| c.value[*used..].trim().is_empty())
                        .map(|(v, _)| v);
                    (c.name.clone(), value)
                })
                .collect(),
        }
    }
    /// The match holds for a stack of `item` whose merged
    /// components (defaults included) are `components`: the
    /// item is the stack's, and every named component is
    /// present with the parsed value.
    pub fn holds(
        &self,
        item: &str,
        components: &std::collections::BTreeMap<String, serde_json::Value>,
    ) -> bool {
        self.item == item && self.components_hold(components)
    }
    /// As [`ParsedMatch::holds`], but an empty item matches
    /// every item: a slot mark keyed on a component alone (a
    /// rarity, wherever it sits). Model rules never key this
    /// way — an empty item is dropped at set time there.
    pub fn holds_loose(
        &self,
        item: &str,
        components: &std::collections::BTreeMap<String, serde_json::Value>,
    ) -> bool {
        (self.item.is_empty() || self.item == item) && self.components_hold(components)
    }
    fn components_hold(
        &self,
        components: &std::collections::BTreeMap<String, serde_json::Value>,
    ) -> bool {
        self.components
            .iter()
            .all(|(name, want)| match (want, components.get(name)) {
                (Some(want), Some(got)) => value_eq(want, got),
                _ => false,
            })
    }
    /// The match as a test or a log names it: the item (or `*`
    /// for the loose any-item), then each component as
    /// `name=value` in SNBT.
    pub fn spelling(&self) -> String {
        let mut out = if self.item.is_empty() {
            "*".to_string()
        } else {
            self.item.clone()
        };
        for (name, value) in &self.components {
            out.push(' ');
            out.push_str(name);
            out.push('=');
            match value {
                Some(v) => out.push_str(&snbt_value(v)),
                None => out.push_str("<unparsed>"),
            }
        }
        out
    }
}

/// Component values compare as values (API 23): numbers across
/// their representations (an SNBT `64` and a prototype's `64`
/// are the one number, as a `1b` is), compounds key by key,
/// lists in order. Everything else is serde_json's equality.
fn value_eq(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    use serde_json::Value;
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            if let (Some(x), Some(y)) = (x.as_i64(), y.as_i64()) {
                x == y
            } else if let (Some(x), Some(y)) = (x.as_u64(), y.as_u64()) {
                x == y
            } else {
                match (x.as_f64(), y.as_f64()) {
                    (Some(x), Some(y)) => x == y,
                    _ => false,
                }
            }
        }
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(x, y)| value_eq(x, y))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, v)| y.get(k).is_some_and(|w| value_eq(v, w)))
        }
        _ => a == b,
    }
}

/// A parsed component value back in SNBT, for spellings: the
/// inverse of the set-time parse, deterministic (the JSON map
/// is key-ordered), close enough to read — never re-parsed.
pub fn snbt_value(value: &serde_json::Value) -> String {
    use serde_json::Value;
    match value {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => snbt_string(s),
        Value::Array(list) => {
            let parts: Vec<String> = list.iter().map(snbt_value).collect();
            format!("[{}]", parts.join(","))
        }
        Value::Object(map) => {
            let parts: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{}:{}", snbt_string(k), snbt_value(v)))
                .collect();
            format!("{{{}}}", parts.join(","))
        }
    }
}
fn snbt_string(s: &str) -> String {
    let plain = !s.is_empty()
        && s.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'+' | b'/' | b':')
        });
    if plain {
        s.to_string()
    } else {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

/// One slot mark as the game draws it (WIT `item-model`, API
/// 23): the mod's declaration with its stack match parsed, the
/// shape [`Mods::decorations`] hands over. The styles are the
/// WIT record's; only the target's match is resolved.
#[derive(Clone, Debug, PartialEq)]
pub struct ItemDecoration {
    pub target: ItemDecorationTarget,
    pub places: Vec<ModDecorationPlace>,
    pub fill: Option<u32>,
    pub border: Option<u32>,
    pub star: bool,
}
/// What a slot mark keys on, parsed (API 23).
#[derive(Clone, Debug, PartialEq)]
pub enum ItemDecorationTarget {
    Slot(u32),
    Stack(ParsedMatch),
}
impl ItemDecoration {
    fn parse(d: &ModDecoration) -> ItemDecoration {
        ItemDecoration {
            target: match &d.target {
                ModDecorationTarget::Slot(slot) => ItemDecorationTarget::Slot(*slot),
                ModDecorationTarget::Stack(m) => ItemDecorationTarget::Stack(ParsedMatch::parse(m)),
            },
            places: d.places.clone(),
            fill: d.fill,
            border: d.border,
            star: d.star,
        }
    }
    /// The mark as a test names it: its target, its places,
    /// then its styles in declaration order.
    pub fn spelling(&self) -> String {
        let target = match &self.target {
            ItemDecorationTarget::Slot(slot) => format!("slot {slot}"),
            ItemDecorationTarget::Stack(m) => format!("stack {}", m.spelling()),
        };
        let places: Vec<&str> = self
            .places
            .iter()
            .map(|p| match p {
                ModDecorationPlace::Hotbar => "hotbar",
                ModDecorationPlace::Screens => "screens",
            })
            .collect();
        let mut out = format!("{target} @ {}", places.join(","));
        if let Some(fill) = self.fill {
            out.push_str(&format!(" fill {fill:06x}"));
        }
        if let Some(border) = self.border {
            out.push_str(&format!(" border {border:06x}"));
        }
        if self.star {
            out.push_str(" star");
        }
        out
    }
}

/// The running mods' model rules as one evaluation index (WIT
/// `item-model`, API 23): every rule sanitized and parsed,
/// ordered for the game's one question — the first rule that
/// holds, newest-loaded mod first, each mod's list in its
/// declared order. Rebuilt only when a store's rules change
/// ([`Mods::item_model_index`]), so a resolve pays one set
/// lookup for the prefilter and walks rules only for a ruled
/// item.
#[derive(Clone, Debug, Default)]
pub struct ItemModelIndex {
    /// (match, contexts, model) in evaluation order.
    rules: Vec<(ParsedMatch, Vec<ModDisplayContext>, String)>,
    /// The item ids any rule keys on: the swap's prefilter.
    ruled: std::collections::HashSet<String>,
}
impl ItemModelIndex {
    /// Builds the index from each mod's stored rules, in load
    /// order: the evaluation order is its reverse, mod by mod.
    pub fn from_mod_rules<'a>(
        mods: impl IntoIterator<Item = &'a [ModModelRule]>,
    ) -> ItemModelIndex {
        let mods: Vec<&[ModModelRule]> = mods.into_iter().collect();
        let mut index = ItemModelIndex::default();
        for rules in mods.iter().rev() {
            for rule in *rules {
                index.ruled.insert(rule.match_.item.clone());
                index.rules.push((
                    ParsedMatch::parse(&rule.match_),
                    rule.contexts.clone(),
                    rule.model.clone(),
                ));
            }
        }
        index
    }
    /// No running mod declares a rule.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }
    /// The prefilter: any rule keys on this item at all.
    pub fn rules_item(&self, item: &str) -> bool {
        self.ruled.contains(item)
    }
    /// The definition a stack displays as, or None for its own:
    /// the first rule in evaluation order whose contexts name
    /// `group`, whose match holds against the stack's merged
    /// `components`, and whose model names a definition that
    /// exists (a rule naming a missing one is inert, never the
    /// missing model — the walk goes on past it).
    pub fn evaluate(
        &self,
        item: &str,
        components: &std::collections::BTreeMap<String, serde_json::Value>,
        group: ModDisplayContext,
        definition_exists: impl Fn(&str) -> bool,
    ) -> Option<&str> {
        if !self.rules_item(item) {
            return None;
        }
        self.rules
            .iter()
            .find(|(m, contexts, model)| {
                contexts.contains(&group) && m.holds(item, components) && definition_exists(model)
            })
            .map(|(_, _, model)| model.as_str())
    }
    /// The rules in evaluation order, for tests and logs.
    pub fn rules(&self) -> impl Iterator<Item = (&ParsedMatch, &[ModDisplayContext], &str)> {
        self.rules
            .iter()
            .map(|(m, contexts, model)| (m, contexts.as_slice(), model.as_str()))
    }
    /// One rule as a test names it: its match, its model, its
    /// contexts.
    pub fn rule_spelling(rule: (&ParsedMatch, &[ModDisplayContext], &str)) -> String {
        let (m, contexts, model) = rule;
        let contexts: Vec<&str> = contexts
            .iter()
            .map(|c| match c {
                ModDisplayContext::FirstPerson => "first-person",
                ModDisplayContext::ThirdPerson => "third-person",
                ModDisplayContext::Gui => "gui",
                ModDisplayContext::Ground => "ground",
                ModDisplayContext::Fixed => "fixed",
                ModDisplayContext::Head => "head",
            })
            .collect();
        format!("{} -> {model} @ {}", m.spelling(), contexts.join(","))
    }
}

/// A mod's text panel, drawn every frame until it changes.
#[derive(Clone, Debug)]
pub struct Panel {
    pub corner: Corner,
    pub lines: Vec<Line>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    /// Installed and turned off by the player.
    Off,
    Running,
    /// Turned on, and run by another game: a server mod in the window while
    /// the world runs on its server thread.
    Elsewhere,
    /// Could not load or start, or stopped while running.
    Failed(String),
}

/// One installed mod, for the Mods page.
pub struct Entry {
    pub path: PathBuf,
    /// None when the manifest could not be read (`status` says why).
    pub manifest: Option<Manifest>,
    pub status: Status,
    /// The mod's average time per call of its busiest hook.
    pub cost: Duration,
    run: Option<Running>,
    modified: Option<SystemTime>,
}
impl Entry {
    /// The running mod has a settings page (capability `ui` set widgets
    /// with `ui.set-settings`, or `input` registered keys the player may
    /// bind there): its row in Options > Mods gains a button.
    ///
    /// Declaring the `ui` capability counts from load: a mod only pushes
    /// its widgets in `ui_settings_opened`, which fires when the page
    /// opens, so waiting for widgets first would hide the button forever.
    /// A mod with no `ui` capability, no keys and no widgets still has
    /// none; opening the page still bails when it stays empty.
    pub fn has_ui_settings(&self) -> bool {
        self.run.as_ref().is_some_and(|r| {
            let d = r.store.data();
            !d.ui.settings.is_empty()
                || !d.input.keys.is_empty()
                || self
                    .manifest
                    .as_ref()
                    .is_some_and(|m| m.capabilities.contains(&Capability::Ui))
        })
    }
}

struct Running {
    store: Store<State>,
    /// The mod's `ui-events` export; none for a mod built before `ui`, or a
    /// server mod.
    ui: Option<UiHost>,
    /// The mod's `audio-events` export (API 15); none for a mod built
    /// before it, or a server mod.
    audio: Option<AudioHost>,
    hooks: Hooks,
    /// It imports `game.raycast`.
    raycasts: bool,
    /// It imports capability `recipes`.
    reads_recipes: bool,
    /// It imports capability `inventory`.
    reads_inventory: bool,
    /// It imports capability `entity`.
    reads_entities: bool,
    /// It imports `render.set-world`.
    reads_world_shapes: bool,
    /// It imports `game.get-effects` or `game.get-self-state` (API 18).
    reads_self: bool,
    /// It imports capability `actions` (API 21).
    reads_actions: bool,
    /// It imports capability `screen` (API 22).
    reads_screen: bool,
    /// It imports capability `inventory-actions` (API 24).
    reads_inventory_actions: bool,
    /// It imports capability `map` (API 25): the position
    /// events are pushed to its queue.
    reads_map: bool,
    /// It imports `map.tiles` or `map.tile-versions` (API 25):
    /// the game keeps its map cache warm for it.
    reads_map_tiles: bool,
    /// It imports `map.context` or `map.last-death` (API 25):
    /// the snapshot needs its map fields.
    reads_map_snapshot: bool,
}

/// A running mod's exports and the hooks its `init` asked for.
enum Hooks {
    Client(HorizonMod, Registration),
    Server(HorizonServerMod, ServerRegistration),
}
impl Hooks {
    fn client(&self) -> Option<&Registration> {
        match self {
            Hooks::Client(_, r) => Some(r),
            Hooks::Server(..) => None,
        }
    }
    fn server(&self) -> Option<&ServerRegistration> {
        match self {
            Hooks::Server(_, r) => Some(r),
            Hooks::Client(..) => None,
        }
    }
    fn commands(&self) -> Vec<&str> {
        match self {
            Hooks::Client(_, r) => r.commands.iter().map(String::as_str).collect(),
            Hooks::Server(_, r) => r.commands.iter().map(|c| c.name.as_str()).collect(),
        }
    }
}

/// What capability `ui` pushed since the hooks last ran.
#[derive(Default)]
struct UiState {
    /// The screen `ui.open-screen` last pushed, until `ui.close-screen`.
    screen: Option<UiScreen>,
    /// The widgets `ui.set-settings` last pushed (the mod's page in
    /// Options > Mods).
    settings: Vec<UiWidget>,
    /// The calls since the last hook ended, for [`Output`].
    open: bool,
    close: bool,
}

/// Capability `ui`'s panel beside container screens (`browser`).
#[derive(Default)]
struct BrowserState {
    items: Vec<BrowserStack>,
    /// Which `set-items` or `set-recipes` call the panel is from, unique
    /// across mods and reloads, so the game converts them once.
    version: u64,
    /// `set-recipes`' title and recipes, shown instead of the items.
    recipes: Option<RecipePage>,
    events: std::collections::VecDeque<BrowserEvent>,
}
/// What `browser.set-recipes` shows: a title, tabs (`tab` the selected
/// one) and the recipes.
#[derive(Clone, Debug)]
pub struct RecipePage {
    pub title: String,
    pub tabs: Vec<BrowserStack>,
    pub tab: u32,
    pub recipes: Vec<BrowserRecipe>,
}
static BROWSER_VERSION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
/// The order `hand.set-offset` calls happened in, across mods (API
/// 16): there is one hand, so the newest stamp wins.
static HAND_OFFSET_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
/// The order `hand.set-swing-scale` calls happened in, across mods
/// (API 17): there is one swing, so the newest stamp wins, as with
/// the offset.
static HAND_SWING_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
/// The order `entity-pose` writes happened in, across mods (API
/// 20): one writer owns a target's pose, so the newest stamp wins,
/// as with the hand's offset.
static POSE_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
/// The order `item-model.set-dropped-display` calls happened in,
/// across mods (API 23): one policy applies to every dropped
/// item, so the newest stamp wins, as with the hand's offset.
static ITEM_DISPLAY_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
/// The order `map`'s sets happened in, across mods (API 25):
/// one map screen shows, so the newest `set-map-screen` stamp
/// wins, as with the dropped display; a `set-minimap-image`
/// stamp tells the game when the shown image changed.
static MAP_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
/// The order `chat-decoration` sets happened in, across mods
/// (API 27): the game's chat carries one decoration and one
/// animation, so the newest stamp of each record wins, as
/// with the hand's offset.
static CHAT_DECORATION_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
/// The order `dynamic-light.set-policy` calls happened in,
/// across mods (API 28): one policy stands over the game's
/// own gather, so the newest stamp wins, as with the chat
/// decoration.
static DYNAMIC_LIGHT_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
/// Items a panel may hold, and events kept for `browser.poll`.
const BROWSER_ITEMS: usize = 8192;
/// `recipes.place` and `give` calls a hook call may make.
const RECIPE_ACTIONS: usize = 36;
/// Recipes `set-recipes` may show, slots in one, and stacks a slot cycles.
const BROWSER_RECIPES: usize = 256;
const RECIPE_SLOTS: usize = 9;
const RECIPE_TABS: usize = 16;
const SLOT_STACKS: usize = 64;
const BROWSER_EVENTS: usize = 256;
impl BrowserState {
    fn push(&mut self, event: BrowserEvent) {
        if self.events.len() == BROWSER_EVENTS {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }
}

struct State {
    name: String,
    id: String,
    snapshot: Arc<Snapshot>,
    out: Vec<Output>,
    panel: Option<Panel>,
    ui: UiState,
    /// Capability `render`'s HUD shapes, drawn until replaced.
    shapes: Vec<Shape>,
    /// Capability `render`'s world shapes, drawn until replaced.
    world_shapes: Vec<WorldShape>,
    /// Capability `render`'s corner panel.
    corner: Option<CornerPanel>,
    /// Capability `render`'s zoom, when the mod zooms.
    zoom: Option<ModZoom>,
    /// Capability `render`'s minimap, when the mod shows one.
    minimap: Option<ModMinimap>,
    /// Capability `render`'s far terrain, when the mod draws one.
    far_terrain: Option<ModFarTerrain>,
    /// Capability `render`'s gamma, when the mod sets one.
    gamma: Option<f32>,
    /// Capability `render`'s distance-fog flag, when the mod sets one.
    fog: Option<bool>,
    /// Capability `render`'s camera override, when the mod sets one.
    camera: Option<ModCameraOverride>,
    /// Capability `render`'s HUD hide (API 19): while set, the game
    /// hides the HUD as F1 does ([`Mods::hide_hud`]).
    hide_hud: bool,
    /// Capability `render`'s hand FOV (API 19), kept sanitized by the
    /// host call: the first running mod that sets one wins
    /// ([`Mods::hand_fov`]).
    hand_fov: Option<ModHandFov>,
    /// Capability `render`'s frame-rate cap (API 19), kept sanitized
    /// to 1..=240 by the host call: the lowest running mod's cap
    /// applies ([`Mods::fps_cap`]).
    fps_cap: Option<u32>,
    /// Capability `audio`'s shared reverb character (wet gain, decay
    /// seconds), when the mod sets one (API 15).
    sound_env: Option<(f32, f32)>,
    /// The first-person hand's state the game last pushed (API 16):
    /// [`Mods::set_hand_state`] writes it into every running mod's
    /// store before the frame hooks, and `hand.get-state` reads it.
    /// The zero state until the first push.
    hand_state: ModHandState,
    /// This mod's hand offset (API 16) and the order it was set in:
    /// the running mod with the newest stamp drives the one hand
    /// ([`Mods::hand_offset`]). Kept clamped by the host call.
    hand_offset: Option<(u64, ModHandTransform)>,
    /// The swing animation the game last pushed (API 17):
    /// [`Mods::set_swing_anim`] writes it into every running mod's
    /// store next to the hand state, and `hand.get-swing-anim`
    /// reads it. `None` until the first push.
    swing_anim: ModSwingAnim,
    /// This mod's swing scale (API 17) and the order it was set in:
    /// the running mod with the newest stamp sets how much of the
    /// vanilla swing pose applies ([`Mods::swing_scale`]). Kept
    /// clamped by the host call.
    swing_scale: Option<(u64, f32)>,
    /// Player models' pose states the game last pushed (API 20):
    /// [`Mods::set_pose_states`] writes them into every running
    /// mod's store before the frame hooks, and
    /// `entity-pose.get-pose-states` reads them. Empty until the
    /// first push.
    pose_states: Vec<ModEntityPoseState>,
    /// This mod's pose offsets (API 20), per target, each with the
    /// order it was set in: one writer owns a target's pose wholly
    /// ([`Mods::pose_offset`]). Kept clamped by the host call.
    pose_offsets: std::collections::HashMap<PoseKey, (u64, ModPoseOffset)>,
    /// This mod's pose scales (API 20), per target, each with the
    /// order it was set in: resolved with the offsets, from the
    /// same writer ([`Mods::pose_offset`]). Kept clamped by the
    /// host call.
    pose_scales: std::collections::HashMap<PoseKey, (u64, f32)>,
    /// Whether this mod shows the player's own body in first
    /// person (API 20): any running mod asking is enough
    /// ([`Mods::own_body`]). The flag is the mod's own store's, so
    /// a stopped mod's ask dies with it.
    own_body: bool,
    /// The buttons this mod holds down (API 21): `(attack, use)`
    /// from its last `actions.set-held`, read by the aggregate
    /// ([`Mods::held_buttons`]). State, not a request — set in
    /// place by the host call, and a stopped mod's hold dies with
    /// its store, as its pose offsets do.
    held: (bool, bool),
    /// This mod's hold is latched off (API 21): a screen opened
    /// (or the world was left) while it held, so the aggregate
    /// skips it until the mod calls `set-held` again — a hold
    /// never resumes silently ([`Mods::release_held_buttons`]).
    held_latched: bool,
    /// Capability `screen`'s declarations and events (API 22):
    /// the buttons and highlights this mod put on the game's
    /// screens, the presses the game queued for it, and its
    /// debug claim. A stopped mod's declarations die with its
    /// store, as its held buttons do.
    screen: ScreenStore,
    /// Capability `map`'s surfaces and queues (API 25): the
    /// mod's full-screen map and minimap image, and what the
    /// game pushed for its two polls. A stopped mod's map
    /// dies with its store, as its screen declarations do.
    map: MapStore,
    /// Capability `particles`' emitters and budget (API 26):
    /// the mod's standing streams and the tick's weighed
    /// spawn count. A stopped mod's streams die with its
    /// store, as its map does.
    particles: ParticleStore,
    /// Capability `chat-decoration`'s two policies (API 27):
    /// how the game's own chat is decorated and animated when
    /// this mod's declarations stand. A stopped mod's styling
    /// ends with its store, as its emitters do.
    chat_decoration: ChatDecorationStore,
    /// Capability `render`'s `dynamic-light` declarations
    /// (API 28): the mod's own light sources and its policy
    /// over the game's own gather. A stopped mod's lights
    /// end with its store, as its styling does.
    dynamic_light: DynamicLightStore,
    /// The tick the game last began ([`Mods::tick`]): the
    /// particle budget keys its count on it, so a call
    /// between ticks weighs against the last tick's spends.
    tick: u64,
    /// The wheel turns over the open screen the game observed
    /// for this mod (capability `inventory-actions`, API 24),
    /// oldest first, capped like `screen`'s events: a turn the
    /// mod never polls for is not kept forever.
    wheel: std::collections::VecDeque<ModWheelEvent>,
    /// This mod's model rules (WIT `item-model`, API 23),
    /// sanitized at set time by the host call; the running
    /// mods' rules are indexed together for the game's swap
    /// ([`Mods::item_model_index`]). `model_rules_version`
    /// moves on every set, so the index rebuilds only then.
    model_rules: Vec<ModModelRule>,
    model_rules_version: u64,
    /// This mod's slot marks (WIT `item-model`, API 23), parsed
    /// at set time by the host call: the aggregate is the union
    /// in load order ([`Mods::decorations`]).
    item_decorations: Vec<ItemDecoration>,
    /// This mod's dropped-item display (WIT `item-model`, API
    /// 23) and the order it was set in: the running mod with
    /// the newest stamp sets the one policy
    /// ([`Mods::dropped_display`]).
    dropped_display: Option<(u64, ModDroppedDisplay)>,
    /// The movement toggle options this mod last asked to set in the
    /// current call (API 18): `Mods::call` moves it into an
    /// [`Output::SetMovementToggles`] when the call ends, as it does
    /// the ui open/close flags.
    movement_toggles: Option<(Option<bool>, Option<bool>)>,
    /// How many times this mod called `set-camera`: the game re-reads
    /// the look whenever the winning mod's count moves, so a mod
    /// calling `set-camera` every tick with fixed angles keeps the
    /// mouse from drifting it, while a mod that calls only on change
    /// leaves the mouse look alone.
    camera_seq: u64,
    /// The strongest gamma / fog-off the other running mods asked for
    /// when this mod's current call started, so `get-gamma` / `get-fog`
    /// read the effective value, not just this mod's own (`Mods::call`
    /// refreshes them before every call).
    aggregate_gamma: Option<f32>,
    aggregate_fog_off: bool,
    /// The camera the first running mod drives when this mod's current
    /// call started (`Mods::call` refreshes it before every call), so
    /// `get-camera` / `project` read the effective camera even before
    /// this mod sets anything of its own.
    aggregate_camera: Option<ModCameraOverride>,
    /// Whether `aggregate_camera` is this mod's own override: `set-camera`
    /// keeps the aggregate live within a call only when this mod is (or
    /// becomes) the winner, and `Mods::call` refreshes it before every
    /// call.
    aggregate_is_mine: bool,
    input: InputState,
    browser: BrowserState,
    /// Capability `storage`'s folder; made on first `set`.
    data_dir: PathBuf,
    /// Bytes `storage::set` may still write before this hook call ends
    /// (reset per call in [`Mods::call`]): fuel bounds a mod's CPU time but
    /// not disk I/O time, so a loop of small-fuel, large-payload `set`s
    /// needs its own budget.
    storage_budget: u64,
    limits: StoreLimits,
    world: WorldSlot,
    /// The map view lent for the current calls (API 25),
    /// shared by every mod's store, as [`State::world`] is.
    map_view: MapSlot,
    /// Whether this mod runs where the world is simulated: only a server
    /// mod may change blocks (`world.set-blocks`); a client mod reads only.
    server: bool,
}

/// The installed mods and the engine running them. `Mods::default()` is
/// the state with none: no engine, and every hook a no-op.
#[derive(Default)]
pub struct Mods {
    engine: Option<(Engine, Linker<State>)>,
    entries: Vec<Entry>,
    snapshot: Arc<Snapshot>,
    out: Vec<Output>,
    world: WorldSlot,
    /// The lent map view (API 25), shared by every mod's
    /// store, as [`Mods::world`] is.
    map: MapSlot,
    /// Moves on whenever a mod starts or stops: the game resends the server
    /// mods' commands to players when it changes.
    generation: u64,
    /// The item-model evaluation index (API 23), rebuilt only
    /// when a running mod's rules change: the per-entry rule
    /// versions it was built from, and the index itself.
    item_model_index: Mutex<Option<(Vec<Option<u64>>, Arc<ItemModelIndex>)>>,
}

/// A `Send` handle to a running [`Mods`] for the sound engine's hook
/// (API 15): the engine owns the hook and calls it from inside the
/// game's own audio calls, where the borrow checker cannot see that
/// the game's audio and mods are disjoint fields, so the game hands
/// over a raw pointer rather than a reference.
///
/// The contract is the game's: it builds the handle from `&mut Mods`,
/// refreshes it whenever mods load and on every sound tick, calls
/// [`SoundHookHandle::adjust`] only on the game thread while the game
/// runs, and the `Mods` outlives the handle (both are owned by the
/// same session object). The unsafe stays in this crate so the game's
/// crate roots can forbid unsafe code.
pub struct SoundHookHandle {
    mods: *mut Mods,
}

// SAFETY: the handle is only a pointer the game dereferences on its
// own thread under the contract above; the `Mods` itself never moves
// between threads through it.
unsafe impl Send for SoundHookHandle {}

impl SoundHookHandle {
    pub fn new(mods: &mut Mods) -> Self {
        SoundHookHandle { mods }
    }

    /// Ask the sound mod about one world sound; see
    /// [`Mods::adjust_sound`].
    pub fn adjust(&mut self, query: &ModSoundQuery) -> ModSoundAdjustment {
        // SAFETY: the game's contract on the type: the pointer came
        // from its `&mut Mods`, is refreshed on every sound tick, and
        // this call happens on the game thread while the game runs.
        let mods = unsafe { &mut *self.mods };
        mods.adjust_sound(query)
    }
}

impl Mods {
    /// Changes whenever the running mods (so their commands) change.
    pub fn generation(&self) -> u64 {
        self.generation
    }
    /// Whether a running mod may use capability `world`: only then is a
    /// view worth lending.
    pub fn wants_world(&self) -> bool {
        self.entries.iter().any(|e| {
            e.status == Status::Running
                && e.manifest
                    .as_ref()
                    .is_some_and(|m| m.capabilities.contains(&crate::Capability::World))
        })
    }
    /// Lends the game's world to capability `world` for the calls until
    /// [`Mods::take_world`]; with none lent, `get-block` finds nothing and
    /// `set-blocks` checks only its count.
    pub fn lend_world(&mut self, view: Box<dyn WorldView>) {
        if let Ok(mut slot) = self.world.lock() {
            *slot = Some(view);
        }
    }
    /// The view [`Mods::lend_world`] lent, back to the game.
    pub fn take_world(&mut self) -> Option<Box<dyn WorldView>> {
        self.world.lock().ok()?.take()
    }
    /// Lends the game's map cache to capability `map` for the
    /// calls until [`Mods::take_map`]; with none lent,
    /// `map.tiles` finds nothing.
    pub fn lend_map(&mut self, view: Box<dyn MapView>) {
        if let Ok(mut slot) = self.map.lock() {
            *slot = Some(view);
        }
    }
    /// The view [`Mods::lend_map`] lent, back to the game.
    pub fn take_map(&mut self) -> Option<Box<dyn MapView>> {
        self.map.lock().ok()?.take()
    }

    /// Reads every mod at `paths` and starts those `enabled` says are on,
    /// client and server mods alike. A command that `reserved` (vanilla's
    /// commands) or an earlier mod already uses is refused.
    pub fn load(
        paths: &[PathBuf],
        enabled: impl Fn(&str) -> bool,
        reserved: impl Fn(&str) -> bool,
    ) -> Mods {
        Mods::load_for(Runs::Both, paths, enabled, reserved)
    }
    /// [`Mods::load`], starting only the mods `runs` says this game runs; the
    /// others that are on are [`Status::Elsewhere`].
    pub fn load_for(
        runs: Runs,
        paths: &[PathBuf],
        enabled: impl Fn(&str) -> bool,
        reserved: impl Fn(&str) -> bool,
    ) -> Mods {
        let mut mods = Mods::default();
        for path in paths {
            let modified = modified(path);
            let mut entry = Entry {
                path: path.clone(),
                manifest: None,
                status: Status::Off,
                cost: Duration::ZERO,
                run: None,
                modified,
            };
            match Package::read(path) {
                Err(e) => entry.status = Status::Failed(e),
                Ok(package) => {
                    let on = enabled(&package.manifest.id);
                    entry.manifest = Some(package.manifest.clone());
                    if on && !runs.starts(package.manifest.side) {
                        entry.status = Status::Elsewhere;
                    } else if on {
                        // Content is read before the world loads; a mod whose
                        // content is refused stays off, with the reason.
                        let content = crate::Content::read(path, &package.manifest);
                        entry.status =
                            match content.and_then(|_| mods.start(path, &package, &reserved)) {
                                Ok(run) => {
                                    entry.run = run;
                                    Status::Running
                                }
                                Err(e) => Status::Failed(e),
                            };
                    }
                }
            }
            if let (Status::Failed(reason), Some(m)) = (&entry.status, &entry.manifest) {
                log::warn!("Horizon: mod {} did not start: {reason}", m.id);
            }
            mods.entries.push(entry);
        }
        mods
    }

    fn engine(&mut self) -> Result<&(Engine, Linker<State>), String> {
        if self.engine.is_none() {
            let mut config = Config::new();
            config.wasm_component_model(true).consume_fuel(true);
            let engine = Engine::new(&config).map_err(|e| e.to_string())?;
            let mut linker = Linker::new(&engine);
            HorizonMod::add_to_linker::<State, HasSelf<State>>(&mut linker, |s| s)
                .map_err(|e| e.to_string())?;
            players::add_to_linker::<State, HasSelf<State>>(&mut linker, |s| s)
                .map_err(|e| e.to_string())?;
            // No `mod_world::add_to_linker` here: `horizon-mod` imports
            // `world` too, so `HorizonMod::add_to_linker` already defined
            // it; a second definition stops every mod from starting.
            UiHost::add_to_linker::<State, HasSelf<State>>(&mut linker, |s| s)
                .map_err(|e| e.to_string())?;
            AudioHost::add_to_linker::<State, HasSelf<State>>(&mut linker, |s| s)
                .map_err(|e| e.to_string())?;
            RenderHost::add_to_linker::<State, HasSelf<State>>(&mut linker, |s| s)
                .map_err(|e| e.to_string())?;
            RecipesHost::add_to_linker::<State, HasSelf<State>>(&mut linker, |s| s)
                .map_err(|e| e.to_string())?;
            self.engine = Some((engine, linker));
        }
        Ok(self.engine.as_ref().expect("made above"))
    }

    /// Starts a mod's code; `Ok(None)` for a content-only mod, which has
    /// nothing to run (the game reads its content before the world loads).
    /// `path` is the mod's own `.kmod` or folder, for capability `storage`'s
    /// per-mod directory.
    fn start(
        &mut self,
        path: &Path,
        package: &Package,
        reserved: &dyn Fn(&str) -> bool,
    ) -> Result<Option<Running>, String> {
        let m = &package.manifest;
        let Some(wasm) = &package.wasm else {
            return Ok(None);
        };
        // Two running mods with the same id would share one `data_dir`
        // (capability `storage`): the first loaded keeps the id.
        if self
            .entries
            .iter()
            .any(|e| e.run.is_some() && e.manifest.as_ref().is_some_and(|other| other.id == m.id))
        {
            return Err(format!("another running mod already uses id {}", m.id));
        }
        let server = m.side == Side::Server;
        let taken: Vec<String> = self
            .entries
            .iter()
            .filter_map(|e| e.run.as_ref())
            .filter(|r| r.hooks.server().is_some() == server)
            .flat_map(|r| r.hooks.commands())
            .map(str::to_owned)
            .collect();
        let snapshot = self.snapshot.clone();
        let world = self.world.clone();
        let map_view = self.map.clone();
        let (engine, linker) = self.engine()?;
        let component =
            Component::new(engine, wasm).map_err(|e| format!("not a component: {e}"))?;
        let ty = component.component_type();
        let imports: Vec<String> = ty
            .imports(engine)
            .map(|(name, _)| name.to_string())
            .collect();
        m.check_imports(imports.iter().map(String::as_str))?;
        // A function this Horizon doesn't have: say so, not wasmtime's linking error.
        let functions = crate::api_level::imports(wasm)?;
        let (level, function) = crate::api_level::needed(functions.iter().map(String::as_str));
        if let Some(why) = crate::api_level::too_new(level, function) {
            return Err(why);
        }
        let data_dir = path
            .parent()
            .unwrap_or(Path::new("."))
            .join(crate::manifest::STORAGE_DIR)
            .join(&m.id);
        let state = State {
            name: m.name.clone(),
            id: m.id.clone(),
            snapshot,
            out: Vec::new(),
            panel: None,
            ui: UiState::default(),
            shapes: Vec::new(),
            world_shapes: Vec::new(),
            corner: None,
            zoom: None,
            minimap: None,
            far_terrain: None,
            gamma: None,
            fog: None,
            camera: None,
            hide_hud: false,
            hand_fov: None,
            fps_cap: None,
            sound_env: None,
            hand_state: zero_hand_state(),
            hand_offset: None,
            swing_anim: ModSwingAnim::None,
            swing_scale: None,
            pose_states: Vec::new(),
            pose_offsets: std::collections::HashMap::new(),
            pose_scales: std::collections::HashMap::new(),
            own_body: false,
            held: (false, false),
            held_latched: false,
            screen: ScreenStore::default(),
            map: MapStore::default(),
            particles: ParticleStore::default(),
            chat_decoration: ChatDecorationStore::default(),
            dynamic_light: DynamicLightStore::default(),
            tick: 0,
            wheel: std::collections::VecDeque::new(),
            model_rules: Vec::new(),
            model_rules_version: 0,
            item_decorations: Vec::new(),
            dropped_display: None,
            movement_toggles: None,
            camera_seq: 0,
            aggregate_camera: None,
            aggregate_is_mine: false,
            aggregate_gamma: None,
            aggregate_fog_off: false,
            input: InputState::default(),
            browser: BrowserState::default(),
            data_dir,
            storage_budget: STORAGE_WRITE_BUDGET,
            limits: StoreLimitsBuilder::new()
                .memory_size(MEMORY)
                .instances(16)
                .build(),
            world,
            map_view,
            server,
        };
        let mut store = Store::new(engine, state);
        store.limiter(|s| &mut s.limits);
        store.set_fuel(FUEL_INIT).map_err(|e| e.to_string())?;
        let instance = linker
            .instantiate(&mut store, &component)
            .map_err(|e| reason(&e))?;
        let (mut hooks, ui, audio) = if server {
            let exports = HorizonServerMod::new(&mut store, &instance).map_err(|e| reason(&e))?;
            let r = exports
                .horizon_mods_server_events()
                .call_init(&mut store)
                .map_err(|e| reason(&e))??;
            (Hooks::Server(exports, r), None, None)
        } else {
            let exports = HorizonMod::new(&mut store, &instance).map_err(|e| reason(&e))?;
            // `ui-events` is a mod built since `ui`'s; older mods have no export
            // and stay callable through `events` alone.
            let ui = UiHost::new(&mut store, &instance).ok();
            // Same for `audio-events` (API 15): no export, no sounds.
            let audio = AudioHost::new(&mut store, &instance).ok();
            let r = exports
                .horizon_mods_events()
                .call_init(&mut store)
                .map_err(|e| reason(&e))??;
            (Hooks::Client(exports, r), ui, audio)
        };
        let free = |c: &str| {
            let free = !reserved(c) && !taken.iter().any(|t| t == c);
            if !free {
                log::warn!("Horizon: mod {} cannot take /{c}: already a command", m.id);
            }
            free
        };
        match &mut hooks {
            Hooks::Client(_, r) => r.commands.retain(|c| free(c)),
            Hooks::Server(_, r) => {
                r.commands.retain(|c| free(&c.name));
                for c in &mut r.commands {
                    if let Err(e) = check_arguments(&c.arguments) {
                        log::warn!(
                            "Horizon: mod {}'s /{}: {e}; it takes free text",
                            m.id,
                            c.name
                        );
                        c.arguments.clear();
                    }
                }
            }
        }
        self.generation += 1;
        self.out.append(&mut store.data_mut().out);
        Ok(Some(Running {
            store,
            ui,
            audio,
            hooks,
            raycasts: functions.iter().any(|f| f == "game.raycast"),
            reads_recipes: functions.iter().any(|f| f.starts_with("recipes.")),
            reads_inventory: functions.iter().any(|f| f.starts_with("inventory.")),
            reads_entities: functions.iter().any(|f| f.starts_with("entity.")),
            reads_world_shapes: functions.iter().any(|f| f == "render.set-world"),
            reads_self: functions
                .iter()
                .any(|f| f == "game.get-effects" || f == "game.get-self-state"),
            reads_actions: functions.iter().any(|f| f.starts_with("actions.")),
            reads_screen: functions.iter().any(|f| f.starts_with("screen.")),
            reads_inventory_actions: functions
                .iter()
                .any(|f| f.starts_with("inventory-actions.")),
            reads_map: functions.iter().any(|f| f.starts_with("map.")),
            reads_map_tiles: functions
                .iter()
                .any(|f| f == "map.tiles" || f == "map.tile-versions"),
            reads_map_snapshot: functions
                .iter()
                .any(|f| f == "map.context" || f == "map.last-death"),
        }))
    }

    /// True when at least one mod runs: the game skips its mod work otherwise.
    pub fn is_running(&self) -> bool {
        self.entries.iter().any(|e| e.run.is_some())
    }
    /// True when a server mod runs: the server skips its mod work otherwise.
    pub fn runs_server(&self) -> bool {
        self.entries
            .iter()
            .any(|e| e.run.as_ref().is_some_and(|r| r.hooks.server().is_some()))
    }
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }
    /// What mods read until the next snapshot.
    pub fn set_snapshot(&mut self, snapshot: Snapshot) {
        self.snapshot = Arc::new(snapshot);
    }

    /// Calls `f` on every running mod whose hooks `wants`, with `fuel`.
    fn each(
        &mut self,
        fuel: u64,
        wants: impl Fn(&Hooks) -> bool,
        mut f: impl FnMut(&mut Running) -> wasmtime::Result<()>,
    ) {
        for i in 0..self.entries.len() {
            if self.entries[i]
                .run
                .as_ref()
                .is_some_and(|r| wants(&r.hooks))
            {
                self.call(i, fuel, &mut f);
            }
        }
    }
    /// [`Mods::each`] over the client mods.
    fn each_client(
        &mut self,
        fuel: u64,
        wants: impl Fn(&Registration) -> bool,
        mut f: impl FnMut(&ClientEvents, &mut Store<State>) -> wasmtime::Result<()>,
    ) {
        self.each(
            fuel,
            |h| h.client().is_some_and(&wants),
            |run| match &run.hooks {
                Hooks::Client(m, _) => f(m.horizon_mods_events(), &mut run.store),
                Hooks::Server(..) => Ok(()),
            },
        );
    }
    /// [`Mods::each`] over the server mods.
    fn each_server(
        &mut self,
        fuel: u64,
        wants: impl Fn(&ServerRegistration) -> bool,
        mut f: impl FnMut(&ServerEvents, &mut Store<State>) -> wasmtime::Result<()>,
    ) {
        self.each(
            fuel,
            |h| h.server().is_some_and(&wants),
            |run| match &run.hooks {
                Hooks::Server(m, _) => f(m.horizon_mods_server_events(), &mut run.store),
                Hooks::Client(..) => Ok(()),
            },
        );
    }

    /// One call into mod `i`; a trap or an overrun turns the mod off.
    fn call<R>(
        &mut self,
        i: usize,
        fuel: u64,
        f: impl FnOnce(&mut Running) -> wasmtime::Result<R>,
    ) -> Option<R> {
        // The aggregate as the call starts, from the other running mods,
        // so the mod's getters read the effective value even before it
        // sets anything of its own.
        let (aggregate_gamma, aggregate_fog_off) = (self.gamma_except(i), self.fog_off_except(i));
        // The camera aggregate is the winning override, whoever holds
        // it — including this mod, when it already does.
        let aggregate_camera = self.camera().map(|(camera, _)| camera);
        let aggregate_is_mine = self.camera_winner() == Some(i);
        let entry = &mut self.entries[i];
        let run = entry.run.as_mut()?;
        run.store.data_mut().snapshot = self.snapshot.clone();
        run.store.data_mut().aggregate_camera = aggregate_camera;
        run.store.data_mut().aggregate_is_mine = aggregate_is_mine;
        run.store.data_mut().aggregate_gamma = aggregate_gamma;
        run.store.data_mut().aggregate_fog_off = aggregate_fog_off;
        run.store.data_mut().storage_budget = STORAGE_WRITE_BUDGET;
        let started = Instant::now();
        let result = run.store.set_fuel(fuel).and_then(|()| f(run));
        // An average over about the last 64 calls.
        entry.cost = (entry.cost * 63 + started.elapsed()) / 64;
        let data = run.store.data_mut();
        let (open, close) = (
            std::mem::take(&mut data.ui.open),
            std::mem::take(&mut data.ui.close),
        );
        let movement_toggles = data.movement_toggles.take();
        if let Some((sprint, sneak)) = movement_toggles {
            self.out.push(Output::SetMovementToggles { sprint, sneak });
        }
        self.out.append(&mut data.out);
        if open {
            self.out.push(Output::OpenScreen { owner: i });
        }
        if close {
            self.out.push(Output::CloseScreen { owner: i });
        }
        match result {
            Ok(r) => Some(r),
            Err(e) => {
                let reason = reason(&e);
                log::warn!("Horizon: mod {} stopped: {reason}", run.store.data().id);
                let name = run.store.data().name.clone();
                self.out.push(Output::Stopped {
                    name,
                    reason: reason.clone(),
                });
                entry.status = Status::Failed(reason);
                entry.run = None;
                self.generation += 1;
                None
            }
        }
    }

    /// A game tick.
    pub fn tick(&mut self, tick: u64) {
        // The tick's stamp lands before the hooks: the
        // particle budget (API 26) keys its per-tick count
        // on it, and a call between ticks weighs against
        // the last tick's spends.
        for run in self.entries.iter_mut().filter_map(|e| e.run.as_mut()) {
            run.store.data_mut().tick = tick;
        }
        self.each_client(FUEL_TICK, |h| h.tick, |m, s| m.call_tick(s, tick));
    }
    /// A rendered frame, `seconds` after the last.
    pub fn frame(&mut self, seconds: f32) {
        self.each_client(FUEL_FRAME, |h| h.frame, |m, s| m.call_frame(s, seconds));
    }
    /// A chat line the player received, as plain text.
    pub fn chat(&mut self, text: &str) {
        self.each_client(FUEL_EVENT, |h| h.chat, |m, s| m.call_chat(s, text));
    }
    /// Whether a running client mod owns `/name`.
    pub fn has_command(&self, name: &str) -> bool {
        self.command_owner(false, name).is_some()
    }
    fn command_owner(&self, server: bool, name: &str) -> Option<usize> {
        self.entries.iter().position(|e| {
            e.run.as_ref().is_some_and(|r| {
                r.hooks.server().is_some() == server && r.hooks.commands().contains(&name)
            })
        })
    }
    /// Runs `line` (without its slash) if a client mod owns its command;
    /// false when none does.
    pub fn command(&mut self, line: &str) -> bool {
        let (name, args) = line.split_once(' ').unwrap_or((line, ""));
        let Some(i) = self.command_owner(false, name) else {
            return false;
        };
        let result = self.call(i, FUEL_EVENT, |run| match &run.hooks {
            Hooks::Client(m, _) => m
                .horizon_mods_events()
                .call_command(&mut run.store, name, args),
            Hooks::Server(..) => Ok(Ok(None)),
        });
        match result {
            Some(Ok(Some(text))) => self.out.push(Output::Feedback { text, error: false }),
            Some(Err(text)) => self.out.push(Output::Feedback { text, error: true }),
            Some(Ok(None)) | None => {}
        }
        true
    }
    /// The commands running client mods own, for suggestions.
    pub fn commands(&self) -> impl Iterator<Item = &str> {
        self.owned_commands(false)
    }
    fn owned_commands(&self, server: bool) -> impl Iterator<Item = &str> {
        self.entries
            .iter()
            .filter_map(|e| e.run.as_ref())
            .filter(move |r| r.hooks.server().is_some() == server)
            .flat_map(|r| r.hooks.commands())
    }

    /// A server tick.
    pub fn server_tick(&mut self, tick: u64) {
        self.each_server(FUEL_TICK, |h| h.tick, |m, s| m.call_tick(s, tick));
    }
    /// `player` joined the server (they are in the snapshot's players).
    pub fn player_joined(&mut self, player: &ServerPlayer) {
        self.each_server(
            FUEL_EVENT,
            |h| h.joins,
            |m, s| m.call_player_joined(s, player),
        );
    }
    /// `player` left the server (they are no longer in the snapshot's players).
    pub fn player_left(&mut self, player: &ServerPlayer) {
        self.each_server(
            FUEL_EVENT,
            |h| h.joins,
            |m, s| m.call_player_left(s, player),
        );
    }
    /// A chat message `player` sent, as plain text.
    pub fn player_chat(&mut self, player: &ServerPlayer, text: &str) {
        self.each_server(FUEL_EVENT, |h| h.chat, |m, s| m.call_chat(s, player, text));
    }
    /// `player` broke `state` at `pos`.
    pub fn block_broken(&mut self, player: &ServerPlayer, pos: BlockPos, state: &str) {
        self.each_server(
            FUEL_EVENT,
            |h| h.blocks,
            |m, s| m.call_block_broken(s, player, pos, state),
        );
    }
    /// `player` placed `state` at `pos`.
    pub fn block_placed(&mut self, player: &ServerPlayer, pos: BlockPos, state: &str) {
        self.each_server(
            FUEL_EVENT,
            |h| h.blocks,
            |m, s| m.call_block_placed(s, player, pos, state),
        );
    }
    /// The commands running server mods own and their arguments, for the
    /// server's command tree.
    pub fn server_commands(&self) -> impl Iterator<Item = &ServerCommand> {
        self.entries
            .iter()
            .filter_map(|e| e.run.as_ref()?.hooks.server())
            .flat_map(|r| r.commands.iter())
    }
    /// Runs `line` (without its slash), typed by `player`, if a server mod
    /// owns its command; its feedback is an [`Output::Reply`]. False when
    /// none does.
    pub fn server_command(&mut self, player: &ServerPlayer, line: &str) -> bool {
        let (name, args) = line.split_once(' ').unwrap_or((line, ""));
        let Some(i) = self.command_owner(true, name) else {
            return false;
        };
        let reply = |text, error| Output::Reply {
            player: player.id.clone(),
            text,
            error,
        };
        let arguments = self.entries[i]
            .run
            .as_ref()
            .and_then(|r| r.hooks.server())
            .and_then(|r| r.commands.iter().find(|c| c.name == name))
            .map(|c| c.arguments.as_slice())
            .unwrap_or_default();
        let values = match crate::commands::parse(arguments, args, player, &self.snapshot.players) {
            Ok(values) => values,
            Err(e) => {
                self.out.push(reply(e, true));
                return true;
            }
        };
        let result = self.call(i, FUEL_EVENT, |run| match &run.hooks {
            Hooks::Server(m, _) => m.horizon_mods_server_events().call_command(
                &mut run.store,
                player,
                name,
                args,
                &values,
            ),
            Hooks::Client(..) => Ok(Ok(None)),
        });
        match result {
            Some(Ok(Some(text))) => self.out.push(reply(text, false)),
            Some(Err(text)) => self.out.push(reply(text, true)),
            Some(Ok(None)) | None => {}
        }
        true
    }
    /// The running mods' HUD panels.
    pub fn panels(&self) -> impl Iterator<Item = &Panel> {
        self.entries
            .iter()
            .filter_map(|e| e.run.as_ref()?.store.data().panel.as_ref())
    }
    /// The running mods' corner panels (capability `render`), in load order.
    pub fn corner_panels(&self) -> impl Iterator<Item = &CornerPanel> {
        self.entries
            .iter()
            .filter_map(|e| e.run.as_ref()?.store.data().corner.as_ref())
    }
    /// Some running mod calls `game.raycast`: the snapshot needs a
    /// [`Snapshot::view_hit`].
    pub fn wants_raycast(&self) -> bool {
        self.entries
            .iter()
            .any(|e| e.run.as_ref().is_some_and(|r| r.raycasts))
    }
    /// The panel beside container screens (`browser.set-items`): the first
    /// running mod with items or recipes, its index, the panel's version and
    /// the items.
    pub fn browser_items(&self) -> Option<(usize, u64, &[BrowserStack])> {
        self.entries.iter().enumerate().find_map(|(i, e)| {
            let b = &e.run.as_ref()?.store.data().browser;
            (!b.items.is_empty() || b.recipes.is_some()).then_some((
                i,
                b.version,
                b.items.as_slice(),
            ))
        })
    }
    /// What mod `owner`'s panel shows instead of its items
    /// (`browser.set-recipes`).
    pub fn browser_recipes(&self, owner: usize) -> Option<&RecipePage> {
        let b = &self.entries.get(owner)?.run.as_ref()?.store.data().browser;
        b.recipes.as_ref()
    }
    /// The player did `event` on mod `owner`'s panel.
    pub fn browser_event(&mut self, owner: usize, event: BrowserEvent) {
        if let Some(run) = self.entries.get_mut(owner).and_then(|e| e.run.as_mut()) {
            run.store.data_mut().browser.push(event);
        }
    }
    /// The key `key` went down with the pointer on `stack` in a container
    /// screen: each mod with a key on it hears its own name for it.
    pub fn browser_key(&mut self, key: &str, stack: &BrowserStack) {
        for run in self.entries.iter_mut().filter_map(|e| e.run.as_mut()) {
            let data = run.store.data_mut();
            let names: Vec<String> = data
                .input
                .keys
                .iter()
                .filter(|k| k.key == key)
                .map(|k| k.name.clone())
                .collect();
            for key in names {
                data.browser.push(BrowserEvent::Key(BrowserItemKey {
                    key,
                    stack: stack.clone(),
                }));
            }
        }
    }
    /// Some running mod reads capability `recipes`: the snapshot needs
    /// [`Snapshot::recipes`] and [`Snapshot::known_recipes`].
    pub fn wants_recipes(&self) -> bool {
        self.entries
            .iter()
            .any(|e| e.run.as_ref().is_some_and(|r| r.reads_recipes))
    }
    /// Some running mod reads capability `inventory`: the snapshot needs
    /// [`Snapshot::inventory`] and [`Snapshot::open_container`].
    pub fn wants_inventory(&self) -> bool {
        self.entries
            .iter()
            .any(|e| e.run.as_ref().is_some_and(|r| r.reads_inventory))
    }
    /// Some running mod reads capability `entity`: the snapshot needs
    /// [`Snapshot::entities`] and [`Snapshot::targeted`].
    pub fn wants_entities(&self) -> bool {
        self.entries
            .iter()
            .any(|e| e.run.as_ref().is_some_and(|r| r.reads_entities))
    }
    /// Some running mod reads the local player's own effects or
    /// self-state (API 18): the snapshot needs [`Snapshot::effects`]
    /// and [`Snapshot::self_state`].
    pub fn wants_self_state(&self) -> bool {
        self.entries
            .iter()
            .any(|e| e.run.as_ref().is_some_and(|r| r.reads_self))
    }
    /// Some running mod imports capability `actions` (API 21):
    /// the snapshot needs [`Snapshot::action_target`].
    pub fn wants_actions(&self) -> bool {
        self.entries
            .iter()
            .any(|e| e.run.as_ref().is_some_and(|r| r.reads_actions))
    }
    /// Some running mod imports capability `screen` (API 22):
    /// the snapshot needs [`Snapshot::screen_state`] and
    /// [`Snapshot::trades`].
    pub fn wants_screen(&self) -> bool {
        self.entries
            .iter()
            .any(|e| e.run.as_ref().is_some_and(|r| r.reads_screen))
    }
    /// Some running mod imports capability `inventory-actions`
    /// (API 24): the game observes the wheel over its screens
    /// only then.
    pub fn wants_inventory_actions(&self) -> bool {
        self.entries
            .iter()
            .any(|e| e.run.as_ref().is_some_and(|r| r.reads_inventory_actions))
    }
    /// Some running mod imports `map.context` or
    /// `map.last-death` (API 25): the snapshot needs
    /// [`Snapshot::map_context`] and [`Snapshot::last_death`].
    pub fn wants_map(&self) -> bool {
        self.entries
            .iter()
            .any(|e| e.run.as_ref().is_some_and(|r| r.reads_map_snapshot))
    }
    /// Some running mod imports `map.tiles` or
    /// `map.tile-versions` (API 25): the game keeps its map
    /// cache warm for it and lends the cache to its calls.
    pub fn wants_map_tiles(&self) -> bool {
        self.entries
            .iter()
            .any(|e| e.run.as_ref().is_some_and(|r| r.reads_map_tiles))
    }
    /// Some running mod has a `frame` hook: the game refreshes the snapshot
    /// every frame for it.
    pub fn wants_frames(&self) -> bool {
        self.entries.iter().any(|e| {
            e.run
                .as_ref()
                .is_some_and(|r| r.hooks.client().is_some_and(|h| h.frame))
        })
    }
    /// Some running mod claimed items (capability `input`).
    pub fn claims_any(&self) -> bool {
        self.inputs().any(|i| !i.claimed.is_empty())
    }
    /// Some running mod claimed the item `id`.
    pub fn claims(&self, id: &str) -> bool {
        self.inputs().any(|i| i.claimed.iter().any(|c| c == id))
    }
    /// Some running mod registered a key.
    pub fn has_keys(&self) -> bool {
        self.inputs().any(|i| !i.keys.is_empty())
    }
    /// The mods' keys: (mod name, key name, label, key), for Controls.
    pub fn mod_keys(&self) -> impl Iterator<Item = (&str, &ModKey)> {
        self.entries
            .iter()
            .filter_map(|e| e.run.as_ref())
            .flat_map(|r| {
                let d = r.store.data();
                d.input.keys.iter().map(move |k| (d.name.as_str(), k))
            })
    }
    /// Mod `i`'s keys, as it registered them (with the player's choices).
    pub fn keys_of(&self, i: usize) -> &[ModKey] {
        self.entries
            .get(i)
            .and_then(|e| e.run.as_ref())
            .map_or(&[], |r| r.store.data().input.keys.as_slice())
    }
    /// The keys the player chose (`ui`'s key widget), by mod id: each
    /// running mod's `name → key`. They replace the defaults registered
    /// so far and later ones. Keys that aren't keys are skipped.
    pub fn set_bindings<'a>(&mut self, chosen: impl Fn(&str) -> Vec<(&'a str, &'a str)>) {
        for e in &mut self.entries {
            let (Some(m), Some(run)) = (&e.manifest, e.run.as_mut()) else {
                continue;
            };
            let input = &mut run.store.data_mut().input;
            input.bindings = chosen(&m.id)
                .into_iter()
                .filter(|(_, key)| key_ok(key))
                .map(|(n, k)| (n.to_owned(), k.to_owned()))
                .collect();
            for (name, key) in input.bindings.clone() {
                input.rebind(&name, key);
            }
        }
    }
    /// The player binds mod `i`'s key `name` to `key` (`key.keyboard.x`,
    /// `key.mouse.4`, `key.keyboard.unknown` for none). False when the mod
    /// has no such key or `key` isn't one.
    pub fn rebind(&mut self, i: usize, name: &str, key: &str) -> bool {
        let Some(run) = self.entries.get_mut(i).and_then(|e| e.run.as_mut()) else {
            return false;
        };
        let input = &mut run.store.data_mut().input;
        if !key_ok(key) || !input.keys.iter().any(|k| k.name == name) {
            return false;
        }
        input.bindings.retain(|(n, _)| n != name);
        input.bindings.push((name.to_owned(), key.to_owned()));
        input.rebind(name, key.to_owned());
        true
    }
    /// Mod `i`'s keys back to the defaults they registered, forgetting the
    /// player's choices; live, so a held key lets go first. False when the
    /// mod isn't running.
    pub fn reset_keys(&mut self, i: usize) -> bool {
        let Some(run) = self.entries.get_mut(i).and_then(|e| e.run.as_mut()) else {
            return false;
        };
        run.store.data_mut().input.reset_keys();
        true
    }
    /// Every other running mod's keys: (mod name, key), for the duplicate
    /// warnings on the Mods screens. The caller's own index is skipped, so
    /// a mod's keys never warn against themselves.
    pub fn other_mod_keys(&self, skip: usize) -> impl Iterator<Item = (&str, &ModKey)> {
        self.entries
            .iter()
            .enumerate()
            .filter(move |(i, _)| *i != skip)
            .filter_map(|(_, e)| e.run.as_ref())
            .flat_map(|r| {
                let d = r.store.data();
                d.input.keys.iter().map(move |k| (d.name.as_str(), k))
            })
    }
    /// The use button went down with `item`: `use-start` to each mod that
    /// claimed it.
    pub fn use_start(&mut self, item: &HeldItem) {
        let event = InputEvent::UseStart(ItemUse {
            item: item.clone(),
            ticks: 0,
        });
        self.push_input(|i| i.claimed.contains(&item.id), &event);
    }
    /// The claimed use ended after `ticks`.
    pub fn use_stop(&mut self, item: &HeldItem, ticks: u32) {
        let event = InputEvent::UseStop(ItemUse {
            item: item.clone(),
            ticks,
        });
        self.push_input(|i| i.claimed.contains(&item.id), &event);
    }
    /// The key `key` (`key.keyboard.r`) went down or up: each mod with a
    /// key on it hears its own name for it.
    pub fn key(&mut self, key: &str, down: bool) {
        for run in self.entries.iter_mut().filter_map(|e| e.run.as_mut()) {
            let input = &mut run.store.data_mut().input;
            let names: Vec<String> = input
                .keys
                .iter()
                .filter(|k| k.key == key)
                .map(|k| k.name.clone())
                .collect();
            for name in names {
                let held = input.down.iter().position(|n| *n == name);
                match (down, held) {
                    (true, None) => input.down.push(name.clone()),
                    (false, Some(i)) => {
                        input.down.swap_remove(i);
                    }
                    // Already let go when a screen opened.
                    (false, None) => continue,
                    (true, Some(_)) => {}
                }
                input.push(if down {
                    InputEvent::KeyDown(name)
                } else {
                    InputEvent::KeyUp(name)
                });
            }
        }
    }
    /// A screen opened, or the game stopped playing: every mod key held
    /// lets go (`key-up`), so a key needs a new press afterwards.
    pub fn release_keys(&mut self) {
        for run in self.entries.iter_mut().filter_map(|e| e.run.as_mut()) {
            let input = &mut run.store.data_mut().input;
            for name in std::mem::take(&mut input.down) {
                input.push(InputEvent::KeyUp(name));
            }
        }
    }
    /// Some running mod holds the mouse wheel.
    pub fn wheel_claimed(&self) -> bool {
        self.inputs().any(|i| i.wheel)
    }
    /// Wheel steps while playing: `wheel` to the mods holding it; false
    /// when none does (the hotbar takes them).
    pub fn wheel(&mut self, steps: f32) -> bool {
        let event = InputEvent::Wheel(steps);
        let claimed = self.wheel_claimed();
        self.push_input(|i| i.wheel, &event);
        claimed
    }
    /// The strongest zoom a running mod asks for (capability `render`).
    pub fn zoom(&self) -> Option<ModZoom> {
        self.entries
            .iter()
            .filter_map(|e| e.run.as_ref()?.store.data().zoom)
            .max_by(|a, b| a.factor.total_cmp(&b.factor))
    }
    /// The minimap the first running mod shows (capability `render`).
    pub fn minimap(&self) -> Option<ModMinimap> {
        self.entries
            .iter()
            .find_map(|e| e.run.as_ref()?.store.data().minimap)
    }
    /// The far terrain the first running mod draws (capability `render`).
    pub fn far_terrain(&self) -> Option<ModFarTerrain> {
        self.entries
            .iter()
            .find_map(|e| e.run.as_ref()?.store.data().far_terrain)
    }
    /// Whether any running mod hides the HUD (capability `render`,
    /// API 19): the flag is the mod's own store's, so a stopped mod's
    /// ask dies with it, as its fog and gamma do.
    pub fn hide_hud(&self) -> bool {
        self.entries
            .iter()
            .filter_map(|e| e.run.as_ref())
            .any(|r| r.store.data().hide_hud)
    }
    /// The hand FOV the first running mod set (capability `render`,
    /// API 19): one renderer-wide property two mods cannot compose,
    /// as with the minimap. A stopped mod's setting goes with it.
    pub fn hand_fov(&self) -> Option<ModHandFov> {
        self.entries
            .iter()
            .find_map(|e| e.run.as_ref()?.store.data().hand_fov)
    }
    /// The lowest frame-rate cap a running mod set (capability
    /// `render`, API 19): caps only ever lower the rate, so the
    /// minimum is the aggregate. A stopped mod's cap goes with it.
    pub fn fps_cap(&self) -> Option<u32> {
        self.entries
            .iter()
            .filter_map(|e| e.run.as_ref()?.store.data().fps_cap)
            .min()
    }
    /// The camera the first running mod drives (capability `render`),
    /// with the call count of the `set-camera` that set it.
    pub fn camera(&self) -> Option<(ModCameraOverride, u64)> {
        self.entries.iter().find_map(|e| {
            let data = e.run.as_ref()?.store.data();
            data.camera.map(|camera| (camera, data.camera_seq))
        })
    }
    /// The first running mod that hears sounds (capability `audio`,
    /// API 15): the only one ever asked, as with the camera.
    fn sound_winner(&self) -> Option<usize> {
        self.entries
            .iter()
            .position(|e| e.run.as_ref().is_some_and(|run| run.audio.is_some()))
    }

    /// Ask the sound mod about one world sound. No sound mod, a trap
    /// or an out-of-fuel call all mean no change, and a failed call
    /// never disables the mod.
    pub fn adjust_sound(&mut self, query: &ModSoundQuery) -> ModSoundAdjustment {
        const NO_CHANGE: ModSoundAdjustment = ModSoundAdjustment {
            gain: 1.0,
            lowpass: 0.0,
            reverb: 0.0,
        };
        let Some(i) = self.sound_winner() else {
            return NO_CHANGE;
        };
        let query = query.clone();
        self.call(i, FUEL_SOUND, |run| {
            run.audio
                .as_ref()
                .expect("the winner has the export")
                .horizon_mods_audio_events()
                .call_sound(&mut run.store, &query)
        })
        .unwrap_or(NO_CHANGE)
    }

    /// The reverb character the sound mod set, if it set one.
    pub fn sound_environment(&self) -> Option<(f32, f32)> {
        let run = self.entries.get(self.sound_winner()?)?.run.as_ref()?;
        run.store.data().sound_env
    }

    /// The first-person hand's state for this frame (API 16): the
    /// game pushes it once per rendered frame, before the frame
    /// hooks run, and a mod reads it with `hand.get-state` in any
    /// hook. Written into the running mods' stores at once, as
    /// `browser_event` writes its events: the frame hooks that
    /// follow in the same frame read this push, not a stale one.
    pub fn set_hand_state(&mut self, state: ModHandState) {
        for run in self.entries.iter_mut().filter_map(|e| e.run.as_mut()) {
            run.store.data_mut().hand_state = state.clone();
        }
    }
    /// Back to the zero hand state (API 16): the game calls this
    /// while the hand is not being posed (the title screen, third
    /// person, sleeping), so a mod never reads a stale frame as the
    /// current one. The pushed swing animation (API 17) resets to
    /// none with it: it rides the same push.
    pub fn clear_hand_state(&mut self) {
        for run in self.entries.iter_mut().filter_map(|e| e.run.as_mut()) {
            run.store.data_mut().hand_state = zero_hand_state();
            run.store.data_mut().swing_anim = ModSwingAnim::None;
        }
    }
    /// The hand offset the game applies at pose time (API 16): the
    /// one the running mods set most recently with
    /// `hand.set-offset`, already clamped by the host call. None
    /// when no running mod sets one; a stopped mod's offset is
    /// dropped with its store, as its camera and gamma are.
    pub fn hand_offset(&self) -> Option<ModHandTransform> {
        self.entries
            .iter()
            .filter_map(|e| e.run.as_ref()?.store.data().hand_offset.as_ref())
            .max_by_key(|(seq, _)| *seq)
            .map(|(_, offset)| *offset)
    }

    /// The swing animation for this frame (API 17): the game pushes
    /// it with the hand state, once per rendered frame before the
    /// frame hooks run, and a mod reads it with
    /// `hand.get-swing-anim`. Written into the running mods' stores
    /// at once, as the hand state is.
    pub fn set_swing_anim(&mut self, anim: ModSwingAnim) {
        for run in self.entries.iter_mut().filter_map(|e| e.run.as_mut()) {
            run.store.data_mut().swing_anim = anim;
        }
    }
    /// How much of the vanilla swing pose applies (API 17): the
    /// scale the running mods set most recently with
    /// `hand.set-swing-scale`, already clamped by the host call.
    /// 1.0 — vanilla in full — when no running mod sets one; a
    /// stopped mod's scale is dropped with its store, as its
    /// offset is.
    pub fn swing_scale(&self) -> f32 {
        self.entries
            .iter()
            .filter_map(|e| e.run.as_ref()?.store.data().swing_scale.as_ref())
            .max_by_key(|(seq, _)| *seq)
            .map_or(1.0, |(_, scale)| *scale)
    }

    /// Player models' pose states for this frame (API 20): the
    /// game pushes them once per rendered frame, before the frame
    /// hooks run, and a mod reads them with
    /// `entity-pose.get-pose-states` in any hook. Written into the
    /// running mods' stores at once, as the hand state is.
    pub fn set_pose_states(&mut self, states: Vec<ModEntityPoseState>) {
        for run in self.entries.iter_mut().filter_map(|e| e.run.as_mut()) {
            run.store.data_mut().pose_states.clone_from(&states);
        }
    }
    /// Back to no pose states (API 20): the game calls this while
    /// no player model is posed (the title screen), so a mod never
    /// reads a stale frame as the current one.
    pub fn clear_pose_states(&mut self) {
        for run in self.entries.iter_mut().filter_map(|e| e.run.as_mut()) {
            run.store.data_mut().pose_states.clear();
        }
    }
    /// A target's pose shift (API 20): the offset and scale the
    /// game folds into that player's pose, already clamped by the
    /// host calls. One writer owns a target's pose wholly: the
    /// running mod that wrote to the target most recently — by
    /// either write — supplies both its offset (an empty one if it
    /// set only a scale) and its scale (1.0 if it set only an
    /// offset), exactly as two camera mods cannot share a camera.
    /// None when no running mod wrote to the target; a stopped
    /// mod's writes are dropped with its store, as its hand
    /// offset is.
    pub fn pose_offset(&self, target: ModPoseTarget) -> Option<(ModPoseOffset, f32)> {
        let target = PoseKey::from(target);
        let mut best: Option<(u64, ModPoseOffset, f32)> = None;
        for data in self
            .entries
            .iter()
            .filter_map(|e| e.run.as_ref())
            .map(|r| r.store.data())
        {
            let offset = data.pose_offsets.get(&target);
            let scale = data.pose_scales.get(&target);
            let stamp = match (offset, scale) {
                (Some((o, _)), Some((s, _))) => (*o).max(*s),
                (Some((o, _)), None) => *o,
                (None, Some((s, _))) => *s,
                (None, None) => continue,
            };
            if best.as_ref().is_none_or(|(b, _, _)| stamp > *b) {
                best = Some((
                    stamp,
                    offset.map_or_else(
                        || ModPoseOffset {
                            parts: Vec::new(),
                            hidden: Vec::new(),
                        },
                        |(_, o)| o.clone(),
                    ),
                    scale.map_or(1.0, |(_, s)| *s),
                ));
            }
        }
        best.map(|(_, offset, scale)| (offset, scale))
    }
    /// Whether any running mod shows the player's own body in
    /// first person (API 20): the flag is the mod's own store's,
    /// so a stopped mod's ask dies with it, as its HUD hide does.
    pub fn own_body(&self) -> bool {
        self.entries
            .iter()
            .filter_map(|e| e.run.as_ref())
            .any(|r| r.store.data().own_body)
    }

    /// The buttons the running mods hold down (capability
    /// `actions`, API 21), `(attack, use)`: any unlatched holder
    /// holds the button, as any own-body ask shows the body. The
    /// game ORs these with the player's physical buttons where it
    /// reads them. A stopped mod's hold dies with its store.
    pub fn held_buttons(&self) -> (bool, bool) {
        let mut held = (false, false);
        for data in self
            .entries
            .iter()
            .filter_map(|e| e.run.as_ref())
            .map(|r| r.store.data())
        {
            if !data.held_latched {
                held.0 |= data.held.0;
                held.1 |= data.held.1;
            }
        }
        held
    }
    /// Latches every running mod's hold off (API 21): the game
    /// calls this as a screen opens or the world is left, beside
    /// `input`'s release of the mods' keys. A latched hold reads
    /// as released in [`Mods::held_buttons`] until its mod calls
    /// `set-held` again — it must opt back in, never resumes
    /// silently.
    pub fn release_held_buttons(&mut self) {
        for run in self.entries.iter_mut().filter_map(|e| e.run.as_mut()) {
            let data = run.store.data_mut();
            if data.held != (false, false) {
                data.held_latched = true;
            }
        }
    }

    /// The buttons the running mods declared for one open
    /// context (capability `screen`, API 22): screen `screen`
    /// with menu `menu` (none: a screen without one, or a mod's
    /// menu-less button matches any). Mods in load order, each
    /// in its declared order, the first eight across all of
    /// them — the aggregate the game draws and hit-tests, so
    /// the two always agree. Each is paired with its owner's
    /// entry index, which is where its presses go
    /// ([`Mods::screen_event`]).
    pub fn screen_buttons(
        &self,
        screen: &str,
        menu: Option<&str>,
    ) -> Vec<(usize, ModScreenButton)> {
        let mut out = Vec::new();
        for (i, data) in self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(i, e)| e.run.as_ref().map(|r| (i, r.store.data())))
        {
            for button in &data.screen.buttons {
                if button.screen == screen && button.menu.as_deref().is_none_or(|m| Some(m) == menu)
                {
                    out.push((i, button.clone()));
                    if out.len() == 8 {
                        return out;
                    }
                }
            }
        }
        out
    }
    /// The slots the running mods mark on the open screen
    /// (capability `screen`, API 22), every mod's list in load
    /// order: the union the game draws. A stopped mod's marks
    /// go with its store.
    pub fn screen_highlights(&self) -> Vec<ModHighlight> {
        let mut out = Vec::new();
        for data in self
            .entries
            .iter()
            .filter_map(|e| e.run.as_ref())
            .map(|r| r.store.data())
        {
            out.extend(data.screen.highlights.iter().copied());
        }
        out
    }
    /// The player did `event` on mod `owner`'s part of a
    /// screen (API 22): queued for its `screen.poll`, the
    /// `browser_event` shape.
    pub fn screen_event(&mut self, owner: usize, event: ModScreenEvent) {
        if let Some(run) = self.entries.get_mut(owner).and_then(|e| e.run.as_mut()) {
            run.store.data_mut().screen.push(event);
        }
    }
    /// The middle button went down over the open
    /// inventory/container screen (API 22): every running
    /// `screen` mod hears it, with the slot under the pointer
    /// or none. Observed only — the press itself is the game's
    /// to use as it always has.
    pub fn screen_middle_click(&mut self, slot: Option<u32>) {
        for run in self
            .entries
            .iter_mut()
            .filter_map(|e| e.run.as_mut())
            .filter(|r| r.reads_screen)
        {
            run.store
                .data_mut()
                .screen
                .push(ModScreenEvent::MiddleClick(slot));
        }
    }
    /// The wheel turned over the open inventory or container
    /// screen (capability `inventory-actions`, API 24): every
    /// running `inventory-actions` mod hears it, with the slot
    /// under the pointer or none. Observed only — the turn
    /// itself is the game's to use as it always has. Each mod's
    /// queue keeps at most [`WHEEL_EVENTS`], the oldest dropped,
    /// as `screen`'s events do.
    pub fn push_wheel(&mut self, event: ModWheelEvent) {
        for run in self
            .entries
            .iter_mut()
            .filter_map(|e| e.run.as_mut())
            .filter(|r| r.reads_inventory_actions)
        {
            let wheel = &mut run.store.data_mut().wheel;
            if wheel.len() >= WHEEL_EVENTS {
                wheel.pop_front();
            }
            wheel.push_back(event);
        }
    }
    /// Whether any running mod stands in for the game's debug
    /// overlay (capability `screen`, API 22): while one claims
    /// it, the game does not draw its own debug entries. A
    /// stopped mod's claim dies with its store, as its
    /// own-body ask does.
    pub fn debug_claimed(&self) -> bool {
        self.entries
            .iter()
            .filter_map(|e| e.run.as_ref())
            .any(|r| r.store.data().screen.debug_claimed)
    }

    /// The player died or came back (capability `map`, API
    /// 25): every running `map` mod hears it, in the order the
    /// game pushes them. Each mod's queue keeps at most
    /// [`MAP_EVENTS`], the oldest dropped, as `screen`'s
    /// events do.
    pub fn push_position_event(&mut self, event: ModPositionEvent) {
        for run in self
            .entries
            .iter_mut()
            .filter_map(|e| e.run.as_mut())
            .filter(|r| r.reads_map)
        {
            run.store.data_mut().map.push_position(event.clone());
        }
    }
    /// What the player did on mod `owner`'s map screen (API
    /// 25): queued for its `map.poll-map`, the
    /// [`Mods::screen_event`] shape.
    pub fn push_map_event(&mut self, owner: usize, event: ModMapEvent) {
        if let Some(run) = self.entries.get_mut(owner).and_then(|e| e.run.as_mut()) {
            run.store.data_mut().map.push_map(event);
        }
    }
    /// The full-screen map to show (capability `map`, API
    /// 25): the running mod that called `set-map-screen` most
    /// recently owns the one screen — its entry index, the
    /// set's stamp, its image and markers, and the game's
    /// view of it. None when no running mod shows one; a
    /// stopped mod's map goes with its store.
    pub fn map_screen(&self) -> Option<(usize, u64, ModMapScreen, ModMapView)> {
        let mut best: Option<(usize, u64, ModMapScreen, ModMapView)> = None;
        for (i, data) in self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(i, e)| e.run.as_ref().map(|r| (i, r.store.data())))
        {
            if let Some((seq, screen, view)) = &data.map.screen
                && best.as_ref().is_none_or(|(_, b, _, _)| *seq > *b)
            {
                best = Some((i, *seq, screen.clone(), *view));
            }
        }
        best
    }
    /// The game's view of mod `owner`'s map screen (API 25):
    /// a pan or a zoom writes back into the winner's store,
    /// so `map.poll-map`'s view events and the game's own
    /// reading of the view agree. A store that no longer
    /// holds a screen (the mod cleared it, or stopped) takes
    /// nothing.
    pub fn set_map_view(&mut self, owner: usize, view: ModMapView) {
        if let Some(run) = self.entries.get_mut(owner).and_then(|e| e.run.as_mut())
            && let Some((_, _, held)) = &mut run.store.data_mut().map.screen
        {
            *held = view;
        }
    }
    /// The minimap image to show (capability `map`, API 25):
    /// the first loaded running mod that set one — its entry
    /// index, the set's stamp and the image. None when no
    /// running mod shows one; a stopped mod's image goes with
    /// its store, as its map screen does.
    pub fn minimap_image(&self) -> Option<(usize, u64, ModMinimapImage)> {
        self.entries.iter().enumerate().find_map(|(i, e)| {
            let (seq, image) = e.run.as_ref()?.store.data().map.minimap_image.as_ref()?;
            Some((i, *seq, image.clone()))
        })
    }

    /// The running mods' emitters (capability `particles`,
    /// API 26): every running mod's stored streams in load
    /// order, as (entry index, emitter) pairs. The game steps
    /// them after the tick's hooks: it resolves each anchor
    /// against the drawn world and asks
    /// [`Mods::particle_emitter_spawns`] for the spawns it may
    /// draw. A stopped mod's streams are gone with its store.
    pub fn particle_emitters(&self) -> Vec<(usize, ModEmitter)> {
        self.entries
            .iter()
            .enumerate()
            .filter_map(|(i, e)| e.run.as_ref().map(|r| (i, r.store.data())))
            .flat_map(|(i, data)| {
                data.particles
                    .emitters
                    .iter()
                    .map(move |stored| (i, stored.emitter.clone()))
            })
            .collect()
    }
    /// Steps mod `owner`'s emitter `index` one tick (API 26):
    /// advances its accumulator by its rate and answers how
    /// many whole spawns this tick may draw — each weighed
    /// against the mod's budget as a burst is, so an emitter
    /// starved by its own mod's bursts idles this tick rather
    /// than borrowing. 0 for a mod or an emitter that is not
    /// there, and for an anchor the game could not resolve:
    /// it asks only for a resolved one, so an unanchored
    /// emitter does not advance.
    pub fn particle_emitter_spawns(&mut self, owner: usize, index: usize) -> u32 {
        let Some(run) = self.entries.get_mut(owner).and_then(|e| e.run.as_mut()) else {
            return 0;
        };
        let data = run.store.data_mut();
        let Some(stored) = data.particles.emitters.get(index) else {
            return 0;
        };
        let (rate, count) = (stored.emitter.rate, stored.emitter.burst.count);
        let whole = {
            let stored = &mut data.particles.emitters[index];
            advance_emitter(&mut stored.accumulator, rate)
        };
        let mut admitted = 0;
        for _ in 0..whole {
            if !data.weigh_particles(count) {
                break;
            }
            admitted += 1;
        }
        admitted
    }
    /// Ends every running mod's emitters (API 26): the game
    /// calls this as the world closes — anchors are the open
    /// world's, and a reopened world starts with no emitters
    /// until the mods declare them again.
    pub fn clear_particle_emitters(&mut self) {
        for run in self.entries.iter_mut().filter_map(|e| e.run.as_mut()) {
            run.store.data_mut().particles.emitters.clear();
        }
    }

    /// The running mods' own dynamic-light sources
    /// (capability `render`'s `dynamic-light`, API 28):
    /// every running mod's registered sources in load order,
    /// as (entry index, source) pairs — per-mod and additive,
    /// as the emitters are. The game resolves each anchor
    /// against the drawn world in its frame gather. A
    /// stopped mod's sources are gone with its store.
    pub fn dynamic_light_sources(&self) -> Vec<(usize, ModLightSource)> {
        self.entries
            .iter()
            .enumerate()
            .filter_map(|(i, e)| e.run.as_ref().map(|r| (i, r.store.data())))
            .flat_map(|(i, data)| {
                data.dynamic_light
                    .sources
                    .iter()
                    .map(move |source| (i, source.clone()))
            })
            .collect()
    }
    /// The policy standing over the game's own dynamic-light
    /// gather (API 28): the newest `set-policy` among running
    /// mods — one policy stands, by tenancy. When its author
    /// stops or clears, the next newest setter's stands; when
    /// none stands, `None`: the player's own Dynamic lights
    /// setting rules the gather, exactly as it does today.
    pub fn dynamic_light_policy(&self) -> Option<&ModDynamicLightPolicy> {
        self.entries
            .iter()
            .filter_map(|e| e.run.as_ref()?.store.data().dynamic_light.policy.as_ref())
            .max_by_key(|(seq, _)| *seq)
            .map(|(_, policy)| policy)
    }
    /// Ends every running mod's dynamic lights (API 28): the
    /// game calls this as the world closes — anchors and the
    /// gather are the open world's, and a reopened world
    /// starts with no sources and no policy until the mods
    /// declare them again.
    pub fn clear_dynamic_lights(&mut self) {
        for run in self.entries.iter_mut().filter_map(|e| e.run.as_mut()) {
            let store = &mut run.store.data_mut().dynamic_light;
            store.sources.clear();
            store.policy = None;
        }
    }

    /// The line decoration that applies to the game's chat
    /// (capability `chat-decoration`, API 27): the newest set
    /// among running mods that set one — the latest-writer
    /// rule the hand's offset runs, per record. A stopped
    /// mod's declaration is gone with its store; when the
    /// winner stops, an earlier declaration stands again.
    /// `None`: vanilla chat.
    pub fn chat_line_decoration(&self) -> Option<ModLineDecoration> {
        self.entries
            .iter()
            .filter_map(|e| {
                e.run
                    .as_ref()?
                    .store
                    .data()
                    .chat_decoration
                    .line_decoration
                    .as_ref()
            })
            .max_by_key(|(seq, _)| *seq)
            .map(|(_, decoration)| *decoration)
    }

    /// The chat animation that applies (capability
    /// `chat-decoration`, API 27), aggregated exactly like
    /// [`Mods::chat_line_decoration`] and independent of it:
    /// one mod's heads and another's animation stand
    /// together. `None`: vanilla chat.
    pub fn chat_animation(&self) -> Option<ModChatAnimation> {
        self.entries
            .iter()
            .filter_map(|e| {
                e.run
                    .as_ref()?
                    .store
                    .data()
                    .chat_decoration
                    .animation
                    .as_ref()
            })
            .max_by_key(|(seq, _)| *seq)
            .map(|(_, animation)| *animation)
    }

    /// The running mods' model rules as one evaluation index
    /// (WIT `item-model`, API 23): rebuilt only when a running
    /// mod's rule version moves — a set, a start, a stop — and
    /// shared as one `Arc` between rebuilds, so the game's
    /// per-stack resolve pays a lock and a lookup, not a walk
    /// of every mod's list.
    pub fn item_model_index(&self) -> Arc<ItemModelIndex> {
        let versions: Vec<Option<u64>> = self
            .entries
            .iter()
            .map(|e| e.run.as_ref().map(|r| r.store.data().model_rules_version))
            .collect();
        let mut cache = self
            .item_model_index
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some((built, index)) = cache.as_ref()
            && *built == versions
        {
            return index.clone();
        }
        let index = Arc::new(ItemModelIndex::from_mod_rules(
            self.entries
                .iter()
                .filter_map(|e| e.run.as_ref())
                .map(|r| r.store.data().model_rules.as_slice()),
        ));
        *cache = Some((versions, index.clone()));
        index
    }
    /// The slot marks the running mods declared (WIT
    /// `item-model`, API 23), every mod's list in load order:
    /// the union the game draws — a mark never erases a fellow
    /// mod's. A stopped mod's marks go with its store.
    pub fn decorations(&self) -> Vec<ItemDecoration> {
        let mut out = Vec::new();
        for data in self
            .entries
            .iter()
            .filter_map(|e| e.run.as_ref())
            .map(|r| r.store.data())
        {
            out.extend(data.item_decorations.iter().cloned());
        }
        out
    }
    /// How dropped items display (WIT `item-model`, API 23):
    /// the record the running mods set most recently with
    /// `set-dropped-display`. The all-vanilla record when no
    /// running mod sets one; a stopped mod's policy is dropped
    /// with its store, as its hand offset is.
    pub fn dropped_display(&self) -> ModDroppedDisplay {
        self.entries
            .iter()
            .filter_map(|e| e.run.as_ref()?.store.data().dropped_display.as_ref())
            .max_by_key(|(seq, _)| *seq)
            .map_or_else(
                || ModDroppedDisplay {
                    flat: ModFlatDisplay::Vanilla,
                    blocks: ModBlockDisplay::Vanilla,
                    shadows: true,
                },
                |(_, display)| display.clone(),
            )
    }

    /// The first running mod driving a camera, if any.
    fn camera_winner(&self) -> Option<usize> {
        self.entries.iter().position(|e| {
            e.run
                .as_ref()
                .is_some_and(|run| run.store.data().camera.is_some())
        })
    }
    /// The strongest gamma a running mod asks for (capability `render`).
    pub fn gamma(&self) -> Option<f32> {
        strongest_gamma(self.gamma_values(None))
    }
    /// The strongest gamma a running mod other than `skip` asks for: what
    /// that mod's `get-gamma` reads beside its own value, so a mod that
    /// lowers its own gamma after setting it high does not keep reading
    /// its old value back.
    fn gamma_except(&self, skip: usize) -> Option<f32> {
        strongest_gamma(self.gamma_values(Some(skip)))
    }
    fn gamma_values(&self, skip: Option<usize>) -> impl Iterator<Item = f32> + '_ {
        self.entries
            .iter()
            .enumerate()
            .filter(move |(i, _)| Some(*i) != skip)
            .filter_map(|(_, e)| e.run.as_ref()?.store.data().gamma)
    }
    /// Whether any running mod turned distance fog off (capability `render`).
    pub fn fog_off(&self) -> bool {
        any_fog_off(self.fog_values(None))
    }
    /// Whether any running mod other than `skip` turned distance fog off.
    fn fog_off_except(&self, skip: usize) -> bool {
        any_fog_off(self.fog_values(Some(skip)))
    }
    fn fog_values(&self, skip: Option<usize>) -> impl Iterator<Item = bool> + '_ {
        self.entries
            .iter()
            .enumerate()
            .filter(move |(i, _)| Some(*i) != skip)
            .filter_map(|(_, e)| e.run.as_ref()?.store.data().fog)
    }
    fn inputs(&self) -> impl Iterator<Item = &InputState> {
        self.entries
            .iter()
            .filter_map(|e| Some(&e.run.as_ref()?.store.data().input))
    }
    fn push_input(&mut self, to: impl Fn(&InputState) -> bool, event: &InputEvent) {
        for run in self.entries.iter_mut().filter_map(|e| e.run.as_mut()) {
            let input = &mut run.store.data_mut().input;
            if to(input) {
                input.push(event.clone());
            }
        }
    }
    /// The running mods' HUD shapes (capability `render`), each mod's in
    /// its own order, mods in load order.
    pub fn hud_shapes(&self) -> impl Iterator<Item = &Shape> {
        self.entries
            .iter()
            .filter_map(|e| e.run.as_ref())
            .flat_map(|r| &r.store.data().shapes)
    }
    /// Some running mod draws world shapes (`render.set-world`): the game
    /// drains them into the frame's depth-tested lines. Any mod importing
    /// `render` sets this — [`api_level::imports`] lists every function
    /// of an imported interface — so the gate saves work only while no
    /// render mod runs at all.
    pub fn wants_world_shapes(&self) -> bool {
        self.entries
            .iter()
            .any(|e| e.run.as_ref().is_some_and(|r| r.reads_world_shapes))
    }
    /// The running mods' world shapes (capability `render`), each mod's
    /// in its own order, mods in load order.
    pub fn world_shapes(&self) -> impl Iterator<Item = &WorldShape> {
        self.entries
            .iter()
            .filter_map(|e| e.run.as_ref())
            .flat_map(|r| &r.store.data().world_shapes)
    }
    /// The running mods' world shapes as depth-tested lines, in the same
    /// order: one line per line, a box's twelve edges.
    pub fn world_lines(&self) -> Vec<WorldDrawLine> {
        self.entries
            .iter()
            .filter_map(|e| e.run.as_ref())
            .flat_map(|r| world_shape_lines(&r.store.data().world_shapes))
            .collect()
    }
    /// The screen mod `i` last pushed with `ui.open-screen`, while it is up.
    pub fn screen(&self, i: usize) -> Option<&UiScreen> {
        self.entries
            .get(i)?
            .run
            .as_ref()?
            .store
            .data()
            .ui
            .screen
            .as_ref()
    }
    /// The widgets `ui.set-settings` pushed: mod `i`'s page in
    /// Options > Mods. Empty while it has none.
    pub fn settings_widgets(&self, i: usize) -> &[UiWidget] {
        self.entries
            .get(i)
            .and_then(|e| e.run.as_ref())
            .map_or(&[], |r| &r.store.data().ui.settings)
    }
    /// A `list` widget's row was picked on a surface of mod `i`.
    pub fn ui_select(&mut self, i: usize, id: u32, row: u32) {
        self.ui_event(i, |ui, s| {
            ui.horizon_mods_ui_events().call_select(s, id, row)
        });
    }
    /// The player left mod `i`'s screen (Esc or Done).
    pub fn ui_closed(&mut self, i: usize) {
        self.ui_event(i, |ui, s| ui.horizon_mods_ui_events().call_closed(s));
    }
    /// The player opened mod `i`'s settings page.
    pub fn ui_settings_opened(&mut self, i: usize) {
        self.ui_event(i, |ui, s| {
            ui.horizon_mods_ui_events().call_settings_opened(s)
        });
    }
    /// A `ui-events` call on mod `i`; no export, no call (mods built
    /// before `ui` have none).
    fn ui_event(
        &mut self,
        i: usize,
        f: impl FnOnce(&UiHost, &mut Store<State>) -> wasmtime::Result<()>,
    ) {
        let has_ui = self
            .entries
            .get(i)
            .and_then(|e| e.run.as_ref())
            .is_some_and(|r| r.ui.is_some());
        if has_ui {
            self.call(i, FUEL_EVENT, |run| {
                f(run.ui.as_ref().expect("checked above"), &mut run.store)
            });
        }
    }
    /// What mods asked to show since the last call.
    pub fn drain(&mut self) -> Vec<Output> {
        std::mem::take(&mut self.out)
    }

    /// Restarts the mods whose file changed since they loaded (mod
    /// development: rebuild, and the game picks it up). True when any did.
    pub fn reload_changed(&mut self, reserved: impl Fn(&str) -> bool) -> bool {
        let mut any = false;
        for i in 0..self.entries.len() {
            let entry = &self.entries[i];
            let now = modified(&entry.path);
            let was_on = !matches!(entry.status, Status::Off | Status::Elsewhere);
            if now == entry.modified || !was_on {
                continue;
            }
            any = true;
            self.generation += 1;
            self.entries[i].modified = now;
            self.entries[i].run = None;
            let path = self.entries[i].path.clone();
            let started = Package::read(&path).and_then(|p| {
                self.entries[i].manifest = Some(p.manifest.clone());
                self.start(&path, &p, &reserved)
            });
            let entry = &mut self.entries[i];
            let id = entry.manifest.as_ref().map_or("?", |m| m.id.as_str());
            match started {
                Ok(run) => {
                    log::info!("Horizon: mod {id} reloaded");
                    entry.run = run;
                    entry.status = Status::Running;
                }
                Err(e) => {
                    log::warn!("Horizon: mod {id} did not reload: {e}");
                    entry.status = Status::Failed(e);
                }
            }
        }
        any
    }
}

/// A command's arguments are well formed: names given and distinct,
/// optional ones last, free text only last.
fn check_arguments(arguments: &[Argument]) -> Result<(), String> {
    for (i, a) in arguments.iter().enumerate() {
        if a.name.is_empty()
            || !a
                .name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return Err(format!("argument name '{}' isn't [A-Za-z0-9_]+", a.name));
        }
        if arguments[..i].iter().any(|b| b.name == a.name) {
            return Err(format!("two arguments are named {}", a.name));
        }
        if i > 0 && arguments[i - 1].optional && !a.optional {
            return Err(format!("{} is required after an optional argument", a.name));
        }
        if a.kind == ArgumentKind::Text && i + 1 < arguments.len() {
            return Err(format!(
                "{} takes the rest of the line, so it must be last",
                a.name
            ));
        }
    }
    Ok(())
}

fn modified(path: &Path) -> Option<SystemTime> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.is_dir() {
        // A folder mod changes when its component does.
        return std::fs::metadata(path.join(crate::manifest::WASM))
            .ok()?
            .modified()
            .ok();
    }
    meta.modified().ok()
}

/// A trap or error, as the player reads it.
fn reason(e: &wasmtime::Error) -> String {
    match e.downcast_ref::<Trap>() {
        Some(Trap::OutOfFuel) => "it took too long (an endless loop?)".into(),
        Some(Trap::UnreachableCodeReached) => "it crashed (a panic)".into(),
        Some(trap) => format!("it crashed: {trap}"),
        None => format!("{e:#}"),
    }
}

fn clip(text: String) -> String {
    clip_to(text, LINE_CHARS)
}
fn clip_to(text: String, chars: usize) -> String {
    match text.char_indices().nth(chars) {
        Some((at, _)) => text[..at].to_string(),
        None => text,
    }
}

/// Widgets on a `ui` screen or settings page, and rows on one `list`.
const WIDGETS: usize = 64;
const LIST_ROWS: usize = 512;

fn clip_widgets(widgets: Vec<UiWidget>) -> Vec<UiWidget> {
    widgets
        .into_iter()
        .take(WIDGETS)
        .map(|w| match w {
            UiWidget::Text(mut t) => {
                t.text = clip(t.text);
                UiWidget::Text(t)
            }
            UiWidget::Button(mut b) => {
                b.label = clip(b.label);
                UiWidget::Button(b)
            }
            UiWidget::Item(mut i) => {
                i.item = clip(i.item);
                i.name = clip(i.name);
                UiWidget::Item(i)
            }
            UiWidget::List(mut l) => {
                l.rows = l.rows.into_iter().take(LIST_ROWS).map(clip).collect();
                l.selected = l.selected.filter(|&s| (s as usize) < l.rows.len());
                UiWidget::List(l)
            }
        })
        .collect()
}

/// The hand state before the game's first push (API 16): nothing
/// swings, nothing is held, the look is level — what `hand.get-state`
/// reads on the title screen.
fn zero_hand_state() -> ModHandState {
    ModHandState {
        swinging: false,
        swing_progress: 0.0,
        swing_kind: ModSwingKind::Other,
        equip_progress: 0.0,
        yaw: 0.0,
        pitch: 0.0,
        look_delta_yaw: 0.0,
        look_delta_pitch: 0.0,
        bob_phase: 0.0,
        bob_amount: 0.0,
        held_item: None,
        using_item: false,
        two_handed: false,
    }
}

/// `hand.set-offset`'s limits (API 16): a translation of a block
/// per axis and a turn of 60 degrees per axis are far past what a
/// feel mod needs, and a non-finite field reads as 0, so a NaN
/// never poisons the pose matrix.
const HAND_TRANSLATION_MAX: f32 = 1.0;
const HAND_ROTATION_MAX: f32 = 60.0;
fn clamp_hand_transform(offset: ModHandTransform) -> ModHandTransform {
    let finite = |v: f32, max: f32| {
        if v.is_finite() {
            v.clamp(-max, max)
        } else {
            0.0
        }
    };
    ModHandTransform {
        x: finite(offset.x, HAND_TRANSLATION_MAX),
        y: finite(offset.y, HAND_TRANSLATION_MAX),
        z: finite(offset.z, HAND_TRANSLATION_MAX),
        pitch: finite(offset.pitch, HAND_ROTATION_MAX),
        yaw: finite(offset.yaw, HAND_ROTATION_MAX),
        roll: finite(offset.roll, HAND_ROTATION_MAX),
    }
}

impl hand::Host for State {
    fn get_state(&mut self) -> ModHandState {
        self.hand_state.clone()
    }
    fn set_offset(&mut self, offset: ModHandTransform) {
        let seq = HAND_OFFSET_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.hand_offset = Some((seq, clamp_hand_transform(offset)));
    }
    fn clear_offset(&mut self) {
        self.hand_offset = None;
    }
    fn get_swing_anim(&mut self) -> ModSwingAnim {
        self.swing_anim
    }
    fn set_swing_scale(&mut self, scale: f32) {
        let seq = HAND_SWING_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // API 17's clamp: 0.0..=2.0, and a non-finite value reads
        // as 1.0 — vanilla in full, never a poisoned pose.
        let scale = if scale.is_finite() {
            scale.clamp(0.0, 2.0)
        } else {
            1.0
        };
        self.swing_scale = Some((seq, scale));
    }
}

/// `entity-pose.set-pose-offset`'s limits (API 20): a turn of
/// 180 degrees an axis and half a block of pivot are the whole
/// range a pose can use, and a non-finite field reads as 0, so a
/// NaN never poisons a pose matrix. The part list is capped at
/// 24 entries and the hidden list at 16 names — sanity caps, as
/// input's key cap: the player rig has 13 parts.
const POSE_ROTATION_MAX: f32 = 180.0;
const POSE_PIVOT_MAX: f32 = 0.5;
const POSE_PARTS_MAX: usize = 24;
const POSE_HIDDEN_MAX: usize = 16;
fn clamp_pose_offset(offset: ModPoseOffset) -> ModPoseOffset {
    let finite = |v: f32, max: f32| {
        if v.is_finite() {
            v.clamp(-max, max)
        } else {
            0.0
        }
    };
    ModPoseOffset {
        parts: offset
            .parts
            .into_iter()
            .take(POSE_PARTS_MAX)
            .map(|p| ModPartOffset {
                part: p.part,
                x: finite(p.x, POSE_PIVOT_MAX),
                y: finite(p.y, POSE_PIVOT_MAX),
                z: finite(p.z, POSE_PIVOT_MAX),
                pitch: finite(p.pitch, POSE_ROTATION_MAX),
                yaw: finite(p.yaw, POSE_ROTATION_MAX),
                roll: finite(p.roll, POSE_ROTATION_MAX),
            })
            .collect(),
        hidden: offset.hidden.into_iter().take(POSE_HIDDEN_MAX).collect(),
    }
}

/// `entity-pose.set-pose-scale`'s clamp (API 20): 0.0..=2.0, and
/// a non-finite value reads as 1.0 — vanilla in full, as the
/// swing scale's clamp.
fn clamp_pose_scale(scale: f32) -> f32 {
    if scale.is_finite() {
        scale.clamp(0.0, 2.0)
    } else {
        1.0
    }
}

/// A pose write's target as the stores key it (API 20): the WIT
/// `pose-target` variant in a hashable shape of our own (the
/// generated variant carries no `Hash`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum PoseKey {
    SelfPlayer,
    Entity(u32),
}
impl From<ModPoseTarget> for PoseKey {
    fn from(target: ModPoseTarget) -> Self {
        match target {
            ModPoseTarget::SelfPlayer => PoseKey::SelfPlayer,
            ModPoseTarget::Entity(id) => PoseKey::Entity(id),
        }
    }
}

/// `item-model.set-model-rules`' limits (API 23): at most
/// [`MODEL_RULES`] rules, at most [`MODEL_MATCHES`] component
/// matches a rule, the strings clipped as the other stores'
/// are. A rule naming no item or no model is dropped — it
/// could only ever be inert, and a dropped rule keeps the
/// evaluation index honest about what it holds. The values
/// themselves are parsed when the index builds
/// ([`ParsedMatch::parse`]).
fn sanitize_model_rules(rules: Vec<ModModelRule>) -> Vec<ModModelRule> {
    rules
        .into_iter()
        .take(MODEL_RULES)
        .filter_map(|mut rule| {
            rule.match_.item = clip_to(std::mem::take(&mut rule.match_.item), 256);
            rule.model = clip_to(std::mem::take(&mut rule.model), 256);
            if rule.match_.item.is_empty() || rule.model.is_empty() {
                return None;
            }
            rule.match_.components.truncate(MODEL_MATCHES);
            for m in &mut rule.match_.components {
                m.name = clip_to(std::mem::take(&mut m.name), 256);
                m.value = clip_to(std::mem::take(&mut m.value), 4096);
            }
            Some(rule)
        })
        .collect()
}

impl item_model::Host for State {
    fn set_model_rules(&mut self, rules: Vec<ModModelRule>) {
        self.model_rules = sanitize_model_rules(rules);
        self.model_rules_version += 1;
    }
    fn set_decorations(&mut self, mut decorations: Vec<ModDecoration>) {
        // The store is clamped here, at the door, and the stack
        // matches parsed with it: what the aggregate later reads
        // back is what the game draws.
        decorations.truncate(ITEM_DECORATIONS);
        self.item_decorations = decorations.iter().map(ItemDecoration::parse).collect();
    }
    fn set_dropped_display(&mut self, display: ModDroppedDisplay) {
        let seq = ITEM_DISPLAY_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.dropped_display = Some((seq, display));
    }
}

impl entity_pose::Host for State {
    fn get_pose_states(&mut self) -> Vec<ModEntityPoseState> {
        self.pose_states.clone()
    }
    fn set_pose_offset(&mut self, target: ModPoseTarget, offset: ModPoseOffset) {
        let seq = POSE_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.pose_offsets
            .insert(target.into(), (seq, clamp_pose_offset(offset)));
    }
    fn clear_pose_offset(&mut self, target: ModPoseTarget) {
        self.pose_offsets.remove(&PoseKey::from(target));
    }
    fn set_pose_scale(&mut self, target: ModPoseTarget, scale: f32) {
        let seq = POSE_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.pose_scales
            .insert(target.into(), (seq, clamp_pose_scale(scale)));
    }
    fn set_first_person_body(&mut self, shown: bool) {
        self.own_body = shown;
    }
}

impl State {
    /// Queues one action for the game to perform after this hook
    /// returns (API 21), unless the call already queued
    /// [`ACTIONS_PER_CALL`] — the `command.run` discipline: past
    /// the cap the request is dropped, never deferred. The cap
    /// is shared with `screen`'s trade choice (API 22): both
    /// spend the player's clicks or items, so together they
    /// stay a click's worth of intent, never a script.
    fn queue_action(&mut self, action: ModAction) {
        if self.queued_verbs() < ACTIONS_PER_CALL {
            self.out.push(Output::Action {
                mod_id: self.id.clone(),
                action,
            });
        }
    }
    /// The outputs this hook call has queued that spend the
    /// player's clicks or items: `actions`' clicks (API 21)
    /// and `screen`'s trade choices (API 22) together.
    fn queued_verbs(&self) -> usize {
        self.out
            .iter()
            .filter(|o| matches!(o, Output::Action { .. } | Output::SelectTrade { .. }))
            .count()
    }
    /// Queues one slot verb for the game to perform after this
    /// hook returns (API 24), unless the call already queued
    /// [`SLOT_VERBS_PER_CALL`] weighed verbs — the `command.run`
    /// discipline again: past the cap the request is dropped,
    /// never deferred. The count is this district's own, not
    /// `actions`' shared one: slot moves are cold and drags are
    /// heavy, so they weigh differently and cap separately.
    fn queue_slot_move(&mut self, verb: ModSlotMove) {
        if self.queued_slot_verbs() + verb.weight() <= SLOT_VERBS_PER_CALL {
            let out = match verb {
                ModSlotMove::Click(click) => Output::SlotClick {
                    mod_id: self.id.clone(),
                    click,
                },
                ModSlotMove::Drag(drag) => Output::SlotDrag {
                    mod_id: self.id.clone(),
                    drag,
                },
            };
            self.out.push(out);
        }
    }
    /// The weighed slot verbs this hook call has queued so far
    /// (API 24): clicks weigh 1, drags weigh 8.
    fn queued_slot_verbs(&self) -> usize {
        self.out
            .iter()
            .map(|o| match o {
                Output::SlotClick { .. } => 1,
                Output::SlotDrag { .. } => 8,
                _ => 0,
            })
            .sum()
    }
}

impl actions::Host for State {
    fn target(&mut self) -> Option<ModActionTarget> {
        self.snapshot.action_target.clone()
    }
    fn use_(&mut self) {
        self.queue_action(ModAction::Use);
    }
    fn use_on_block(&mut self, target: ModBlockTarget, hand: ModHand) -> Result<(), String> {
        // The one malformed case a target can carry: its cursor
        // outside the block it names (the face is an enum, the
        // position any block in the world).
        let in_block = |v: f32| v.is_finite() && (0.0..=1.0).contains(&v);
        if ![target.cursor_x, target.cursor_y, target.cursor_z]
            .iter()
            .all(|v| in_block(*v))
        {
            return Err("the target's cursor is outside its block".into());
        }
        self.queue_action(ModAction::UseOnBlock { target, hand });
        Ok(())
    }
    fn attack(&mut self) {
        self.queue_action(ModAction::Attack);
    }
    fn set_held(&mut self, attack: bool, use_: bool) {
        // State, not a request: set in place, and the mod's own
        // call re-arms a hold a screen latched off.
        self.held = (attack, use_);
        self.held_latched = false;
    }
    fn select_slot(&mut self, slot: u32) -> Result<(), String> {
        if slot > 8 {
            return Err(format!("slot {slot} is outside 0 to 8"));
        }
        self.queue_action(ModAction::SelectSlot(slot));
        Ok(())
    }
}

impl inventory_actions::Host for State {
    fn click(&mut self, slot: Option<u32>, button: u32, kind: ModClickKind) -> Result<(), String> {
        // Call-time errors are only for the malformed: a slot
        // past the numbering's end, or a button the kind does
        // not take. Whether the click means anything in the
        // menu open when it is performed is the menu's own
        // judgement, and a dropped click is not an error.
        if let Some(slot) = slot
            && slot > 999
        {
            return Err(format!("slot {slot} is past 999"));
        }
        match kind {
            ModClickKind::Pickup
            | ModClickKind::QuickMove
            | ModClickKind::Throw
            | ModClickKind::PickupAll => {
                if button > 1 {
                    return Err(format!("button {button} is outside 0 to 1"));
                }
            }
            ModClickKind::Swap => {
                if button > 8 && button != 40 {
                    return Err(format!(
                        "swap takes a hotbar slot, 0 to 8 or 40, not button {button}"
                    ));
                }
            }
            ModClickKind::Clone => {
                if button != 0 {
                    return Err(format!("clone takes button 0, not button {button}"));
                }
            }
        }
        self.queue_slot_move(ModSlotMove::Click(ModSlotClick { slot, button, kind }));
        Ok(())
    }
    fn drag(&mut self, button: ModDragButton, slots: Vec<u32>) -> Result<(), String> {
        if slots.is_empty() {
            return Err("a drag names no slots".into());
        }
        if slots.len() > 64 {
            return Err(format!("a drag names {} slots, past 64", slots.len()));
        }
        if let Some(slot) = slots.iter().find(|s| **s > 999) {
            return Err(format!("slot {slot} is past 999"));
        }
        self.queue_slot_move(ModSlotMove::Drag(ModSlotDrag { button, slots }));
        Ok(())
    }
    fn poll_wheel(&mut self) -> Vec<ModWheelEvent> {
        self.wheel.drain(..).collect()
    }
}

impl screen::Host for State {
    fn state(&mut self) -> ModScreenState {
        self.snapshot.screen_state.clone()
    }
    fn trades(&mut self) -> Option<ModTradeList> {
        self.snapshot.trades.clone()
    }
    fn set_buttons(&mut self, mut buttons: Vec<ModScreenButton>) {
        // The store is clamped here, at the door: at most
        // SCREEN_BUTTONS declarations, labels cut to 64
        // characters — what the context aggregate
        // ([`Mods::screen_buttons`]) later reads back.
        buttons.truncate(SCREEN_BUTTONS);
        for button in &mut buttons {
            button.label = clip_to(std::mem::take(&mut button.label), 64);
        }
        self.screen.buttons = buttons;
    }
    fn set_highlights(&mut self, mut slots: Vec<ModHighlight>) {
        slots.truncate(SCREEN_HIGHLIGHTS);
        self.screen.highlights = slots;
    }
    fn poll(&mut self) -> Vec<ModScreenEvent> {
        self.screen.events.drain(..).collect()
    }
    fn select_trade(&mut self, index: u32) -> Result<(), String> {
        // The one call-time fact the mod can be told: whether a
        // merchant screen is open as of this tick. The index is
        // judged when the choice performs, next tick, against
        // the merchant that is still open then.
        if self.snapshot.trades.is_none() {
            return Err("no merchant screen is open".into());
        }
        if self.queued_verbs() < ACTIONS_PER_CALL {
            self.out.push(Output::SelectTrade {
                mod_id: self.id.clone(),
                index,
            });
        }
        Ok(())
    }
    fn claim_debug(&mut self, claimed: bool) {
        self.screen.debug_claimed = claimed;
    }
}

/// Whether a map image passes the door (capability `map`,
/// API 25): the acting districts' call-time law, in the
/// declaration districts' form — `set-map-screen` and
/// `set-minimap-image` return nothing, so a malformed image
/// is dropped here and sets nothing, as `screen`'s
/// declarations clamp at theirs. A pixel count that is not
/// `width * height`, an empty palette or one past 256, a side
/// past 2048 or a pixel count past the whole allowance, and
/// a scale that is not a positive number are malformed; an
/// index past the palette's end is not — it draws as clear,
/// the inert-rule style of `item-model` (API 23).
fn map_image_ok(image: &ModMapImage) -> bool {
    if image.width > MAP_IMAGE_MAX_SIDE || image.height > MAP_IMAGE_MAX_SIDE {
        return false;
    }
    let pixels = u64::from(image.width) * u64::from(image.height);
    if pixels > MAP_IMAGE_MAX_PIXELS || image.pixels.len() as u64 != pixels {
        return false;
    }
    if image.palette.is_empty() || image.palette.len() > 256 {
        return false;
    }
    image.blocks_per_pixel.is_finite() && image.blocks_per_pixel > 0.0
}

impl map::Host for State {
    fn tiles(&mut self, chunks: Vec<ModChunkPos>) -> Vec<ModMapTile> {
        // The bulk-read law, as `world.get-blocks` keeps it:
        // over the cap, no tiles — never a truncated survey.
        if chunks.len() > TILES_PER_CALL {
            return Vec::new();
        }
        chunks
            .iter()
            .filter_map(|c| {
                let data = self.view_tile(c.x, c.z)?;
                Some(ModMapTile {
                    chunk_x: c.x,
                    chunk_z: c.z,
                    layer: data.layer,
                    cut_y: data.cut_y,
                    version: data.version,
                    colors: data.colors.to_vec(),
                    heights: data.heights.to_vec(),
                    depths: data.depths.to_vec(),
                })
            })
            .collect()
    }
    fn tile_versions(&mut self, chunks: Vec<ModChunkPos>) -> Vec<ModTileVersion> {
        if chunks.len() > TILE_VERSIONS_PER_CALL {
            return Vec::new();
        }
        chunks
            .iter()
            .filter_map(|c| {
                self.view_tile_version(c.x, c.z)
                    .map(|version| ModTileVersion {
                        chunk_x: c.x,
                        chunk_z: c.z,
                        version,
                    })
            })
            .collect()
    }
    fn context(&mut self) -> Option<ModMapContext> {
        self.snapshot.map_context.clone()
    }
    fn last_death(&mut self) -> Option<ModPosition> {
        self.snapshot.last_death.clone()
    }
    fn poll_events(&mut self) -> Vec<ModPositionEvent> {
        self.map.position_events.drain(..).collect()
    }
    fn set_map_screen(&mut self, screen: Option<ModMapScreen>) {
        let Some(mut screen) = screen else {
            self.map.screen = None;
            return;
        };
        if !map_image_ok(&screen.image) {
            return;
        }
        // The store is clamped here, at the door: at most
        // MAP_MARKERS marks, their labels cut to 64
        // characters — what the aggregate
        // ([`Mods::map_screen`]) later reads back.
        screen.markers.truncate(MAP_MARKERS);
        for marker in &mut screen.markers {
            marker.label = clip_to(std::mem::take(&mut marker.label), 64);
        }
        // The view starts at the image's centre and scale.
        let view = ModMapView {
            center_x: f64::from(screen.image.center_x),
            center_z: f64::from(screen.image.center_z),
            blocks_per_pixel: screen.image.blocks_per_pixel,
        };
        let seq = MAP_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.map.screen = Some((seq, screen, view));
    }
    fn poll_map(&mut self) -> Vec<ModMapEvent> {
        self.map.map_events.drain(..).collect()
    }
    fn set_minimap_image(&mut self, image: Option<ModMinimapImage>) {
        let Some(mut image) = image else {
            self.map.minimap_image = None;
            return;
        };
        if !map_image_ok(&image.image) {
            return;
        }
        // The side is the WIT's 16 to 256, sanitized as
        // `render.set-minimap` sanitizes its own: a value
        // that is not a number reads as the smallest.
        image.size = if image.size.is_finite() {
            image.size.clamp(16.0, 256.0)
        } else {
            16.0
        };
        let seq = MAP_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.map.minimap_image = Some((seq, image));
    }
}

/// A burst as the door keeps it (capability `particles`,
/// API 26): sanitized at call time, as the declaration
/// districts sanitize theirs — the name trimmed and cut to
/// [`PARTICLE_NAME_CHARS`], the count cut to
/// [`PARTICLE_COUNT_MAX`], the spread and speed clamped to
/// their ranges, the scale and duration to the ones the
/// types themselves clamp to. A non-finite number anywhere
/// in it refuses the whole burst (None), as the native line
/// refuses them: half a burst is not a burst. A name past
/// the cap is not refused — it is inert, clipped to a name
/// no loaded type has, so the game draws nothing for it.
fn sanitize_burst(mut burst: ModBurst) -> Option<ModBurst> {
    fn finite(point: &ModParticlePoint) -> bool {
        point.x.is_finite() && point.y.is_finite() && point.z.is_finite()
    }
    if !finite(&burst.at) || !finite(&burst.spread) || !burst.speed.is_finite() {
        return None;
    }
    if burst.params.scale.is_some_and(|scale| !scale.is_finite()) {
        return None;
    }
    if burst.params.target.as_ref().is_some_and(|t| !finite(t)) {
        return None;
    }
    burst.particle = clip_to(burst.particle.trim().to_string(), PARTICLE_NAME_CHARS);
    burst.count = burst.count.min(PARTICLE_COUNT_MAX);
    burst.spread.x = burst
        .spread
        .x
        .clamp(-PARTICLE_SPREAD_MAX, PARTICLE_SPREAD_MAX);
    burst.spread.y = burst
        .spread
        .y
        .clamp(-PARTICLE_SPREAD_MAX, PARTICLE_SPREAD_MAX);
    burst.spread.z = burst
        .spread
        .z
        .clamp(-PARTICLE_SPREAD_MAX, PARTICLE_SPREAD_MAX);
    burst.speed = burst.speed.clamp(-PARTICLE_SPEED_MAX, PARTICLE_SPEED_MAX);
    if let Some(scale) = &mut burst.params.scale {
        *scale = scale.clamp(0.01, 4.0);
    }
    if let Some(duration) = &mut burst.params.duration {
        *duration = (*duration).clamp(1, 200);
    }
    if let Some(block) = &mut burst.params.block {
        let taken = std::mem::take(block);
        *block = clip_to(taken, LINE_CHARS);
    }
    Some(burst)
}

/// An emitter as the door keeps it (API 26): its burst
/// sanitized as a burst — its `at` zeroed first, since the
/// anchor replaces it and its numbers are not the stream's —
/// its rate clamped to the stream's range (a non-finite
/// rate reads as 0, the district's non-finite style for a
/// scale), and its offset clamped to the anchor's
/// neighbourhood, a non-finite component reading as 0.
fn sanitize_emitter(mut emitter: ModEmitter) -> Option<StoredEmitter> {
    emitter.burst.at = ModParticlePoint {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    emitter.burst = sanitize_burst(emitter.burst)?;
    emitter.rate = if emitter.rate.is_finite() {
        emitter.rate.clamp(0.0, PARTICLE_EMITTER_RATE_MAX)
    } else {
        0.0
    };
    for axis in [
        &mut emitter.offset.x,
        &mut emitter.offset.y,
        &mut emitter.offset.z,
    ] {
        *axis = if axis.is_finite() {
            (*axis).clamp(-PARTICLE_OFFSET_MAX, PARTICLE_OFFSET_MAX)
        } else {
            0.0
        };
    }
    Some(StoredEmitter {
        emitter,
        accumulator: 0.0,
    })
}

/// How many whole spawns an emitter's accumulator holds
/// after one tick at `rate` spawns a second (capability
/// `particles`, API 26): the fraction carries, so 7 a second
/// is seven spawns in most seconds — the game steps
/// emitters once a tick, 20 a second.
fn advance_emitter(accumulator: &mut f32, rate: f32) -> u32 {
    *accumulator += rate / 20.0;
    let whole = accumulator.floor().max(0.0);
    *accumulator -= whole;
    whole as u32
}

impl State {
    /// Weighs `count` particles against this mod's per-tick
    /// budget (capability `particles`, API 26): the first
    /// weigh of a tick resets the spent count; an ask that
    /// fits is charged and answered true; one that does not
    /// is answered false and charges nothing — drop, never
    /// defer. A count of 0 still weighs 1: a count-0 burst is
    /// one directional particle, and the budget prices the
    /// ask, not only the draw.
    fn weigh_particles(&mut self, count: u32) -> bool {
        if self.particles.spent.0 != self.tick {
            self.particles.spent = (self.tick, 0);
        }
        let weighed = count.max(1) as usize;
        if self.particles.spent.1 + weighed > PARTICLE_BUDGET_PER_TICK {
            return false;
        }
        self.particles.spent.1 += weighed;
        true
    }
}

impl particles::Host for State {
    fn emit(&mut self, burst: ModBurst) {
        // Sanitized at the door, weighed against the tick's
        // budget, and queued for the game's drain — what the
        // engine draws of it is the game's side, as the
        // actions queue is.
        let Some(burst) = sanitize_burst(burst) else {
            return;
        };
        if self.weigh_particles(burst.count) {
            self.out.push(Output::ParticleBurst {
                mod_id: self.id.clone(),
                burst,
            });
        }
    }
    fn set_emitters(&mut self, emitters: Option<Vec<ModEmitter>>) {
        // Replaced wholesale, as the declaration districts'
        // sets are: at most PARTICLE_EMITTERS streams, each
        // sanitized at the door, each accumulator starting
        // at nothing. None clears, as the screens clear.
        self.particles.emitters = emitters
            .unwrap_or_default()
            .into_iter()
            .take(PARTICLE_EMITTERS)
            .filter_map(sanitize_emitter)
            .collect();
    }
}

/// A dynamic-light source as the door keeps it (capability
/// `render`'s `dynamic-light`, API 28): its level clamped
/// to the arena's 15, its item's name trimmed and cut to
/// [`DYNAMIC_LIGHT_NAME_CHARS`]. A `position` anchor with a
/// non-finite component drops the entry whole, as a burst's
/// non-finite numbers refuse it — half a place is not a
/// place. A source at level 0 with no item names nothing
/// that could glow, and is dropped; with an item it stands,
/// and the game's own table decides whether it glows.
fn sanitize_light_source(mut source: ModLightSource) -> Option<ModLightSource> {
    if let ModLightAnchor::Position(at) = &source.anchor
        && !(at.x.is_finite() && at.y.is_finite() && at.z.is_finite())
    {
        return None;
    }
    source.level = source.level.min(15);
    if let Some(item) = &mut source.item {
        let taken = std::mem::take(item);
        *item = clip_to(taken.trim().to_string(), DYNAMIC_LIGHT_NAME_CHARS);
    }
    if source.level == 0 && source.item.is_none() {
        return None;
    }
    Some(source)
}

/// A dynamic-light policy as the door keeps it (API 28):
/// its glowing kinds cut to [`DYNAMIC_LIGHT_KINDS`] with
/// their levels clamped to 1..=15 and names trimmed and
/// cut, its brightness clamped to 10..=200, its range to
/// 8..=96 and its max-lights to 1..=16 — a policy can only
/// narrow the game's arena, never widen it.
fn sanitize_light_policy(mut policy: ModDynamicLightPolicy) -> ModDynamicLightPolicy {
    policy.glowing = policy
        .glowing
        .into_iter()
        .take(DYNAMIC_LIGHT_KINDS)
        .map(|mut kind| {
            kind.kind = clip_to(kind.kind.trim().to_string(), DYNAMIC_LIGHT_NAME_CHARS);
            kind.level = kind.level.clamp(1, 15);
            kind
        })
        .collect();
    policy.brightness = policy.brightness.clamp(10, 200);
    policy.range = policy.range.clamp(8, 96);
    policy.max_lights = policy.max_lights.clamp(1, 16);
    policy
}

impl dynamic_light::Host for State {
    fn set_sources(&mut self, sources: Option<Vec<ModLightSource>>) {
        // Replaced wholesale, as the declaration districts'
        // sets are: at most DYNAMIC_LIGHT_SOURCES sources,
        // each sanitized at the door. None clears, as the
        // emitters clear. The game resolves the anchors in
        // its frame gather — what a source draws is its side.
        self.dynamic_light.sources = sources
            .unwrap_or_default()
            .into_iter()
            .take(DYNAMIC_LIGHT_SOURCES)
            .filter_map(sanitize_light_source)
            .collect();
    }
    fn set_policy(&mut self, policy: Option<ModDynamicLightPolicy>) {
        // A policy set, not an action: the declaration is
        // sanitized and stamped with the order it happened
        // in, and stored whole, as the chat decoration is —
        // the aggregate ([`Mods::dynamic_light_policy`])
        // runs the tenancy rule. None clears the mod's own
        // policy; the player's setting is never written.
        self.dynamic_light.policy = policy.map(|policy| {
            let seq = DYNAMIC_LIGHT_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            (seq, sanitize_light_policy(policy))
        });
    }
}

impl chat_decoration::Host for State {
    fn set_line_decoration(&mut self, decoration: Option<ModLineDecoration>) {
        // A policy set, not an action: the declaration is
        // stamped with the order it happened in and stored
        // whole, as the hand's offset is — the aggregate
        // ([`Mods::chat_line_decoration`]) runs the
        // latest-writer rule per record. None clears.
        self.chat_decoration.line_decoration = decoration.map(|decoration| {
            let seq = CHAT_DECORATION_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            (seq, decoration)
        });
    }
    fn set_animation(&mut self, animation: Option<ModChatAnimation>) {
        // The durations are clamped at the door
        // ([`CHAT_ANIMATION_MS_MAX`]): an arrival or an
        // opening may ease for a second, never longer. The
        // record is otherwise the mod's own, stored whole
        // and stamped as the decoration is.
        self.chat_decoration.animation = animation.map(|mut animation| {
            animation.message_ms = animation.message_ms.min(CHAT_ANIMATION_MS_MAX);
            animation.input_ms = animation.input_ms.min(CHAT_ANIMATION_MS_MAX);
            let seq = CHAT_DECORATION_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            (seq, animation)
        });
    }
}

impl audio::Host for State {
    fn set_environment(&mut self, wet: f32, decay: f32) {
        self.sound_env = Some((wet.clamp(0.0, 1.0), decay.clamp(0.1, 10.0)));
    }
    fn clear_environment(&mut self) {
        self.sound_env = None;
    }
}

impl mod_log::Host for State {
    fn write(&mut self, level: mod_log::Level, message: String) {
        let level = match level {
            mod_log::Level::Debug => log::Level::Debug,
            mod_log::Level::Info => log::Level::Info,
            mod_log::Level::Warn => log::Level::Warn,
            mod_log::Level::Error => log::Level::Error,
        };
        log::log!(level, "[mod {}] {}", self.id, clip(message));
    }
}
impl game::Host for State {
    fn get_player(&mut self) -> Option<Player> {
        self.snapshot.player.clone()
    }
    fn get_clock(&mut self) -> Option<Clock> {
        self.snapshot.clock
    }
    fn get_effects(&mut self) -> Vec<ModEffect> {
        self.snapshot.effects.clone()
    }
    fn get_self_state(&mut self) -> Option<ModSelfState> {
        self.snapshot.self_state.clone()
    }
    fn set_movement_toggles(&mut self, sprint: Option<bool>, sneak: Option<bool>) {
        // The last call in the hook wins, whole: the game applies it
        // as one options change when the call ends (`Mods::call`).
        self.movement_toggles = Some((sprint, sneak));
    }
    fn raycast(&mut self, distance: f32) -> Option<Hit> {
        let (at, hit) = self.snapshot.view_hit.as_ref()?;
        // NaN compares false: no hit.
        (*at <= distance.min(RAYCAST_MAX) && !distance.is_nan()).then(|| hit.clone())
    }
}
/// Capability `inventory`: the player's own inventory and the open
/// container, read-only by absence — this interface has no write function
/// to refuse, unlike `world.set-blocks`.
impl inventory::Host for State {
    fn inventory(&mut self) -> PlayerInventory {
        self.snapshot.inventory.clone().unwrap_or(PlayerInventory {
            selected: 0,
            saturation: 0.0,
            slots: Vec::new(),
            carried: None,
        })
    }
    fn open_container(&mut self) -> Option<Container> {
        self.snapshot.open_container.clone()
    }
}
/// Capability `entity`: the living mobs around the player and the one
/// under the crosshair, read-only by absence — this interface has no
/// write function to refuse, unlike `world.set-blocks`.
impl entity::Host for State {
    fn entities(&mut self) -> Vec<Entity> {
        self.snapshot.entities.clone()
    }
    fn targeted(&mut self, distance: f32) -> Option<Target> {
        let target = self.snapshot.targeted.clone()?;
        // NaN compares false: no hit. A request past the cap gets the
        // capped result, as `game.raycast` does with `RAYCAST_MAX`.
        (target.distance <= distance.min(ENTITY_TARGET_MAX) && !distance.is_nan()).then_some(target)
    }
}
impl browser::Host for State {
    fn set_items(&mut self, items: Vec<BrowserStack>) {
        self.browser.items = items
            .into_iter()
            .take(BROWSER_ITEMS)
            .map(|s| BrowserStack {
                item: clip(s.item),
                components: clip_to(s.components, 4096),
                ..s
            })
            .collect();
        self.browser.version = BROWSER_VERSION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    fn set_recipes(
        &mut self,
        title: String,
        tabs: Vec<BrowserStack>,
        tab: u32,
        recipes: Vec<BrowserRecipe>,
    ) {
        let stack = |s: BrowserStack| BrowserStack {
            item: clip(s.item),
            components: clip_to(s.components, 4096),
            ..s
        };
        let cycle = |list: Vec<BrowserStack>| -> Vec<BrowserStack> {
            list.into_iter().take(SLOT_STACKS).map(stack).collect()
        };
        self.browser.recipes = (!recipes.is_empty()).then(|| {
            let recipes = recipes
                .into_iter()
                .take(BROWSER_RECIPES)
                .map(|r| BrowserRecipe {
                    width: r.width.clamp(1, 3),
                    height: r.height.clamp(1, 3),
                    slots: r.slots.into_iter().take(RECIPE_SLOTS).map(cycle).collect(),
                    results: cycle(r.results),
                    note: clip(r.note),
                    ..r
                })
                .collect();
            RecipePage {
                title: clip(title),
                tabs: tabs.into_iter().take(RECIPE_TABS).map(stack).collect(),
                tab,
                recipes,
            }
        });
        self.browser.version = BROWSER_VERSION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    fn poll(&mut self) -> Vec<BrowserEvent> {
        self.browser.events.drain(..).collect()
    }
}
impl recipes::Host for State {
    fn items(&mut self) -> Vec<RecipeItem> {
        self.snapshot
            .recipes
            .as_ref()
            .map_or_else(Vec::new, |r| r.items.clone())
    }
    fn all(&mut self) -> Vec<Recipe> {
        self.snapshot
            .recipes
            .as_ref()
            .map_or_else(Vec::new, |r| r.recipes.clone())
    }
    fn known(&mut self) -> Vec<i32> {
        self.snapshot.known_recipes.to_vec()
    }
    fn place(&mut self, book_id: i32, all: bool) -> Result<(), String> {
        self.recipe_action()?;
        if !self.snapshot.placing {
            return Err("no screen that takes recipes is open".into());
        }
        if !self.snapshot.known_recipes.contains(&book_id) {
            return Err(format!(
                "recipe {book_id} is not in the player's recipe book"
            ));
        }
        self.out.push(Output::PlaceRecipe { book_id, all });
        Ok(())
    }
    fn give(&mut self, stack: RecipeStack) -> Result<(), String> {
        self.recipe_action()?;
        if !self.snapshot.creative {
            return Err("only in creative mode".into());
        }
        let item = self
            .snapshot
            .recipes
            .as_ref()
            .and_then(|r| r.items.get(stack.item as usize))
            .ok_or_else(|| format!("no item {}", stack.item))?
            .id
            .clone();
        self.out.push(Output::Give {
            item,
            count: stack.count.clamp(1, 99),
            components: clip_to(stack.components, 4096),
        });
        Ok(())
    }
}
impl State {
    /// `recipes.place` and `give` take at most [`RECIPE_ACTIONS`] a call.
    fn recipe_action(&self) -> Result<(), String> {
        let done = self
            .out
            .iter()
            .filter(|o| matches!(o, Output::PlaceRecipe { .. } | Output::Give { .. }))
            .count();
        if done < RECIPE_ACTIONS {
            Ok(())
        } else {
            Err(format!("at most {RECIPE_ACTIONS} places and gives a call"))
        }
    }
}
impl State {
    /// Queues a chat line unless the call already sent [`CHAT_PER_CALL`].
    fn say(&mut self, line: impl FnOnce(String) -> Output) -> bool {
        let shown = self
            .out
            .iter()
            .filter(|o| {
                matches!(
                    o,
                    Output::Chat { .. } | Output::Tell { .. } | Output::Broadcast { .. }
                )
            })
            .count();
        if shown < CHAT_PER_CALL {
            let name = self.name.clone();
            self.out.push(line(name));
        }
        shown < CHAT_PER_CALL
    }
}
impl chat::Host for State {
    fn show(&mut self, text: String) {
        self.say(|name| Output::Chat {
            name,
            text: clip(text),
        });
    }
}
impl State {
    /// One block read through the lent world: `None` where nothing is
    /// lent or no chunk is loaded there.
    fn view_block(&self, dimension: &str, x: i32, y: i32, z: i32) -> Option<String> {
        let view = self.world.lock().ok()?;
        view.as_ref()?.block(dimension, x, y, z)
    }
    /// One biome read, as [`State::view_block`].
    fn view_biome(&self, dimension: &str, x: i32, y: i32, z: i32) -> Option<String> {
        let view = self.world.lock().ok()?;
        view.as_ref()?.biome(dimension, x, y, z)
    }
    /// The block states at `positions`, in order, through the lent world.
    fn view_blocks(&self, dimension: &str, positions: &[(i32, i32, i32)]) -> Vec<Option<String>> {
        let Ok(view) = self.world.lock() else {
            return positions.iter().map(|_| None).collect();
        };
        let Some(view) = view.as_ref() else {
            return positions.iter().map(|_| None).collect();
        };
        positions
            .iter()
            .map(|&(x, y, z)| view.block(dimension, x, y, z))
            .collect()
    }
    /// The light at `positions`, in order, as [`State::view_blocks`].
    fn view_lights(&self, dimension: &str, positions: &[(i32, i32, i32)]) -> Vec<Option<(u8, u8)>> {
        let Ok(view) = self.world.lock() else {
            return positions.iter().map(|_| None).collect();
        };
        let Some(view) = view.as_ref() else {
            return positions.iter().map(|_| None).collect();
        };
        positions
            .iter()
            .map(|&(x, y, z)| view.light(dimension, x, y, z))
            .collect()
    }
    /// One tile read through the lent map view (API 25):
    /// `None` where nothing is lent or the game's map holds
    /// no tile of the chunk.
    fn view_tile(&self, cx: i32, cz: i32) -> Option<MapTileData> {
        let view = self.map_view.lock().ok()?;
        view.as_ref()?.tile(cx, cz)
    }
    /// One tile version read, as [`State::view_tile`].
    fn view_tile_version(&self, cx: i32, cz: i32) -> Option<u64> {
        let view = self.map_view.lock().ok()?;
        view.as_ref()?.tile_version(cx, cz)
    }
    /// `set-blocks`, shared by the client and server hosts: a client mod
    /// is refused outright and queues nothing; a server mod's changes are
    /// checked now and queued for the game to apply after the call.
    fn set_blocks_checked(
        &mut self,
        dimension: String,
        blocks: Vec<BlockChange>,
    ) -> Result<(), String> {
        if !self.server {
            return Err("client mods cannot change blocks".into());
        }
        if blocks.len() > BLOCKS_PER_CALL {
            return Err(format!(
                "{} blocks: at most {BLOCKS_PER_CALL} per call",
                blocks.len()
            ));
        }
        if let Some(view) = self.world.lock().ok().as_ref().and_then(|v| v.as_ref()) {
            if !view.has_dimension(&dimension) {
                return Err(format!("no dimension {dimension}"));
            }
            for b in &blocks {
                view.check_state(&b.state)?;
            }
        }
        if !blocks.is_empty() {
            self.out.push(Output::SetBlocks {
                name: self.name.clone(),
                dimension,
                blocks,
            });
        }
        Ok(())
    }
}

impl mod_world::Host for State {
    fn get_block(&mut self, dimension: String, x: i32, y: i32, z: i32) -> Option<String> {
        self.view_block(&dimension, x, y, z)
    }
    fn get_biome(&mut self, dimension: String, x: i32, y: i32, z: i32) -> Option<String> {
        self.view_biome(&dimension, x, y, z)
    }
    fn get_blocks(
        &mut self,
        dimension: String,
        positions: Vec<mod_world::BlockPos>,
    ) -> Vec<mod_world::BlockResult> {
        if positions.len() > BLOCKS_PER_CALL {
            return Vec::new();
        }
        let at: Vec<(i32, i32, i32)> = positions.iter().map(|p| (p.x, p.y, p.z)).collect();
        self.view_blocks(&dimension, &at)
            .into_iter()
            .zip(positions)
            .map(|(state, p)| mod_world::BlockResult {
                x: p.x,
                y: p.y,
                z: p.z,
                state,
            })
            .collect()
    }
    fn get_lights(
        &mut self,
        dimension: String,
        positions: Vec<mod_world::BlockPos>,
    ) -> Vec<mod_world::LightResult> {
        if positions.len() > BLOCKS_PER_CALL {
            return Vec::new();
        }
        let at: Vec<(i32, i32, i32)> = positions.iter().map(|p| (p.x, p.y, p.z)).collect();
        self.view_lights(&dimension, &at)
            .into_iter()
            .zip(positions)
            .map(|(light, p)| mod_world::LightResult {
                x: p.x,
                y: p.y,
                z: p.z,
                light: light.map(|(block, sky)| mod_world::LightValue { block, sky }),
            })
            .collect()
    }
    fn set_blocks(&mut self, dimension: String, blocks: Vec<BlockChange>) -> Result<(), String> {
        self.set_blocks_checked(dimension, blocks)
    }
}
impl players::Host for State {
    fn online(&mut self) -> Vec<ServerPlayer> {
        self.snapshot.players.clone()
    }
    fn tell(&mut self, player: String, text: String) -> bool {
        self.snapshot.players.iter().any(|p| p.id == player)
            && self.say(|name| Output::Tell {
                name,
                player,
                text: clip(text),
            })
    }
    fn broadcast(&mut self, text: String) {
        self.say(|name| Output::Broadcast {
            name,
            text: clip(text),
        });
    }
}
/// A key a mod registered (capability `input`).
#[derive(Clone, Debug, PartialEq)]
pub struct ModKey {
    /// The mod's own name for it, echoed in `key-down`.
    pub name: String,
    /// What it does, for Controls.
    pub label: String,
    /// `key.keyboard.r`.
    pub key: String,
    /// The key `register-key` gave, before the player's choice: what a
    /// reset restores. `rebind` never touches it.
    pub default: String,
}

/// What capability `input` holds for one mod.
#[derive(Default)]
struct InputState {
    claimed: Vec<String>,
    keys: Vec<ModKey>,
    events: std::collections::VecDeque<InputEvent>,
    /// The mod's keys held now, by name.
    down: Vec<String>,
    /// It holds the mouse wheel.
    wheel: bool,
    /// The keys the player chose, `name → key`: they win over
    /// `register-key`'s.
    bindings: Vec<(String, String)>,
}
/// Events kept for `poll`, items claimed and keys, per mod.
const INPUT_EVENTS: usize = 256;
const CLAIMS: usize = 64;
/// Keys one mod may register: 32 since API 19 (16 before; every
/// consumer is linear, so the number is a sanity bound only).
const KEYS: usize = 32;

/// `key.keyboard.<name>` or `key.mouse.<name>`, as options.txt writes keys;
/// the game knows which names exist and ignores a key it has none for.
fn key_ok(key: &str) -> bool {
    let rest = key
        .strip_prefix("key.keyboard.")
        .or_else(|| key.strip_prefix("key.mouse."));
    rest.is_some_and(|r| {
        (1..=32).contains(&r.len())
            && r.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.')
    })
}

impl InputState {
    /// Key `name` is `key` now; held, it lets go first.
    fn rebind(&mut self, name: &str, key: String) {
        let Some(k) = self.keys.iter_mut().find(|k| k.name == name) else {
            return;
        };
        k.key = key;
        if let Some(i) = self.down.iter().position(|n| n == name) {
            let name = self.down.swap_remove(i);
            self.push(InputEvent::KeyUp(name));
        }
    }
    /// Every key back to the default it registered with; the player's
    /// choices are forgotten. A held key lets go first, as `rebind` does.
    fn reset_keys(&mut self) {
        self.bindings.clear();
        let defaults: Vec<(String, String)> = self
            .keys
            .iter()
            .map(|k| (k.name.clone(), k.default.clone()))
            .collect();
        for (name, key) in defaults {
            self.rebind(&name, key);
        }
    }
    fn push(&mut self, event: InputEvent) {
        if self.events.len() == INPUT_EVENTS {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }
}

impl input::Host for State {
    fn claim_items(&mut self, ids: Vec<String>) {
        self.input.claimed = ids.into_iter().take(CLAIMS).map(clip).collect();
    }
    fn register_key(&mut self, name: String, label: String, key: String) -> Result<(), String> {
        if !key_ok(&key) {
            return Err(format!(
                "{key:?} is not a key as options.txt writes them (key.keyboard.r)"
            ));
        }
        let (name, label) = (clip(name), clip(label));
        let default = key.clone();
        let key = match self.input.bindings.iter().find(|(n, _)| *n == name) {
            Some((_, chosen)) => chosen.clone(),
            None => key,
        };
        if let Some(k) = self.input.keys.iter_mut().find(|k| k.name == name) {
            (k.label, k.key, k.default) = (label, key, default);
            return Ok(());
        }
        if self.input.keys.len() == KEYS {
            return Err(format!("a mod may register {KEYS} keys"));
        }
        self.input.keys.push(ModKey {
            name,
            label,
            key,
            default,
        });
        Ok(())
    }
    fn held(&mut self) -> Option<HeldItem> {
        self.snapshot.held.clone()
    }
    fn poll(&mut self) -> Vec<InputEvent> {
        self.input.events.drain(..).collect()
    }
    fn mapping(&mut self, name: String) -> Option<input::MappingState> {
        let (_, label, down) = self.snapshot.mappings.iter().find(|m| m.0 == name)?;
        Some(input::MappingState {
            label: label.clone(),
            down: *down,
        })
    }
    fn clicks_per_second(&mut self) -> (u32, u32) {
        self.snapshot.cps
    }
    fn last_input_ms(&mut self) -> u64 {
        self.snapshot.last_input_ms
    }
    fn claim_wheel(&mut self, on: bool) {
        self.input.wheel = on;
    }
}

/// HUD shapes one mod may keep (capability `render`).
const SHAPES: usize = 1024;
/// World-shape lines one mod may keep (capability `render`): a box
/// counts as its twelve edges.
pub const WORLD_SHAPE_LINES: usize = 1024;
/// A world line's width in pixels.
pub const WORLD_LINE_WIDTH: (f32, f32) = (0.5, 8.0);

/// One depth-tested world line, as the game's gizmos draw it: the ends
/// in blocks, a 0xAARRGGBB colour, a width in pixels.
pub type WorldDrawLine = ([f64; 3], [f64; 3], u32, f32);
/// The longest jolt `render.shake` makes, in seconds.
const SHAKE_SECONDS: f32 = 10.0;
/// `render.set-zoom`'s limits: the factor's cap (250 since API 19,
/// 10 before — the camera tick's floor now follows a divided target
/// below 0.1, so a smooth zoom can pass 10x; a finite cap keeps the
/// projection away from the degenerate FOV → 0 region), and turning
/// never stops.
const ZOOM_MAX: f32 = 250.0;
const TURN_MIN: f32 = 0.05;
/// `render.set-zoom`'s factor, sanitized: inside 1.0..=[`ZOOM_MAX`];
/// a non-finite factor reads as 1.0, no zoom.
fn clamp_zoom_factor(factor: f32) -> f32 {
    if factor.is_finite() {
        factor.clamp(1.0, ZOOM_MAX)
    } else {
        1.0
    }
}
/// The range a hand FOV may sit in, in degrees (API 19): what a
/// projection can use — below it the hand inverts, above it the
/// hand all but vanishes.
const HAND_FOV_RANGE: (f32, f32) = (5.0, 175.0);
/// Vanilla's hand FOV, the base a non-finite one reads as.
const HAND_FOV_DEFAULT: f32 = 70.0;
/// `render.set-hand-fov`'s value, sanitized: the base inside
/// [`HAND_FOV_RANGE`]; a non-finite base reads as vanilla's 70.
fn sanitize_hand_fov(hand_fov: ModHandFov) -> ModHandFov {
    ModHandFov {
        base: if hand_fov.base.is_finite() {
            hand_fov.base.clamp(HAND_FOV_RANGE.0, HAND_FOV_RANGE.1)
        } else {
            HAND_FOV_DEFAULT
        },
        ..hand_fov
    }
}
/// The hand FOV a `set-hand-fov` setting (API 19) works out to for a
/// frame, in degrees, before the death/fluid distortion the game
/// applies last: `ratio` is the view's zoom ratio (1 unzoomed,
/// 1/factor settled at a zoom of factor) and `zoomed_view` the
/// view's field of view as rendered, so the unzoomed view is
/// `zoomed_view / ratio`. Settled at factor F, multiply gives
/// base / F and add gives base − V(1 − 1/F) with V the unzoomed
/// view; at ratio 1 both give the base exactly. The spyglass's
/// narrowing is never in the ratio (the game's zoom is off while
/// scoping), so the hand does not follow the spyglass. The result
/// stays inside [`HAND_FOV_RANGE`]; a non-finite one reads as 70.
pub fn hand_fov_base(setting: &ModHandFov, ratio: f32, zoomed_view: f32) -> f32 {
    let ratio = if ratio.is_finite() && ratio > 0.0 {
        ratio
    } else {
        1.0
    };
    let view = zoomed_view / ratio;
    let base = match setting.mode {
        ModHandFovMode::Multiply => setting.base * ratio,
        ModHandFovMode::Add => setting.base - view * (1.0 - ratio),
        ModHandFovMode::Keep => setting.base,
    };
    if base.is_finite() {
        base.clamp(HAND_FOV_RANGE.0, HAND_FOV_RANGE.1)
    } else {
        HAND_FOV_DEFAULT
    }
}
/// `render.set-fps-cap`'s value, sanitized: 1..=240 — 0 reads as 1,
/// the deepest tier (a literal 0 would divide by zero in pacing).
fn sanitize_fps_cap(fps: Option<u32>) -> Option<u32> {
    fps.map(|f| f.clamp(1, 240))
}
/// `set-minimap`'s side in GUI pixels and its blocks per map pixel.
const MINIMAP_SIZE: (f32, f32) = (16.0, 256.0);
const MINIMAP_SCALE: (f32, f32) = (0.5, 4.0);
/// `render.set-gamma`'s limits: 0.0 the darkest the lightmap goes, 10.0
/// the 1000% Fullbright value. A non-finite value reads as 1.0, not 0.0,
/// which would black the screen on a NaN.
const GAMMA_MAX: f32 = 10.0;
fn clamp_gamma(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, GAMMA_MAX)
    } else {
        1.0
    }
}
/// The strongest of the gammas running mods set, if any.
fn strongest_gamma(values: impl Iterator<Item = f32>) -> Option<f32> {
    values.max_by(|a, b| a.total_cmp(b))
}
/// Whether any running mod turned distance fog off.
fn any_fog_off(mut values: impl Iterator<Item = bool>) -> bool {
    values.any(|fog| !fog)
}
/// `render.set-camera`'s distance cap, in blocks from the player's feet:
/// a freecam cannot scout past it. The offset is scaled back to the cap,
/// so the camera stops at the sphere instead of snapping back to the
/// player.
const CAMERA_DISTANCE_MAX: f64 = 64.0;
/// The player's eye above their feet, for a freelook camera and for
/// `get-camera`'s fallback when the last frame's camera is unknown.
const CAMERA_EYE_HEIGHT: f64 = 1.62;
/// `render.set-camera`'s value with the look wrapped/clamped and the
/// position capped at [`CAMERA_DISTANCE_MAX`] from `feet` (the player's
/// feet, when the game knows them): None when a non-finite position
/// drops the whole call — the previous override, if any, stays.
fn sanitize_camera(camera: ModCameraOverride, feet: Option<[f64; 3]>) -> Option<ModCameraOverride> {
    let position = match camera.position {
        None => None,
        Some(p) if p.x.is_finite() && p.y.is_finite() && p.z.is_finite() => {
            Some(cap_camera_position(p, feet))
        }
        Some(_) => return None,
    };
    // Yaw wraps (a mod turning past 180 keeps turning); pitch and roll
    // clamp; a non-finite look reads as 0.0 for that field, so a NaN
    // never poisons the view matrix.
    let yaw = if camera.yaw.is_finite() {
        (camera.yaw + 180.0).rem_euclid(360.0) - 180.0
    } else {
        0.0
    };
    let finite = |v: f32, lo: f32, hi: f32| if v.is_finite() { v.clamp(lo, hi) } else { 0.0 };
    Some(ModCameraOverride {
        position,
        yaw,
        pitch: finite(camera.pitch, -90.0, 90.0),
        roll: finite(camera.roll, -180.0, 180.0),
    })
}
/// A freecam position scaled back to the [`CAMERA_DISTANCE_MAX`] sphere
/// around `feet`, when it is past it.
fn cap_camera_position(p: WorldPoint, feet: Option<[f64; 3]>) -> WorldPoint {
    let Some(feet) = feet else { return p };
    let offset = [p.x - feet[0], p.y - feet[1], p.z - feet[2]];
    let len = (offset[0] * offset[0] + offset[1] * offset[1] + offset[2] * offset[2]).sqrt();
    if len > CAMERA_DISTANCE_MAX {
        let s = CAMERA_DISTANCE_MAX / len;
        WorldPoint {
            x: feet[0] + offset[0] * s,
            y: feet[1] + offset[1] * s,
            z: feet[2] + offset[2] * s,
        }
    } else {
        p
    }
}
/// The camera `get-camera` / `project` read: the last frame's rendered
/// camera when the game filled one in, else the winning override, else
/// this mod's own just-set override, else the player's own eye.
fn effective_camera(
    snapshot_camera: Option<SnapshotCamera>,
    aggregate_camera: Option<ModCameraOverride>,
    own_camera: Option<ModCameraOverride>,
    player: Option<&Player>,
) -> Option<SnapshotCamera> {
    if let Some(camera) = snapshot_camera {
        return Some(camera);
    }
    let pose = |camera: ModCameraOverride| {
        let (x, y, z) = match camera.position {
            Some(p) => (p.x, p.y, p.z),
            // Freelook: the camera stays at the player's eye.
            None => player.map_or((0.0, 0.0, 0.0), |p| (p.x, p.y + CAMERA_EYE_HEIGHT, p.z)),
        };
        SnapshotCamera {
            x,
            y,
            z,
            yaw: camera.yaw,
            pitch: camera.pitch,
            roll: camera.roll,
            // The game's fill carries the zoomed field of view; before
            // the first frame there is no zoom to read, so the default.
            fov: 70.0,
        }
    };
    aggregate_camera.or(own_camera).map(pose).or_else(|| {
        player.map(|p| SnapshotCamera {
            x: p.x,
            y: p.y + CAMERA_EYE_HEIGHT,
            z: p.z,
            yaw: p.yaw,
            pitch: p.pitch,
            roll: 0.0,
            fov: 70.0,
        })
    })
}
/// `render.project`'s world-to-screen projection, through `camera`:
/// the locator bar's (`Camera.getViewRotationProjectionMatrix`, through
/// view space), extended to the width with the aspect ratio and to the
/// mod roll. No bob: a sub-degree wobble the label should not inherit.
/// `visible` is false behind the camera; `x` and `y` are still given,
/// so a mod can draw an off-screen arrow itself.
fn project_point(camera: &SnapshotCamera, gui: (f32, f32), point: WorldPoint) -> ModScreenPoint {
    if !(point.x.is_finite() && point.y.is_finite() && point.z.is_finite()) {
        return ModScreenPoint {
            x: 0.0,
            y: 0.0,
            visible: false,
            depth: 0.0,
        };
    }
    let (w, h) = (f64::from(gui.0), f64::from(gui.1));
    let aspect = if h > 0.0 { w / h } else { 1.0 };
    let fov = if camera.fov.is_finite() && (0.0..180.0).contains(&f64::from(camera.fov)) {
        f64::from(camera.fov)
    } else {
        70.0
    };
    let focal = 1.0 / (fov / 2.0).to_radians().tan();
    let o = [point.x - camera.x, point.y - camera.y, point.z - camera.z];
    let (yaw, pitch) = (
        f64::from(camera.yaw).to_radians(),
        f64::from(camera.pitch).to_radians(),
    );
    let (sy, cy, sp, cp) = (yaw.sin(), yaw.cos(), pitch.sin(), pitch.cos());
    // The locator bar's basis: forward and up from yaw and pitch, right
    // as forward × up (the game's `Camera.left`, negated).
    let forward = [-sy * cp, -sp, cy * cp];
    let up0 = [-sy * sp, cp, cy * sp];
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let right0 = cross(forward, up0);
    // Roll: the rendered pose rotates the view by -roll about the view
    // direction (positive roll banks the horizon clockwise as the player
    // sees it), so the basis rotates the same way.
    let roll = -f64::from(camera.roll).to_radians();
    let (sr, cr) = (roll.sin(), roll.cos());
    let scale = |a: [f64; 3], s: f64| [a[0] * s, a[1] * s, a[2] * s];
    let add = |a: [f64; 3], b: [f64; 3]| [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
    let right = add(scale(right0, cr), scale(up0, -sr));
    let up = add(scale(right0, sr), scale(up0, cr));
    let dot = |a: [f64; 3]| a[0] * o[0] + a[1] * o[1] + a[2] * o[2];
    let depth = dot(forward);
    // A point dead to the side has no depth: divide by a breath instead
    // of zero, so it projects far off-screen rather than to infinity.
    let d = if depth == 0.0 { 1e-9 } else { depth };
    ModScreenPoint {
        x: (((dot(right) * focal / (d * aspect) + 1.0) / 2.0) * w) as f32,
        y: (((1.0 - dot(up) * focal / d) / 2.0) * h) as f32,
        visible: depth > 0.0,
        depth,
    }
}
/// `set-far-terrain`'s distance in chunks, as the LOD Distance setting's.
const FAR_TERRAIN_DISTANCE: (u32, u32) = (64, 1024);

/// A mod's corner panel: shapes in a `width` × `height` box the game
/// places in `corner`, stacked with the other overlays.
#[derive(Clone, Debug)]
pub struct CornerPanel {
    pub corner: RenderCorner,
    pub width: f32,
    pub height: f32,
    pub shapes: Vec<Shape>,
}

fn clip_shapes(shapes: Vec<Shape>) -> Vec<Shape> {
    shapes
        .into_iter()
        .take(SHAPES)
        .map(|shape| match shape {
            Shape::Sprite(mut s) => {
                s.sprite = clip(s.sprite);
                Shape::Sprite(s)
            }
            Shape::Item(mut i) => {
                i.item = clip(i.item);
                i.scale = if i.scale.is_finite() {
                    i.scale.clamp(0.0, 16.0)
                } else {
                    1.0
                };
                Shape::Item(i)
            }
            Shape::Text(mut t) => {
                t.text = clip(t.text);
                t.color &= 0xFF_FFFF;
                Shape::Text(t)
            }
            other => other,
        })
        .collect()
}

/// The twelve edges of a world box, in `CuboidGizmo.emit`'s order — the
/// same order `horizon_client::gizmos::Gizmos::cuboid` draws them.
fn box_edges(min: [f64; 3], max: [f64; 3]) -> [([f64; 3], [f64; 3]); 12] {
    let [x0, y0, z0] = min;
    let [x1, y1, z1] = max;
    [
        ([x0, y0, z0], [x1, y0, z0]),
        ([x0, y0, z0], [x0, y1, z0]),
        ([x0, y0, z0], [x0, y0, z1]),
        ([x1, y0, z0], [x1, y1, z0]),
        ([x1, y1, z0], [x0, y1, z0]),
        ([x0, y1, z0], [x0, y1, z1]),
        ([x0, y1, z1], [x0, y0, z1]),
        ([x0, y0, z1], [x1, y0, z1]),
        ([x1, y0, z1], [x1, y0, z0]),
        ([x0, y1, z1], [x1, y1, z1]),
        ([x1, y0, z1], [x1, y1, z1]),
        ([x1, y1, z0], [x1, y1, z1]),
    ]
}

/// Clips a `render.set-world` submission to the line budget (a box
/// counts 12): shapes past the budget are dropped, not rejected, and a
/// shape that does not fit is skipped while a later cheaper one still
/// fits. Non-finite coordinates drop the shape (a NaN end would poison
/// the vertex buffer); widths clamp to [`WORLD_LINE_WIDTH`], a
/// non-finite one reading 1.
fn clip_world_shapes(shapes: Vec<WorldShape>) -> Vec<WorldShape> {
    let mut budget = WORLD_SHAPE_LINES;
    let mut out = Vec::new();
    let width = |w: f32| {
        if w.is_finite() {
            w.clamp(WORLD_LINE_WIDTH.0, WORLD_LINE_WIDTH.1)
        } else {
            1.0
        }
    };
    let point_ok = |p: &WorldPoint| p.x.is_finite() && p.y.is_finite() && p.z.is_finite();
    for shape in shapes {
        let cost = match &shape {
            WorldShape::Line(_) => 1,
            WorldShape::Box(_) => 12,
        };
        if cost > budget {
            continue;
        }
        let shape = match shape {
            WorldShape::Line(mut l) => {
                if !point_ok(&l.start) || !point_ok(&l.end) {
                    continue;
                }
                l.width = width(l.width);
                WorldShape::Line(l)
            }
            WorldShape::Box(mut b) => {
                if !point_ok(&b.min) || !point_ok(&b.max) {
                    continue;
                }
                b.width = width(b.width);
                WorldShape::Box(b)
            }
        };
        budget -= cost;
        out.push(shape);
    }
    out
}

/// A mod's world shapes as depth-tested lines: one line per line, a
/// box's twelve edges in [`box_edges`] order. This is the expansion the
/// game drains into the frame's lines, so the sandbox's edge-order
/// check covers the real drain.
pub fn world_shape_lines(shapes: &[WorldShape]) -> Vec<WorldDrawLine> {
    let mut out = Vec::new();
    for shape in shapes {
        match shape {
            WorldShape::Line(l) => out.push((
                [l.start.x, l.start.y, l.start.z],
                [l.end.x, l.end.y, l.end.z],
                l.color,
                l.width,
            )),
            WorldShape::Box(b) => {
                let min = [b.min.x, b.min.y, b.min.z];
                let max = [b.max.x, b.max.y, b.max.z];
                out.extend(
                    box_edges(min, max)
                        .iter()
                        .map(|&(start, end)| (start, end, b.color, b.width)),
                );
            }
        }
    }
    out
}

impl render::Host for State {
    fn set_hud(&mut self, shapes: Vec<Shape>) {
        self.shapes = clip_shapes(shapes);
    }
    fn set_world(&mut self, shapes: Vec<WorldShape>) {
        self.world_shapes = clip_world_shapes(shapes);
    }
    fn set_corner(&mut self, corner: RenderCorner, width: f32, height: f32, shapes: Vec<Shape>) {
        let size = |v: f32| {
            if v.is_finite() {
                v.clamp(0.0, 1024.0)
            } else {
                0.0
            }
        };
        self.corner = Some(CornerPanel {
            corner,
            width: size(width),
            height: size(height),
            shapes: clip_shapes(shapes),
        });
    }
    fn clear_corner(&mut self) {
        self.corner = None;
    }
    fn text_width(&mut self, text: String) -> f32 {
        let widths = &self.snapshot.font_widths;
        text.chars()
            .map(|c| widths.get(c as usize).copied().unwrap_or(6.0))
            .sum::<f32>()
            .ceil()
    }
    fn set_zoom(&mut self, zoom: ModZoom) {
        let factor = clamp_zoom_factor(zoom.factor);
        self.zoom = (factor > 1.0).then(|| ModZoom {
            factor,
            smooth: zoom.smooth,
            turn: if zoom.turn.is_finite() {
                zoom.turn.clamp(TURN_MIN, 1.0)
            } else {
                1.0
            },
        });
    }
    fn set_minimap(&mut self, minimap: Option<ModMinimap>) {
        let finite = |v: f32, lo: f32, hi: f32| if v.is_finite() { v.clamp(lo, hi) } else { lo };
        self.minimap = minimap.map(|m| ModMinimap {
            size: finite(m.size, MINIMAP_SIZE.0, MINIMAP_SIZE.1),
            blocks_per_pixel: finite(m.blocks_per_pixel, MINIMAP_SCALE.0, MINIMAP_SCALE.1),
            ..m
        });
    }
    fn set_far_terrain(&mut self, far_terrain: Option<ModFarTerrain>) {
        self.far_terrain = far_terrain.map(|f| ModFarTerrain {
            distance: f
                .distance
                .clamp(FAR_TERRAIN_DISTANCE.0, FAR_TERRAIN_DISTANCE.1),
            ..f
        });
    }
    fn get_gamma(&mut self) -> f32 {
        // The effective value: the strongest any running mod set (this
        // mod's own, if it set one after the call started, included), else
        // the player's stored Brightness.
        [self.aggregate_gamma, self.gamma]
            .into_iter()
            .flatten()
            .max_by(|a, b| a.total_cmp(b))
            .unwrap_or(self.snapshot.gamma)
    }
    fn set_gamma(&mut self, value: f32) {
        self.gamma = Some(clamp_gamma(value));
    }
    fn get_fog(&mut self) -> bool {
        self.snapshot.fog && !self.aggregate_fog_off && self.fog != Some(false)
    }
    fn set_fog(&mut self, enabled: bool) {
        self.fog = Some(enabled);
    }
    fn get_camera(&mut self) -> ModCameraPose {
        // The effective camera: the last frame's rendered one (which is
        // the mouse-updated look, when a camera is active), else the
        // winning override, else the player's own eye. Before the first
        // frame there is no rendered camera, and on the title screen no
        // player, so the pose is all zeros there.
        match effective_camera(
            self.snapshot.camera,
            self.aggregate_camera,
            self.camera,
            self.snapshot.player.as_ref(),
        ) {
            Some(c) => ModCameraPose {
                x: c.x,
                y: c.y,
                z: c.z,
                yaw: c.yaw,
                pitch: c.pitch,
                roll: c.roll,
            },
            None => ModCameraPose {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                yaw: 0.0,
                pitch: 0.0,
                roll: 0.0,
            },
        }
    }
    fn set_camera(&mut self, camera: Option<ModCameraOverride>) {
        // An accepted call counts even when the value is unchanged: the
        // game re-reads the look on every count, so a mod calling this
        // every tick with fixed angles keeps the mouse from drifting it.
        // A non-finite position drops the call instead, and the previous
        // override stays — the count does not move for a dropped call.
        // A set keeps the aggregate live within the call when this mod
        // is (or becomes) the winner, so a `get-camera` later in the
        // same call already sees the new value, not the one the call
        // started with. Releasing as the winner drops the aggregate
        // this call knew — the next winner is only known to `Mods`,
        // which refreshes it before the next call.
        match camera {
            None => {
                self.camera = None;
                self.camera_seq += 1;
                if self.aggregate_is_mine {
                    self.aggregate_camera = None;
                    self.aggregate_is_mine = false;
                }
            }
            Some(camera) => {
                let feet = self.snapshot.player.as_ref().map(|p| [p.x, p.y, p.z]);
                if let Some(camera) = sanitize_camera(camera, feet) {
                    self.camera = Some(camera);
                    self.camera_seq += 1;
                    if self.aggregate_camera.is_none() || self.aggregate_is_mine {
                        self.aggregate_camera = self.camera;
                        self.aggregate_is_mine = true;
                    }
                }
            }
        }
    }
    fn project(&mut self, point: WorldPoint) -> ModScreenPoint {
        match effective_camera(
            self.snapshot.camera,
            self.aggregate_camera,
            self.camera,
            self.snapshot.player.as_ref(),
        ) {
            Some(camera) => project_point(&camera, self.snapshot.gui_size, point),
            None => ModScreenPoint {
                x: 0.0,
                y: 0.0,
                visible: false,
                depth: 0.0,
            },
        }
    }
    fn frame_stats(&mut self) -> FrameRate {
        let (fps, min, low) = self.snapshot.frame;
        FrameRate { fps, min, low }
    }
    fn can_zoom(&mut self) -> bool {
        self.snapshot.can_zoom
    }
    fn get_spyglass_zoom(&mut self) -> Option<f32> {
        self.snapshot.spyglass_zoom
    }
    fn set_hide_hud(&mut self, hidden: bool) {
        self.hide_hud = hidden;
    }
    fn set_hand_fov(&mut self, hand_fov: Option<ModHandFov>) {
        self.hand_fov = hand_fov.map(sanitize_hand_fov);
    }
    fn set_fps_cap(&mut self, fps: Option<u32>) {
        self.fps_cap = sanitize_fps_cap(fps);
    }
    fn gui_size(&mut self) -> (f32, f32) {
        self.snapshot.gui_size
    }
    fn shake(&mut self, strength: f32, seconds: f32) {
        let ok = |v: f32, max: f32| {
            if v.is_finite() {
                v.clamp(0.0, max)
            } else {
                0.0
            }
        };
        let (strength, seconds) = (ok(strength, 1.0), ok(seconds, SHAKE_SECONDS));
        if strength > 0.0 && seconds > 0.0 {
            self.out.push(Output::Shake { strength, seconds });
        }
    }
}

impl command::Host for State {
    fn run(&mut self, line: String) {
        let queued = self
            .out
            .iter()
            .filter(|o| matches!(o, Output::Command { .. }))
            .count();
        if queued < CHAT_PER_CALL {
            self.out.push(Output::Command { line: clip(line) });
        }
    }
    fn run_quiet(&mut self, lines: Vec<String>) {
        let queued: usize = self
            .out
            .iter()
            .map(|o| match o {
                Output::QuietCommands { lines, .. } => lines.len(),
                _ => 0,
            })
            .sum();
        let room = QUIET_PER_CALL.saturating_sub(queued);
        if room > 0 && !lines.is_empty() {
            self.out.push(Output::QuietCommands {
                mod_id: self.id.clone(),
                lines: lines.into_iter().take(room).map(clip).collect(),
            });
        }
    }
}
impl hud::Host for State {
    fn set_panel(&mut self, corner: Corner, lines: Vec<Line>) {
        let lines = lines
            .into_iter()
            .take(HUD_LINES)
            .map(|l| Line {
                text: clip(l.text),
                color: l.color & 0xFF_FFFF,
            })
            .collect();
        self.panel = Some(Panel { corner, lines });
    }
    fn clear(&mut self) {
        self.panel = None;
    }
}
impl ui::Host for State {
    fn open_screen(&mut self, mut screen: ui::Screen) {
        screen.title = clip(screen.title);
        screen.widgets = clip_widgets(screen.widgets);
        self.ui.screen = Some(screen);
        self.ui.open = true;
    }
    fn close_screen(&mut self) {
        self.ui.screen = None;
        self.ui.close = true;
    }
    fn set_settings(&mut self, widgets: Vec<ui::Widget>) {
        self.ui.settings = clip_widgets(widgets);
    }
}
impl storage::Host for State {
    fn get(&mut self, key: String) -> Option<Vec<u8>> {
        storage_key_ok(&key)
            .then(|| std::fs::read(self.data_dir.join(&key)).ok())
            .flatten()
    }
    fn set(&mut self, key: String, value: Vec<u8>) -> Result<(), String> {
        if !storage_key_ok(&key) {
            return Err(format!("{key:?} is not [a-z0-9_.-]{{1,64}}"));
        }
        let len = value.len() as u64;
        if len > self.storage_budget {
            return Err("too much storage traffic in one call".into());
        }
        // Everything already on disk but the file this call replaces.
        let existing: Vec<std::fs::DirEntry> = std::fs::read_dir(&self.data_dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy() != key)
            .collect();
        if existing.len() >= STORAGE_MAX_KEYS {
            return Err(format!("would hold over {STORAGE_MAX_KEYS} keys"));
        }
        let used: u64 = existing
            .iter()
            .filter_map(|e| e.metadata().ok())
            .map(|m| m.len())
            .sum();
        if used + len > STORAGE_QUOTA {
            return Err(format!(
                "would use over {} MiB of storage",
                STORAGE_QUOTA >> 20
            ));
        }
        std::fs::create_dir_all(&self.data_dir).map_err(|e| e.to_string())?;
        std::fs::write(self.data_dir.join(&key), value).map_err(|e| e.to_string())?;
        self.storage_budget -= len;
        Ok(())
    }
    fn delete(&mut self, key: String) {
        if storage_key_ok(&key) {
            let _ = std::fs::remove_file(self.data_dir.join(&key));
        }
    }
}

/// A storage key: one path segment of `[a-z0-9_.-]{1,64}`, not `.` or `..`.
/// No `/`, so a key can never leave the mod's own directory.
fn storage_key_ok(key: &str) -> bool {
    let chars_ok = (1..=64).contains(&key.len())
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-".contains(&b));
    chars_ok && key != "." && key != ".."
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A view that answers with its own name, so a test can tell which
    /// view the slot holds.
    struct NamedView(&'static str);
    impl WorldView for NamedView {
        fn block(&self, _dimension: &str, _x: i32, _y: i32, _z: i32) -> Option<String> {
            Some(self.0.to_string())
        }
        fn biome(&self, _dimension: &str, _x: i32, _y: i32, _z: i32) -> Option<String> {
            Some(self.0.to_string())
        }
        fn light(&self, _dimension: &str, _x: i32, _y: i32, _z: i32) -> Option<(u8, u8)> {
            Some((0, 0))
        }
        fn has_dimension(&self, _dimension: &str) -> bool {
            true
        }
        fn check_state(&self, _state: &str) -> Result<(), String> {
            Ok(())
        }
    }

    /// The shared `WorldSlot` pairing the spike calls the biggest risk:
    /// one slot, one view at a time, lent and taken back in strict pairs.
    /// A client mod calling `world` during a server tick would read the
    /// server's view — so the game must never have both lent at once, and
    /// must take back what it lent before the other tick lends its own.
    /// This test proves the slot's half of that: a take returns exactly
    /// the view that was lent, and after the take the slot is empty.
    /// The other half — that the game's ticks lend and return in that
    /// order, on one thread — is an inspected invariant of `Game::tick`
    /// (`tick_mods` then `tick_server_mods`, each paired), not a test:
    /// in the default server-thread singleplayer the two ticks do not
    /// even share a `Mods` instance.
    #[test]
    fn the_world_slot_holds_one_view_lent_and_taken_in_pairs() {
        let mut mods = Mods::default();
        assert!(mods.take_world().is_none(), "nothing lent yet");
        mods.lend_world(Box::new(NamedView("client")));
        let view = mods.take_world().expect("the lent client view");
        let view: Box<dyn std::any::Any> = view;
        assert_eq!(view.downcast::<NamedView>().unwrap().0, "client");
        assert!(mods.take_world().is_none(), "taken back: the slot is empty");
        // The server tick lends its own view next, and takes it back the same way.
        mods.lend_world(Box::new(NamedView("server")));
        let view = mods.take_world().expect("the lent server view");
        let view: Box<dyn std::any::Any> = view;
        assert_eq!(view.downcast::<NamedView>().unwrap().0, "server");
        assert!(mods.take_world().is_none());
    }

    /// Lending twice without a take in between replaces the first view:
    /// the game must pair every lend with a take before the next lend, or
    /// the first view is dropped — for the server's view, its chunk maps
    /// with it. The client lend guards this with `client_world_lent`.
    #[test]
    fn lending_twice_without_a_take_replaces_the_first_view() {
        let mut mods = Mods::default();
        mods.lend_world(Box::new(NamedView("first")));
        mods.lend_world(Box::new(NamedView("second")));
        let view = mods.take_world().expect("a view");
        let view: Box<dyn std::any::Any> = view;
        assert_eq!(view.downcast::<NamedView>().unwrap().0, "second");
        assert!(mods.take_world().is_none());
    }

    /// A mod key as `register-key` leaves it: the key is its own default.
    fn mod_key(name: &str, label: &str, key: &str) -> ModKey {
        ModKey {
            name: name.to_owned(),
            label: label.to_owned(),
            key: key.to_owned(),
            default: key.to_owned(),
        }
    }

    /// Resetting restores the defaults the mod registered, not the keys
    /// the player chose — and forgets those choices, so a later
    /// `register-key` sees the default again.
    #[test]
    fn reset_keys_restores_the_registered_defaults() {
        let mut input = InputState::default();
        input
            .keys
            .push(mod_key("zoom", "Zoom key", "key.keyboard.v"));
        input
            .keys
            .push(mod_key("recipes", "Show recipes", "key.keyboard.r"));
        input
            .bindings
            .push(("zoom".to_owned(), "key.keyboard.b".to_owned()));
        input.rebind("zoom", "key.keyboard.b".to_owned());
        assert_eq!(input.keys[0].key, "key.keyboard.b");
        input.reset_keys();
        assert_eq!(input.keys[0].key, "key.keyboard.v");
        assert_eq!(input.keys[0].default, "key.keyboard.v");
        assert_eq!(input.keys[1].key, "key.keyboard.r");
        assert!(input.bindings.is_empty());
    }

    /// A held key lets go on reset, as it does on rebind: the mod hears
    /// `key-up` instead of the key staying down under the default.
    #[test]
    fn reset_keys_lets_go_of_a_held_key() {
        let mut input = InputState::default();
        input
            .keys
            .push(mod_key("zoom", "Zoom key", "key.keyboard.v"));
        input.down.push("zoom".to_owned());
        input.reset_keys();
        assert!(input.down.is_empty());
        assert!(input.events.iter().any(|e| matches!(
            e,
            InputEvent::KeyUp(name) if name == "zoom"
        )));
    }

    /// Resetting a mod that isn't running changes nothing and says so.
    #[test]
    fn reset_keys_of_a_mod_that_isnt_running_is_false() {
        let mut mods = Mods::default();
        assert!(!mods.reset_keys(0));
    }

    fn world_point(x: f64, y: f64, z: f64) -> WorldPoint {
        WorldPoint { x, y, z }
    }
    fn world_line(width: f32) -> WorldShape {
        WorldShape::Line(WorldLine {
            start: world_point(0.0, 64.0, 0.0),
            end: world_point(1.0, 64.0, 0.0),
            color: 0xFF00_FF00,
            width,
        })
    }
    fn world_box(width: f32) -> WorldShape {
        WorldShape::Box(WorldBox {
            min: world_point(0.0, 64.0, 0.0),
            max: world_point(1.0, 65.0, 1.0),
            color: 0xFFFF_0000,
            width,
        })
    }

    /// A `set-world` submission clips to the line budget (a box counts
    /// 12), dropping past it rather than refusing it, while a cheaper
    /// shape after the cut still fits: 200 boxes leave 85 (1020 lines)
    /// and the line after them makes 1021.
    #[test]
    fn world_shapes_clip_to_the_line_budget() {
        let mut shapes: Vec<WorldShape> = (0..200).map(|_| world_box(1.0)).collect();
        shapes.push(world_line(1.0));
        let clipped = clip_world_shapes(shapes);
        assert_eq!(clipped.len(), 86);
        assert_eq!(world_shape_lines(&clipped).len(), 1021);
    }

    /// Widths clamp to 0.5–8, a non-finite one reading 1, and a shape
    /// with a non-finite coordinate is dropped, not drawn.
    #[test]
    fn world_shapes_clamp_widths_and_drop_non_finite_coordinates() {
        let clipped = clip_world_shapes(vec![
            world_line(0.1),
            world_line(99.0),
            world_line(f32::NAN),
        ]);
        let widths: Vec<f32> = clipped
            .iter()
            .map(|s| match s {
                WorldShape::Line(l) => l.width,
                WorldShape::Box(_) => unreachable!(),
            })
            .collect();
        assert_eq!(widths, vec![0.5, 8.0, 1.0]);
        let nan = WorldShape::Line(WorldLine {
            start: world_point(f64::NAN, 64.0, 0.0),
            end: world_point(1.0, 64.0, 0.0),
            color: 0xFF00_FF00,
            width: 1.0,
        });
        assert!(clip_world_shapes(vec![nan]).is_empty());
    }

    /// A box expands to its twelve edges in `CuboidGizmo.emit`'s order:
    /// the three from the min corner, then round the box.
    #[test]
    fn a_world_box_expands_to_its_twelve_edges_in_cuboid_order() {
        let boxed = WorldShape::Box(WorldBox {
            min: world_point(0.0, 0.0, 0.0),
            max: world_point(1.0, 2.0, 3.0),
            color: 0xFFFF_0000,
            width: 2.0,
        });
        let lines = world_shape_lines(std::slice::from_ref(&boxed));
        let edges: Vec<([f64; 3], [f64; 3])> = lines
            .iter()
            .map(|(start, end, _, _)| (*start, *end))
            .collect();
        assert_eq!(
            edges,
            vec![
                ([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
                ([0.0, 0.0, 0.0], [0.0, 2.0, 0.0]),
                ([0.0, 0.0, 0.0], [0.0, 0.0, 3.0]),
                ([1.0, 0.0, 0.0], [1.0, 2.0, 0.0]),
                ([1.0, 2.0, 0.0], [0.0, 2.0, 0.0]),
                ([0.0, 2.0, 0.0], [0.0, 2.0, 3.0]),
                ([0.0, 2.0, 3.0], [0.0, 0.0, 3.0]),
                ([0.0, 0.0, 3.0], [1.0, 0.0, 3.0]),
                ([1.0, 0.0, 3.0], [1.0, 0.0, 0.0]),
                ([0.0, 2.0, 3.0], [1.0, 2.0, 3.0]),
                ([1.0, 0.0, 3.0], [1.0, 2.0, 3.0]),
                ([1.0, 2.0, 0.0], [1.0, 2.0, 3.0]),
            ]
        );
        assert!(
            lines
                .iter()
                .all(|(_, _, color, width)| *color == 0xFFFF_0000 && *width == 2.0)
        );
    }

    /// `set-gamma` clamps to 0.0..=10.0, and a non-finite value reads as
    /// 1.0 — not 0.0, which would black the screen on a NaN.
    #[test]
    fn set_gamma_clamps_and_reads_non_finite_as_one() {
        assert_eq!(clamp_gamma(11.0), 10.0);
        assert_eq!(clamp_gamma(-1.0), 0.0);
        assert_eq!(clamp_gamma(f32::NAN), 1.0);
        assert_eq!(clamp_gamma(f32::INFINITY), 1.0);
        assert_eq!(clamp_gamma(f32::NEG_INFINITY), 1.0);
        assert_eq!(clamp_gamma(0.5), 0.5);
    }

    /// The gamma aggregate is the strongest any running mod sets; with
    /// none set there is no aggregate, and the stored setting applies.
    #[test]
    fn the_gamma_aggregate_is_the_strongest_set() {
        assert_eq!(strongest_gamma([2.0, 5.0].into_iter()), Some(5.0));
        assert_eq!(strongest_gamma([2.0].into_iter()), Some(2.0));
        assert_eq!(strongest_gamma([].into_iter()), None);
    }

    /// The fog aggregate is off when any running mod turns it off; a mod
    /// leaving fog on does not re-enable it while another holds it off.
    #[test]
    fn the_fog_aggregate_is_off_when_any_mod_turns_it_off() {
        assert!(any_fog_off([true, false].into_iter()));
        assert!(!any_fog_off([true, true].into_iter()));
        assert!(!any_fog_off([].into_iter()));
    }

    /// `set-zoom`'s factor clamps to the API 19 cap of 250 (10 before
    /// it), and a non-finite factor reads as 1.0, no zoom.
    #[test]
    fn set_zoom_clamps_the_factor_to_250() {
        assert_eq!(clamp_zoom_factor(300.0), 250.0);
        assert_eq!(clamp_zoom_factor(250.0), 250.0);
        assert_eq!(clamp_zoom_factor(200.0), 200.0);
        assert_eq!(clamp_zoom_factor(10.0), 10.0);
        assert_eq!(clamp_zoom_factor(0.5), 1.0);
        assert_eq!(clamp_zoom_factor(f32::NAN), 1.0);
        assert_eq!(clamp_zoom_factor(f32::INFINITY), 1.0);
    }

    /// `set-fps-cap` sanitizes to 1..=240: 0 reads as 1, the deepest
    /// tier, and none stays none.
    #[test]
    fn set_fps_cap_sanitizes_to_1_through_240() {
        assert_eq!(sanitize_fps_cap(None), None);
        assert_eq!(sanitize_fps_cap(Some(0)), Some(1));
        assert_eq!(sanitize_fps_cap(Some(1)), Some(1));
        assert_eq!(sanitize_fps_cap(Some(60)), Some(60));
        assert_eq!(sanitize_fps_cap(Some(240)), Some(240));
        assert_eq!(sanitize_fps_cap(Some(999)), Some(240));
    }

    /// `set-hand-fov`'s base stays inside the projection's range, and
    /// a non-finite base reads as vanilla's 70.
    #[test]
    fn set_hand_fov_sanitizes_the_base() {
        let fov = |base: f32| {
            sanitize_hand_fov(ModHandFov {
                mode: ModHandFovMode::Add,
                base,
            })
        };
        assert_eq!(fov(70.0).base, 70.0);
        assert_eq!(fov(f32::NAN).base, 70.0);
        assert_eq!(fov(f32::INFINITY).base, 70.0);
        assert_eq!(fov(1000.0).base, 175.0);
        assert_eq!(fov(1.0).base, 5.0);
        assert_eq!(fov(70.0).mode, ModHandFovMode::Add);
    }

    /// The hand FOV's algebra, at the invariants the district pins:
    /// settled at factor F, multiply gives base / F and add gives
    /// base − V(1 − 1/F) with V the unzoomed view; at F = 1 every
    /// mode gives the base exactly; keep never moves.
    #[test]
    fn hand_fov_follows_the_zoom_ratio() {
        let setting = |mode: ModHandFovMode, base: f32| ModHandFov { mode, base };
        // Settled at F = 4 with an unzoomed view of 80 (zoomed: 20).
        assert_eq!(
            hand_fov_base(&setting(ModHandFovMode::Multiply, 70.0), 0.25, 20.0),
            17.5
        );
        assert_eq!(
            hand_fov_base(&setting(ModHandFovMode::Add, 70.0), 0.25, 20.0),
            10.0
        );
        assert_eq!(
            hand_fov_base(&setting(ModHandFovMode::Keep, 70.0), 0.25, 20.0),
            70.0
        );
        // No zoom (ratio 1): every mode gives the base exactly.
        for mode in [
            ModHandFovMode::Multiply,
            ModHandFovMode::Add,
            ModHandFovMode::Keep,
        ] {
            assert_eq!(hand_fov_base(&setting(mode, 70.0), 1.0, 80.0), 70.0);
        }
        // An extreme mode result clamps instead of reaching a
        // projection: add at F = 250 over a 110-degree view.
        assert_eq!(
            hand_fov_base(&setting(ModHandFovMode::Add, 70.0), 0.004, 0.44),
            5.0
        );
        // A non-finite result reads as vanilla's 70.
        assert_eq!(
            hand_fov_base(&setting(ModHandFovMode::Multiply, f32::NAN), 0.25, 20.0),
            70.0
        );
    }

    fn camera_override(
        position: Option<WorldPoint>,
        yaw: f32,
        pitch: f32,
        roll: f32,
    ) -> ModCameraOverride {
        ModCameraOverride {
            position,
            yaw,
            pitch,
            roll,
        }
    }

    fn point(x: f64, y: f64, z: f64) -> WorldPoint {
        WorldPoint { x, y, z }
    }

    /// `set-camera` wraps yaw, clamps pitch and roll, and reads a
    /// non-finite look as 0.0 for that field.
    #[test]
    fn set_camera_wraps_and_clamps_the_look() {
        let camera = sanitize_camera(camera_override(None, 270.0, 120.0, 200.0), None).unwrap();
        assert_eq!(camera.yaw, -90.0);
        assert_eq!(camera.pitch, 90.0);
        assert_eq!(camera.roll, 180.0);
        assert!(camera.position.is_none());
        let camera =
            sanitize_camera(camera_override(None, f32::NAN, f32::NAN, f32::NAN), None).unwrap();
        assert_eq!((camera.yaw, camera.pitch, camera.roll), (0.0, 0.0, 0.0));
    }

    /// A non-finite position drops the whole call, and a position past
    /// the cap scales back to the 64-block sphere around the feet.
    #[test]
    fn set_camera_drops_a_non_finite_position_and_caps_the_distance() {
        let feet = Some([0.5, 64.0, 0.5]);
        assert!(
            sanitize_camera(
                camera_override(Some(point(f64::NAN, 64.0, 0.5)), 0.0, 0.0, 0.0),
                feet
            )
            .is_none()
        );
        let camera = sanitize_camera(
            camera_override(Some(point(200.5, 64.0, 0.5)), 0.0, 0.0, 0.0),
            feet,
        )
        .unwrap();
        let p = camera.position.unwrap();
        assert!((p.x - 64.5).abs() < 1e-9 && p.y == 64.0 && p.z == 0.5);
        let camera = sanitize_camera(
            camera_override(Some(point(10.5, 64.0, 0.5)), 0.0, 0.0, 0.0),
            feet,
        )
        .unwrap();
        let p = camera.position.expect("within the cap: kept");
        assert_eq!((p.x, p.y, p.z), (10.5, 64.0, 0.5));
    }

    /// The harness's camera: the player's eye at (0.5, 65.62, 0.5),
    /// looking along +Z at fov 70 on a 427 × 240 GUI.
    fn harness_camera() -> SnapshotCamera {
        SnapshotCamera {
            x: 0.5,
            y: 65.62,
            z: 0.5,
            yaw: 0.0,
            pitch: 0.0,
            roll: 0.0,
            fov: 70.0,
        }
    }

    /// A point dead ahead projects to the screen centre; a point behind
    /// the camera is not visible and has a negative depth; a point 90°
    /// off-axis lands off-screen.
    #[test]
    fn project_puts_ahead_at_the_centre_and_behind_out_of_view() {
        let camera = harness_camera();
        let ahead = project_point(&camera, (427.0, 240.0), point(0.5, 65.62, 10.5));
        assert!(ahead.visible);
        assert!((ahead.x - 213.5).abs() < 0.01 && (ahead.y - 120.0).abs() < 0.01);
        assert!((ahead.depth - 10.0).abs() < 1e-9);
        let behind = project_point(&camera, (427.0, 240.0), point(0.5, 65.62, -9.5));
        assert!(!behind.visible && behind.depth < 0.0);
        let side = project_point(&camera, (427.0, 240.0), point(10.5, 65.62, 0.5));
        assert!(!(-427.0..=854.0).contains(&side.x), "side x={}", side.x);
    }

    /// A point ahead and to the camera's right sits right of centre at
    /// rest; rolling the view 90° clockwise moves it to the bottom
    /// centre, and rolling it the other way moves it to the top centre.
    #[test]
    fn project_rolls_with_the_camera() {
        let gui = (427.0, 240.0);
        let ahead_right = point(-9.5, 65.62, 10.5);
        let level = project_point(&harness_camera(), gui, ahead_right);
        assert!(level.x > 213.5 && (level.y - 120.0).abs() < 0.01);
        let mut rolled = harness_camera();
        rolled.roll = 90.0;
        let clockwise = project_point(&rolled, gui, ahead_right);
        assert!((clockwise.x - 213.5).abs() < 0.01 && clockwise.y > 240.0);
        rolled.roll = -90.0;
        let counter = project_point(&rolled, gui, ahead_right);
        assert!((counter.x - 213.5).abs() < 0.01 && counter.y < 0.0);
    }

    /// Turning the camera 90° turns the projection with it: the point
    /// that was ahead and right is dead ahead again.
    #[test]
    fn project_follows_the_camera_yaw() {
        let mut camera = harness_camera();
        camera.yaw = 90.0;
        let ahead = project_point(&camera, (427.0, 240.0), point(-9.5, 65.62, 0.5));
        assert!(ahead.visible);
        assert!((ahead.x - 213.5).abs() < 0.01 && (ahead.y - 120.0).abs() < 0.01);
    }

    /// An emitter's accumulator over a simulated second (API
    /// 26): at 7 spawns a second, twenty ticks hold seven
    /// whole spawns between them, the fraction carrying from
    /// tick to tick; at the 200 cap, every tick holds ten.
    /// A rate of 0 never spawns, and the accumulator never
    /// holds a whole spawn between ticks.
    #[test]
    fn emitter_accumulator_counts_a_second() {
        let mut accumulator = 0.0;
        let mut total = 0;
        for _ in 0..20 {
            total += advance_emitter(&mut accumulator, 7.0);
            assert!(accumulator < 1.0, "the fraction carries, never a whole");
        }
        assert_eq!(total, 7);
        let mut accumulator = 0.0;
        for _ in 0..20 {
            assert_eq!(advance_emitter(&mut accumulator, 200.0), 10);
        }
        assert_eq!(accumulator, 0.0);
        let mut accumulator = 0.0;
        for _ in 0..20 {
            assert_eq!(advance_emitter(&mut accumulator, 0.0), 0);
        }
    }
}
