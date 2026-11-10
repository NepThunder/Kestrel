//! Write Horizon mods in Rust (`docs/mods.md`).
//!
//! A mod is a type implementing [`Mod`], exported with [`export_mod!`] from
//! a `cdylib` crate built for `wasm32-unknown-unknown`;
//! `cargo horizon-mod build` turns it into the component the game loads.
//!
//! ```ignore
//! use horizon_mod_sdk::{Mod, Registration, chat};
//!
//! struct Hello;
//! impl Mod for Hello {
//!     fn init() -> Result<(Self, Registration), String> {
//!         Ok((Hello, Registration::new().command("hello")))
//!     }
//!     fn command(&mut self, _name: &str, _args: &str) -> Result<Option<String>, String> {
//!         chat::show("Hello from a mod!");
//!         Ok(None)
//!     }
//! }
//! horizon_mod_sdk::export_mod!(Hello);
//! ```
//!
//! Every interface but `log` is a capability: list the ones the mod uses in
//! `horizon-mod.toml` (`game-read`, `chat`, `hud`, `storage`, `command`, `ui`,
//! `world` — read-only for a client mod, singleplayer in v1; `inventory` —
//! read-only, the player's own inventory and the open container; `entity` —
//! read-only, the living mobs around the player and the entity under the
//! crosshair; `render` — its HUD shapes and world lines and boxes
//! (depth-tested, API level 11), and the first-person hand's pose
//! through [`hand`] (API level 16; the swing's own terms — which
//! animation plays and how much of the vanilla swing applies —
//! are API level 17); a server mod `storage` and `players`).
//! `game-read` also reads the player themselves through [`game`]:
//! their active effects and self-state (API level 18).
//! `render` also reads whether a zoom would take effect and the
//! spyglass's own zoom, hides the HUD, makes the hand's field of
//! view follow the zoom and caps the frame rate, and `input` reads
//! the time since the player's last input (API level 19).
//! `render` also poses player models through [`entity_pose`]:
//! every drawn player's pose inputs each frame, per-part offsets
//! folded into the finished vanilla pose, a pose scale, and the
//! player's own body in first person (API level 20).
//! `actions` (its own capability, API level 21) makes the player's
//! own clicks for the mod: [`actions::target`] reads what the
//! crosshair rests on, and `use`, `use_on_block`, `attack`,
//! `set_held` and `select_slot` perform them as the player,
//! through the same path a real click takes.
//! `screen` (its own capability, API level 22, experimental)
//! sees the game's own screens: [`screen::state`] and
//! [`screen::trades`] read which screen is open and what it
//! holds — the open menu's type, title, panel and hovered
//! slot, the debug overlay's state, the merchant's offers at
//! their charged counts — and `set_buttons`, `set_highlights`,
//! `claim_debug` and `select_trade` put the mod's part on
//! those screens: buttons the game places and the player
//! presses (heard through [`screen::poll`]), slot marks, the
//! debug surface's tenancy, and the merchant's trade choice,
//! performed by the game as clicking the offer would. No stack
//! moves here; that line is a later district's.
//! `render` also shows items themselves through [`item_model`]
//! (API level 23): which item model definition a stack
//! displays as, keyed by its item and components
//! (`set_model_rules`); marks on the game's item slots, the
//! hotbar included (`set_decorations`); and how dropped items
//! lie in the world (`set_dropped_display`). A rule names a
//! definition that already exists — the game's, a resource
//! pack's, a content mod's — never a model the mod supplies.
//! `inventory_actions` (its own capability, API level 24,
//! experimental) makes the player's own slot clicks for the
//! mod: [`inventory_actions::click`] and
//! [`inventory_actions::drag`] queue clicks and a drag over
//! the slots of the inventory and the open container, for the
//! game to perform after the hook returns through its own
//! menu logic — the server sees ordinary clicks and may
//! refuse any of them. [`inventory_actions::poll_wheel`]
//! reads the wheel turns over the open screen, observed never
//! consumed. Reading an inventory is `inventory`'s consent;
//! moving its stacks is this one's.
//! `map` (its own capability, API level 25, experimental)
//! reads the game's own mapping of the ground around the
//! player: [`map::tiles`] and [`map::tile_versions`] read its
//! minimap's tiles verbatim — a view of the loaded chunks,
//! never an archive, so a mod keeps the tiles it wants in
//! its own storage, keyed by [`map::context`].
//! [`map::last_death`] reads where the player last died, and
//! [`map::poll_events`] the deaths and respawns as they
//! happen. The mod composes the pictures itself:
//! [`map::set_map_screen`] shows a full-screen map the game
//! pans, zooms and fields clicks over (heard through
//! [`map::poll_map`]), and [`map::set_minimap_image`] a
//! corner minimap of its own shading.
//! `render` also spawns particles through [`particles`]
//! (API level 26): [`particles::emit`] draws one burst in
//! vanilla's own shape — a type name, its parameters, a
//! centre, a spread, a speed and a count — and
//! [`particles::set_emitters`] keeps standing streams at
//! anchors of the player's or an entity's facing frame,
//! stepped by the game every tick. Everything is drawn by
//! the game's own particle engine on this client: nothing
//! reaches the server and nothing in the world changes.
//! `chat_decoration` (its own capability, API level 27,
//! experimental) restyles the game's own chat:
//! [`chat_decoration::set_line_decoration`] puts a player's
//! own head — the skin's face, as the player list draws it —
//! beside the lines that player wrote, resolved and drawn by
//! the game (the mod never names a player, a line or a
//! picture), and [`chat_decoration::set_animation`] declares
//! how the chat box arrives and opens: a new message's slide
//! and fade, the input field's rise, and the trust
//! indicator's bar hidden. Nothing is read and nothing is
//! drawn by the mod: it declares a policy, the game applies
//! it, and stopping the mod restores vanilla chat.
//! Server mods: [`server`].
//!
//! Widgets (capability `ui`): a mod shows a screen it owns with
//! [`ui::open_screen`] and a page in Options > Mods with
//! [`ui::set_settings`]; what the player does with them comes back as
//! [`Mod::ui_select`], [`Mod::ui_closed`] and [`Mod::ui_settings_opened`].

