//! Capability `input` (docs/mods.md): the use button on items a mod claims,
//! the mod's own keys and the mouse wheel; and `render`'s zoom. With no mod
//! claiming items, keys or the wheel, each hook here returns after one
//! check.
use super::Game;
use winit::{event::MouseButton, keyboard::KeyCode};
#[cfg(feature = "mods")]
use {super::Screen, horizon_mods::HeldItem};

/// A claimed item's use while the button is down.
#[cfg(feature = "mods")]
pub(super) struct ModUse {
    item: HeldItem,
    /// `Game::ticks` when it started.
    since: u64,
}

#[cfg(not(feature = "mods"))]
impl Game<'_> {
    pub(super) fn sandbox_mod_use(&mut self, _button: MouseButton, _pressed: bool) -> bool {
        false
    }
    pub(super) fn mod_key(&mut self, _key: KeyCode, _pressed: bool, _repeat: bool) {}
    pub(super) fn mod_mouse_key(&mut self, _button: MouseButton, _pressed: bool) {}
    pub(super) fn mod_wheel(&mut self, _steps: f32) -> bool {
        false
    }
    pub(super) fn mod_zoom(&self) -> Option<ModZoom> {
        None
    }
    pub(super) fn mod_minimap(&self) -> Option<ModMinimap> {
        None
    }
    pub(super) fn mod_far_terrain(&self) -> Option<ModFarTerrain> {
        None
    }
    pub(super) fn mod_camera(&self) -> Option<(ModCamera, u64)> {
        None
    }
    pub(super) fn mod_camera_detached(&self) -> bool {
        false
    }
    pub(super) fn mod_camera_roll(&self) -> glam::Mat4 {
        glam::Mat4::IDENTITY
    }
    pub(super) fn effective_gamma(&self) -> f32 {
        self.settings.gamma
    }
    pub(super) fn mod_fps_cap(&self) -> Option<u32> {
        None
    }
    pub(super) fn mod_fog_off(&self) -> bool {
        false
    }
}

/// A mod's zoom (`render.set-zoom`), clamped by the runtime.
#[derive(Clone, Copy)]
pub(super) struct ModZoom {
    pub(super) factor: f32,
    pub(super) smooth: bool,
    pub(super) turn: f32,
}

/// A mod's minimap (`render.set-minimap`), clamped by the runtime.
#[derive(Clone, Copy)]
pub(super) struct ModMinimap {
    pub(super) corner: horizon_client::extras::Corner,
    pub(super) size: f32,
    pub(super) blocks_per_pixel: f32,
    pub(super) rotate: bool,
    pub(super) round: bool,
}

/// A mod's far terrain (`render.set-far-terrain`), clamped by the runtime.
#[derive(Clone, Copy)]
pub(super) struct ModFarTerrain {
    pub(super) distance: u32,
    pub(super) reduced: bool,
}

/// A mod's camera override (`render.set-camera`), clamped by the runtime:
/// the position is none for a freelook, some for a freecam.
#[derive(Clone, Copy, PartialEq)]
pub(super) struct ModCamera {
    pub(super) position: Option<[f64; 3]>,
    pub(super) yaw: f32,
    pub(super) pitch: f32,
    pub(super) roll: f32,
}

/// A mod camera override's per-frame state, kept while one is active:
/// the mouse-driven look, the `set-camera` call and override it came
/// from, the previous camera position for the wall-clip ray, and the
/// player's feet and dimension then (a teleport resets the ray).
#[derive(Clone, Copy)]
pub(super) struct ModCameraState {
    pub(super) look: (f32, f32),
    pub(super) seq: u64,
    pub(super) last: ModCamera,
    pub(super) prev: [f64; 3],
    pub(super) prev_player: ([f64; 3], super::Dim),
}