wit_bindgen::generate!({
    path: "wit",
    world: "horizon-ui-mod",
    pub_export_macro: true,
    export_macro_name: "__export_horizon_mod",
    default_bindings_module: "horizon_mod_sdk",
});

pub use exports::horizon::mods::audio_events::{
    Guest as __AudioEventsGuest, SoundAdjustment, SoundQuery,
};
pub use exports::horizon::mods::events::{Guest as __Guest, Registration};
pub use exports::horizon::mods::ui_events::Guest as __UiEventsGuest;
pub use horizon::mods::actions::{
    BlockTarget, CrosshairTarget as ActionTarget, EntityTarget, Face, Hand as ActionHand,
};
pub use horizon::mods::chat_decoration::{ChatAnimation, LineDecoration};
pub use horizon::mods::entity_pose::{
    ArmPose, EntityPoseState, ItemUse, PartOffset, PoseOffset, PoseTarget,
};
pub use horizon::mods::game::{Effect, Mount, SelfState};
pub use horizon::mods::hand::{HandState, HandTransform, SwingAnim, SwingKind};
pub use horizon::mods::inventory_actions::{ClickKind, DragButton, WheelEvent};
pub use horizon::mods::item_model::{
    BlockDisplay, ComponentMatch, Decoration, DecorationPlace, DecorationTarget, DisplayContext,
    DroppedDisplay, FlatDisplay, ModelMatch, ModelRule,
};
pub use horizon::mods::map::{
    ChunkPos, Corner, MapClick, MapContext, MapEvent, MapImage, MapLayer, MapMarker, MapScreen,
    MapTile, MapView, MinimapImage, Position, PositionEvent, TileVersion,
};
pub use horizon::mods::particles::{Burst, Emitter, EmitterAnchor, ParticleParams, WorldPoint};
pub use horizon::mods::render::{HandFov, HandFovMode};
pub use horizon::mods::screen::{
    Anchor, Highlight, ScreenButton, ScreenEvent, ScreenState, TradeList, TradeOffer, TradeStack,
};
pub use horizon::mods::{
    actions, audio, browser, chat, chat_decoration, command, entity, entity_pose, game, hand, hud,
    input, inventory, inventory_actions, item_model, log, map, particles, recipes, render, screen,
    storage, ui, world,
};

/// The game's calls into a mod. Only the hooks [`Registration`] asks for
/// are called; the others may stay as their defaults.
pub trait Mod: Sized + 'static {
    /// Once, after loading: the mod's state and the hooks it wants. An error
    /// keeps the mod off and is shown to the player.
    fn init() -> Result<(Self, Registration), String>;
    /// Every game tick, when [`Registration::tick`] is set.
    fn tick(&mut self, _tick: u64) {}
    /// Every rendered frame, when [`Registration::frame`] is set.
    fn frame(&mut self, _seconds: f32) {}
    /// `/name args` for a command the mod registered. `Ok(Some(text))` is
    /// shown as feedback, `Err(text)` in red.
    fn command(&mut self, _name: &str, _args: &str) -> Result<Option<String>, String> {
        Err("This command does nothing".into())
    }
    /// A chat line the player received, when [`Registration::chat`] is set.
    fn chat(&mut self, _text: &str) {}
    /// A world sound is starting (capability `audio`, API 15): return
    /// how it should be quietened, muffled and sent to the reverb.
    /// Only the first mod in load order that implements this is ever
    /// asked. The default is no change.
    fn sound(&mut self, _query: &SoundQuery) -> SoundAdjustment {
        SoundAdjustment {
            gain: 1.0,
            lowpass: 0.0,
            reverb: 0.0,
        }
    }
    /// A row of one of the mod's `list` widgets was picked (capability
    /// `ui`): the widget's `id` and the row.
    fn ui_select(&mut self, _id: u32, _row: u32) {}
    /// The player left the mod's screen (Esc or Done; capability `ui`).
    fn ui_closed(&mut self) {}
    /// The player opened the mod's settings page (capability `ui`): call
    /// [`ui::set_settings`] for fresh widgets.
    fn ui_settings_opened(&mut self) {}
}

impl Registration {
    /// No commands and no hooks.
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
            tick: false,
            frame: false,
            chat: false,
        }
    }
    /// Adds `/name`.
    pub fn command(mut self, name: &str) -> Self {
        self.commands.push(name.into());
        self
    }
    pub fn on_tick(mut self) -> Self {
        self.tick = true;
        self
    }
    pub fn on_frame(mut self) -> Self {
        self.frame = true;
        self
    }
    pub fn on_chat(mut self) -> Self {
        self.chat = true;
        self
    }
}
impl Default for Registration {
    fn default() -> Self {
        Self::new()
    }
}

/// `log::write` at info level.
pub fn info(message: &str) {
    log::write(log::Level::Info, message);
}
/// `log::write` at warn level.
pub fn warn(message: &str) {
    log::write(log::Level::Warn, message);
}

/// Exports a [`Mod`] as the component's `events` interface. The state lives
/// in a thread local: a mod instance is single-threaded.
#[macro_export]
macro_rules! export_mod {
    ($t:ty) => {
        const _: () = {
            use ::std::cell::RefCell;
            ::std::thread_local! {
                static STATE: RefCell<Option<$t>> = const { RefCell::new(None) };
            }
            fn with(f: impl FnOnce(&mut $t)) {
                STATE.with(|s| {
                    if let Some(m) = s.borrow_mut().as_mut() {
                        f(m)
                    }
                })
            }
            struct Export;
            impl $crate::__Guest for Export {
                fn init() -> Result<$crate::Registration, String> {
                    let (m, registration) = <$t as $crate::Mod>::init()?;
                    STATE.with(|s| *s.borrow_mut() = Some(m));
                    Ok(registration)
                }
                fn tick(tick: u64) {
                    with(|m| $crate::Mod::tick(m, tick))
                }
                fn frame(seconds: f32) {
                    with(|m| $crate::Mod::frame(m, seconds))
                }
                fn command(name: String, args: String) -> Result<Option<String>, String> {
                    let mut out = Err("The mod is not running".to_string());
                    with(|m| out = $crate::Mod::command(m, &name, &args));
                    out
                }
                fn chat(text: String) {
                    with(|m| $crate::Mod::chat(m, &text))
                }
            }
            impl $crate::__AudioEventsGuest for Export {
                fn sound(query: $crate::SoundQuery) -> $crate::SoundAdjustment {
                    let mut out = $crate::SoundAdjustment {
                        gain: 1.0,
                        lowpass: 0.0,
                        reverb: 0.0,
                    };
                    with(|m| out = $crate::Mod::sound(m, &query));
                    out
                }
            }
            impl $crate::__UiEventsGuest for Export {
                fn select(id: u32, row: u32) {
                    with(|m| $crate::Mod::ui_select(m, id, row))
                }
                fn closed() {
                    with(|m| $crate::Mod::ui_closed(m))
                }
                fn settings_opened() {
                    with(|m| $crate::Mod::ui_settings_opened(m))
                }
            }
            // Only WebAssembly has the component's exports: a host build (cargo test or
            // clippy on the workspace) checks the mod without them, which macOS
            // couldn't link (its exported-symbols list cuts names at the `#`).
            #[cfg(target_arch = "wasm32")]
            $crate::__export_horizon_mod!(Export);
            #[cfg(not(target_arch = "wasm32"))]
            let _ = (
                <Export as $crate::__Guest>::init,
                <Export as $crate::__UiEventsGuest>::select,
                <Export as $crate::__AudioEventsGuest>::sound,
            );
        };
    };
}