#[cfg(feature = "mods")]
impl Game<'_> {
    /// The main hand's stack as `input` shows it; none when it is empty.
    pub(super) fn mod_held(&self) -> Option<HeldItem> {
        let stack = self.inventory.get(self.selected)?.as_ref()?;
        Some(HeldItem {
            id: stack.item.to_string(),
            count: stack.count,
            slot: self.selected as u32,
        })
    }
    /// The use button while playing: true when a mod claimed the held item,
    /// so the game does nothing else with it (no placing, eating, swing).
    pub(super) fn sandbox_mod_use(&mut self, button: MouseButton, pressed: bool) -> bool {
        if button != MouseButton::Right || !self.mods.mods.claims_any() {
            return false;
        }
        if !pressed {
            return self.stop_mod_use();
        }
        let Some(item) = self.mod_held().filter(|h| self.mods.mods.claims(&h.id)) else {
            return false;
        };
        self.mods.mods.use_start(&item);
        self.mods.using = Some(ModUse {
            item,
            since: self.ticks,
        });
        true
    }
    /// Ends a claimed use: the button came up, the player switched slots,
    /// or a screen opened. False when none was going.
    pub(super) fn stop_mod_use(&mut self) -> bool {
        let Some(u) = self.mods.using.take() else {
            return false;
        };
        let ticks = self.ticks.saturating_sub(u.since).min(u64::from(u32::MAX)) as u32;
        self.mods.mods.use_stop(&u.item, ticks);
        true
    }
    /// Once a tick: a claimed use stops when its item leaves the hand or a
    /// screen opens, as vanilla's `releaseUsingItem`.
    pub(super) fn tick_sandbox_mod_use(&mut self) {
        let Some(u) = &self.mods.using else {
            return;
        };
        let still = self
            .mod_held()
            .is_some_and(|h| h.id == u.item.id && h.slot == u.item.slot);
        if !still || self.screen != Screen::Playing {
            self.stop_mod_use();
        }
    }
    /// A key went down or up: a mod's registered key while playing (no
    /// screen) is its `key-down` / `key-up`. Vanilla's mapping on the same
    /// key still runs.
    pub(super) fn mod_key(&mut self, key: KeyCode, pressed: bool, repeat: bool) {
        if repeat || !self.mods.mods.has_keys() {
            return;
        }
        // Over an item on a container screen, the mods' keys are `browser`
        // events (a recipe viewer's R and U).
        if pressed
            && self.screen == Screen::Inventory
            && self.title.is_none()
            && !self.mod_browser_typing()
            && let Some(name) = horizon_client::keybinds::key_name(key)
            && let Some(stack) = self.mod_browser_pointed()
        {
            let stack = super::mod_panel::browser_stack(stack);
            self.mods.mods.browser_key(name, &stack);
            return;
        }
        if pressed && (self.screen != Screen::Playing || self.title.is_some()) {
            return;
        }
        if let Some(name) = horizon_client::keybinds::key_name(key) {
            self.mods.mods.key(name, pressed);
        }
    }
    /// A mouse button while playing: a mod's key bound to it
    /// (`key.mouse.4`...), as `mod_key`.
    pub(super) fn mod_mouse_key(&mut self, button: MouseButton, pressed: bool) {
        if self.mods.mods.has_keys() {
            let name = horizon_client::keybinds::mouse_name(button);
            self.mods.mods.key(&name, pressed);
        }
    }
    /// Wheel steps while playing: true when a mod holds the wheel, so the
    /// hotbar doesn't move.
    pub(super) fn mod_wheel(&mut self, steps: f32) -> bool {
        self.mods.mods.wheel(steps)
    }
    /// Playing, with nothing over the world: mods' keys and zoom apply.
    fn mod_playing(&self) -> bool {
        self.screen == Screen::Playing
            && self.title.is_none()
            && self.sign_edit.is_none()
            && self.book.is_none()
    }
    /// Once a frame, before the mods run: a screen that opened lets their
    /// held keys up.
    pub(super) fn mod_input_frame(&mut self) {
        let playing = self.mod_playing();
        if self.mods.playing && !playing {
            self.mods.mods.release_keys();
        }
        self.mods.playing = playing;
    }
    /// Whether a mod's `set-zoom` would take effect now (API 19,
    /// `render.can-zoom`): the exact gate [`Game::mod_zoom`] applies —
    /// playing, and not scoping — answered for the snapshot, so a mod
    /// can tell a kept-but-unshown zoom from a live one.
    pub(super) fn mod_can_zoom(&self) -> bool {
        self.mod_playing() && !self.scoping()
    }
    /// The strongest zoom a mod asks for, while playing; the spyglass's
    /// own view wins.
    pub(super) fn mod_zoom(&self) -> Option<ModZoom> {
        if !self.mod_playing() || self.scoping() {
            return None;
        }
        let z = self.mods.mods.zoom()?;
        Some(ModZoom {
            factor: z.factor,
            smooth: z.smooth,
            turn: z.turn,
        })
    }
    /// The camera the first running mod drives (`render.set-camera`),
    /// with the call count of the `set-camera` that set it, while
    /// playing. A spectator's camera wins over it: `camera_view`
    /// ignores the override while one is active.
    pub(super) fn mod_camera(&self) -> Option<(ModCamera, u64)> {
        if !self.mod_playing() {
            return None;
        }
        let (c, seq) = self.mods.mods.camera()?;
        Some((
            ModCamera {
                position: c.position.map(|p| [p.x, p.y, p.z]),
                yaw: c.yaw,
                pitch: c.pitch,
                roll: c.roll,
            },
            seq,
        ))
    }
    /// A freecam is active: the camera is detached from the player, so
    /// the player's own model renders and the hand hides, as in third
    /// person. A freelook keeps the camera at the eye, so it does not.
    pub(super) fn mod_camera_detached(&self) -> bool {
        self.mod_camera().is_some_and(|(c, _)| c.position.is_some())
    }
    /// A mod camera's roll as a pose matrix: a Z rotation in view space,
    /// so a positive roll banks the horizon clockwise as the player sees
    /// it. Identity while no camera is active.
    pub(super) fn mod_camera_roll(&self) -> glam::Mat4 {
        self.mod_camera().map_or(glam::Mat4::IDENTITY, |(c, _)| {
            glam::Mat4::from_rotation_z(-c.roll.to_radians())
        })
    }
    /// The minimap the first mod showing one asks for.
    pub(super) fn mod_minimap(&self) -> Option<ModMinimap> {
        use horizon_client::extras::Corner;
        use horizon_mods::RenderCorner;
        let m = self.mods.mods.minimap()?;
        Some(ModMinimap {
            corner: match m.corner {
                RenderCorner::TopLeft => Corner::TopLeft,
                RenderCorner::TopRight => Corner::TopRight,
                RenderCorner::BottomLeft => Corner::BottomLeft,
                RenderCorner::BottomRight => Corner::BottomRight,
            },
            size: m.size,
            blocks_per_pixel: m.blocks_per_pixel,
            rotate: m.rotate,
            round: m.round,
        })
    }
    /// The far terrain the first mod drawing one asks for.
    pub(super) fn mod_far_terrain(&self) -> Option<ModFarTerrain> {
        let f = self.mods.mods.far_terrain()?;
        Some(ModFarTerrain {
            distance: f.distance,
            reduced: f.reduced,
        })
    }
    /// The brightness the renderer applies: the strongest gamma a running
    /// mod asks for while playing, else the player's own Brightness setting.
    pub(super) fn effective_gamma(&self) -> f32 {
        if !self.mod_playing() {
            return self.settings.gamma;
        }
        self.mods.mods.gamma().unwrap_or(self.settings.gamma)
    }
    /// Whether any running mod turned distance fog off, while playing.
    pub(super) fn mod_fog_off(&self) -> bool {
        self.mod_playing() && self.mods.mods.fog_off()
    }
    /// The lowest frame-rate cap a running mod set (API 19,
    /// `render.set-fps-cap`): `Game::framerate_limit` folds it in
    /// before the battery cap, so it composes everywhere that limit
    /// does, splitscreen's min included.
    pub(super) fn mod_fps_cap(&self) -> Option<u32> {
        self.mods.mods.fps_cap()
    }
}