/// Server mods (manifest `side = "server"`, `api = "0.3"`, experimental): a
/// type implementing [`ServerMod`](server::ServerMod), exported with
/// [`export_server_mod!`]. It runs where the world is simulated, and reads
/// and messages the players with capability `players`.
pub mod server {
    wit_bindgen::generate!({
        path: "wit",
        world: "horizon-server-mod",
        pub_export_macro: true,
        export_macro_name: "__export_horizon_server_mod",
        default_bindings_module: "horizon_mod_sdk::server",
        with: {
            "horizon:mods/log@0.1.0": crate::log,
            "horizon:mods/storage@0.1.0": crate::storage,
        },
    });

    pub use exports::horizon::mods::server_events::{
        Argument, ArgumentKind, ArgumentValue, BlockPos, Guest as __Guest, Registration,
        ServerCommand,
    };
    pub use horizon::mods::players::{self, GameMode, Player};
    pub use horizon::mods::world::{self, BlockChange};

    /// The server's calls into a mod. Only the hooks [`Registration`] asks
    /// for are called; the others may stay as their defaults.
    pub trait ServerMod: Sized + 'static {
        /// Once, after loading: the mod's state and the hooks it wants. An
        /// error keeps the mod off and is logged.
        fn init() -> Result<(Self, Registration), String>;
        /// Every server tick, when [`Registration::tick`] is set.
        fn tick(&mut self, _tick: u64) {}
        /// When [`Registration::joins`] is set.
        fn player_joined(&mut self, _player: &Player) {}
        fn player_left(&mut self, _player: &Player) {}
        /// A chat message a player sent, when [`Registration::chat`] is set.
        fn chat(&mut self, _player: &Player, _text: &str) {}
        /// `/name args` typed by `player`, parsed as the command's
        /// arguments say ([`Registration::command_with`]). `Ok(Some(text))`
        /// is their feedback, `Err(text)` their error in red.
        fn command(
            &mut self,
            _player: &Player,
            _name: &str,
            _args: &Args,
        ) -> Result<Option<String>, String> {
            Err("This command does nothing".into())
        }
        /// When [`Registration::blocks`] is set: `state` is what was there.
        fn block_broken(&mut self, _player: &Player, _pos: BlockPos, _state: &str) {}
        fn block_placed(&mut self, _player: &Player, _pos: BlockPos, _state: &str) {}
    }

    impl Registration {
        /// No commands and no hooks.
        pub fn new() -> Self {
            Self {
                commands: Vec::new(),
                tick: false,
                joins: false,
                chat: false,
                blocks: false,
            }
        }
        /// Adds `/name`, taking free text ([`Args::text`]): one optional
        /// `text` argument named `args`.
        pub fn command(self, name: &str) -> Self {
            self.command_with(name, [Argument::text("args").optional()])
        }
        /// Adds `/name` with typed arguments (none: nothing after the name):
        /// players' chat boxes complete and check them, and the server
        /// parses them into [`Args`].
        pub fn command_with(
            mut self,
            name: &str,
            arguments: impl IntoIterator<Item = Argument>,
        ) -> Self {
            self.commands.push(ServerCommand {
                name: name.into(),
                arguments: arguments.into_iter().collect(),
            });
            self
        }
        pub fn on_tick(mut self) -> Self {
            self.tick = true;
            self
        }
        pub fn on_joins(mut self) -> Self {
            self.joins = true;
            self
        }
        pub fn on_chat(mut self) -> Self {
            self.chat = true;
            self
        }
        pub fn on_blocks(mut self) -> Self {
            self.blocks = true;
            self
        }
    }
    impl Default for Registration {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Argument {
        fn of(name: &str, kind: ArgumentKind) -> Self {
            Self {
                name: name.into(),
                kind,
                optional: false,
            }
        }
        /// One word: letters, digits and `_ - . +`.
        pub fn word(name: &str) -> Self {
            Self::of(name, ArgumentKind::Word)
        }
        /// The rest of the line: only last.
        pub fn text(name: &str) -> Self {
            Self::of(name, ArgumentKind::Text)
        }
        /// A whole number, at least `min` and at most `max` when given.
        pub fn integer(name: &str, min: Option<i32>, max: Option<i32>) -> Self {
            Self::of(name, ArgumentKind::Integer((min, max)))
        }
        pub fn number(name: &str) -> Self {
            Self::of(name, ArgumentKind::Number)
        }
        pub fn boolean(name: &str) -> Self {
            Self::of(name, ArgumentKind::Boolean)
        }
        /// One online player: a name, `@s` or `@p`.
        pub fn player(name: &str) -> Self {
            Self::of(name, ArgumentKind::Player)
        }
        /// `x y z`, each one also `~` or `~n`.
        pub fn block_pos(name: &str) -> Self {
            Self::of(name, ArgumentKind::BlockPos)
        }
        /// May be left out; optional arguments come after the required ones.
        pub fn optional(mut self) -> Self {
            self.optional = true;
            self
        }
    }

    /// A command's arguments: the line as typed, and the values the server
    /// parsed from it, by argument name.
    #[derive(Clone, Debug, Default)]
    pub struct Args {
        text: String,
        values: Vec<(String, ArgumentValue)>,
    }
    impl Args {
        pub fn new(text: String, values: Vec<(String, ArgumentValue)>) -> Self {
            Self { text, values }
        }
        /// The line after the command's name, as typed.
        pub fn text(&self) -> &str {
            &self.text
        }
        /// The value of argument `name`; `None` when it was left out.
        pub fn get(&self, name: &str) -> Option<&ArgumentValue> {
            self.values.iter().find(|(n, _)| n == name).map(|(_, v)| v)
        }
        pub fn integer(&self, name: &str) -> Option<i32> {
            match self.get(name)? {
                ArgumentValue::Integer(n) => Some(*n),
                _ => None,
            }
        }
        pub fn number(&self, name: &str) -> Option<f64> {
            match self.get(name)? {
                ArgumentValue::Number(n) => Some(*n),
                _ => None,
            }
        }
        /// A `word` or `text` argument.
        pub fn string(&self, name: &str) -> Option<&str> {
            match self.get(name)? {
                ArgumentValue::Word(s) | ArgumentValue::Text(s) => Some(s),
                _ => None,
            }
        }
        pub fn boolean(&self, name: &str) -> Option<bool> {
            match self.get(name)? {
                ArgumentValue::Boolean(b) => Some(*b),
                _ => None,
            }
        }
        pub fn player(&self, name: &str) -> Option<&Player> {
            match self.get(name)? {
                ArgumentValue::Player(p) => Some(p),
                _ => None,
            }
        }
        pub fn block_pos(&self, name: &str) -> Option<BlockPos> {
            match self.get(name)? {
                ArgumentValue::BlockPos(p) => Some(*p),
                _ => None,
            }
        }
    }
}

/// Exports a [`ServerMod`](server::ServerMod) as the component's
/// `server-events` interface, as [`export_mod!`] does for a client mod.
#[macro_export]
macro_rules! export_server_mod {
    ($t:ty) => {
        const _: () = {
            use ::std::cell::RefCell;
            use $crate::server::{Args, ArgumentValue, BlockPos, Player, Registration, ServerMod};
            ::std::thread_local! {
                static STATE: RefCell<Option<$t>> = const { RefCell::new(None) };
            }
            fn with(f: impl FnOnce(&mut $t)) {
                STATE.with(|s| {
                    if let Some(m) = s.borrow_mut().as_mut() {
                        f(m)
                    }
                })
            }
            struct Export;
            impl $crate::server::__Guest for Export {
                fn init() -> Result<Registration, String> {
                    let (m, registration) = <$t as ServerMod>::init()?;
                    STATE.with(|s| *s.borrow_mut() = Some(m));
                    Ok(registration)
                }
                fn tick(tick: u64) {
                    with(|m| m.tick(tick))
                }
                fn player_joined(player: Player) {
                    with(|m| m.player_joined(&player))
                }
                fn player_left(player: Player) {
                    with(|m| m.player_left(&player))
                }
                fn chat(player: Player, text: String) {
                    with(|m| m.chat(&player, &text))
                }
                fn command(
                    player: Player,
                    name: String,
                    args: String,
                    values: Vec<(String, ArgumentValue)>,
                ) -> Result<Option<String>, String> {
                    let mut out = Err("The mod is not running".to_string());
                    let args = Args::new(args, values);
                    with(|m| out = m.command(&player, &name, &args));
                    out
                }
                fn block_broken(player: Player, pos: BlockPos, state: String) {
                    with(|m| m.block_broken(&player, pos, &state))
                }
                fn block_placed(player: Player, pos: BlockPos, state: String) {
                    with(|m| m.block_placed(&player, pos, &state))
                }
            }
            #[cfg(target_arch = "wasm32")]
            $crate::server::__export_horizon_server_mod!(Export);
            #[cfg(not(target_arch = "wasm32"))]
            let _ = <Export as $crate::server::__Guest>::init;
        };
    };
}
