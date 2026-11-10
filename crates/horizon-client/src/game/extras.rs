//! Horizon's extras in the game (`docs/extras.md`); a mod's minimap
//! (`render.set-minimap`: its tiles, image and HUD) and zoom (`render.set-zoom`) on the camera; clicks
//! and frame times for mods (`input.clicks-per-second`, `render.frame-stats`).
//! Each hook does nothing while its extra is off and no mod runs.
use super::{Dim, Game, Screen, native_hooks::NativeMinimap};
use horizon_client::{
    extras::{Corner, Cps, FpsCounter, fov_target},
    extras_hud::{self, Corners},
    hud::{HudBatch, MINIMAP_SPRITE, MOD_MINIMAP_SPRITE},
    minimap::{self, Layer, Minimap, View},
    multiplayer,
    reconnect::{Retry, Tracker},
    title::Mode,
};
use std::time::Duration;
use winit::event::MouseButton;

#[derive(Default)]
pub(super) struct ExtrasState {
    fps: FpsCounter,
    /// Left and right mouse buttons: held, and their clicks.
    mouse: [bool; 2],
    cps: [Cps; 2],
    minimap: Minimap,
    /// The dimension the minimap's tiles belong to.
    minimap_dim: Option<Dim>,
    /// Tile versions beside the minimap's cache (capability
    /// `map`, API 25): a chunk's version starts at 1 with its
    /// first tile, grows when a rescan changes the tile's
    /// content, and dies with the tile — the table is cleared
    /// and pruned where the cache is.
    tile_versions: std::collections::HashMap<(i32, i32), u64>,
    /// The mod minimap image last uploaded to its atlas slot
    /// (API 25): the set's stamp and the uploaded size in
    /// pixels (the image resampled to fit the slot).
    pub(super) minimap_image_shown: Option<(u64, u32, u32)>,
    /// Auto-reconnect's attempts at the current server.
    reconnect: Tracker,
}
/// CPU time the minimap's tiles may take per frame.
const MINIMAP_BUDGET: Duration = Duration::from_micros(300);

impl Game<'_> {
    fn extras_now(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }
    /// Mouse buttons while playing, for mods' clicks.
    pub(super) fn extras_mouse(&mut self, button: MouseButton, pressed: bool) {
        self.native_mouse_event(button, pressed);
        let i = match button {
            MouseButton::Left => 0,
            MouseButton::Right => 1,
            _ => return,
        };
        self.extras.mouse[i] = pressed;
        // For mods (`input.clicks-per-second`, `native_clicks`).
        if pressed {
            let now = self.extras_now();
            self.extras.cps[i].click(now);
        }
    }
    /// Once per frame: the frame's time, and keys let go behind screens.
    pub(super) fn extras_frame(&mut self, elapsed: Duration) {
        // Under agent control the window may lack focus without pausing.
        let focused = self.focused || self.control.is_some();
        let playing = self.screen == Screen::Playing && self.title.is_none() && focused;
        if !playing {
            self.extras.mouse = [false; 2];
        }
        self.update_minimap();
        // For mods (`render.frame-stats`, `native_frame_rate`).
        let now = self.extras_now();
        self.extras.fps.record(now, elapsed.as_secs_f32() * 1000.0);
        self.native_frame(elapsed);
    }
    /// The frame counter for mods: fps, min, 1% low.
    pub(super) fn frame_reading(&self) -> (u32, u32, u32) {
        let r = self.extras.fps.reading();
        (r.fps, r.min, r.low)
    }
    /// Attack and use clicks in the last second, and whether each is held.
    pub(super) fn mouse_reading(&mut self) -> ((u32, u32), [bool; 2]) {
        let now = self.extras_now();
        let [l, r] = &mut self.extras.cps;
        let cps = (l.per_second(now) as u32, r.per_second(now) as u32);
        (cps, self.extras.mouse)
    }
    /// Auto-reconnect: a connection to `address` as `name` starts.
    pub(super) fn extras_connecting(&mut self, address: &str, name: &str, lan: bool) {
        self.extras.reconnect.connecting(address, name, lan);
    }
    /// The server this game connected to last.
    pub(super) fn server_address(&self) -> Option<&str> {
        self.extras.reconnect.address()
    }
    /// Auto-reconnect: the server let the player in.
    pub(super) fn extras_joined(&mut self) {
        self.extras.reconnect.joined(std::time::Instant::now());
    }
    /// Auto-reconnect: the player left the server on purpose.
    pub(super) fn extras_left(&mut self) {
        self.extras.reconnect.reset();
    }
    /// Auto-reconnect's next attempt after a disconnect, when it is on and
    /// the reason allows one.
    pub(super) fn reconnect_after(
        &mut self,
        reason: &str,
        key: Option<&str>,
        from_server: bool,
    ) -> Option<Retry> {
        if !self.settings.extras.auto_reconnect {
            self.extras.reconnect.reset();
            return None;
        }
        let now = std::time::Instant::now();
        self.extras.reconnect.ended(reason, key, from_server, now)
    }
    /// Show the attempt's countdown on the Disconnected screen.
    pub(super) fn offer_reconnect(&mut self, retry: Option<Retry>) {
        let Some(retry) = retry else {
            return;
        };
        if let Some(title) = self.title.as_mut()
            && let Mode::Multiplayer(multiplayer::Screen::Disconnected { retry: slot, .. }) =
                &mut title.mode
        {
            log::info!(
                "Horizon: reconnecting to {} in {} s",
                horizon_client::net::shown_address(&retry.address),
                retry.seconds_left(std::time::Instant::now())
            );
            *slot = Some(retry);
        }
    }
    /// Auto-reconnect: connect again once the countdown is over. An
    /// online-mode server gets the signed-in account again: `connect_server`
    /// takes it from `auth::current()`, held in memory for the whole run
    /// (the launcher's session file is gone after the first read).
    pub(super) fn extras_retry(&mut self) -> horizon_client::Result<()> {
        if let Some(retry) = self.title.as_mut().and_then(|t| t.due_retry()) {
            self.connect_server_with_options(&retry.address, &retry.name, None, retry.lan)?;
        }
        Ok(())
    }
    /// The minimap a mod shows (`render.set-minimap`): its corner, its side
    /// in GUI pixels and its view of the player.
    fn minimap_shown(&self) -> Option<(Corner, f32, View)> {
        let p = self.player.as_ref()?;
        if self.title.is_some() || self.hud_hidden() {
            return None;
        }
        let m = self.minimap_wanted()?;
        let view = View {
            x: p.position.x,
            z: p.position.z,
            yaw: self.yaw,
            rotate: m.rotate,
            blocks_per_pixel: m.blocks_per_pixel,
            round: m.round,
        };
        Some((m.corner, m.size, view))
    }
    /// The minimap shown: a native mod's (`native_minimap`), else a
    /// WebAssembly mod's.
    fn minimap_wanted(&self) -> Option<NativeMinimap> {
        if let Some(m) = self.native_minimap_now() {
            return Some(m);
        }
        let m = self.mod_minimap()?;
        Some(NativeMinimap {
            corner: m.corner,
            size: m.size,
            blocks_per_pixel: m.blocks_per_pixel,
            rotate: m.rotate,
            round: m.round,
        })
    }
    /// A WASM mod's own minimap image (capability `map`, API
    /// 25), when it stands in for the terrain minimap: no
    /// native mod's minimap shows, and the same gates as the
    /// terrain one's (a player, no title screen, the HUD up).
    /// A WASM mod's `render.set-minimap` terrain ask still
    /// warms the cache beside it, but the picture is what
    /// draws.
    fn minimap_picture(&self) -> Option<super::mod_map::ModMinimapPicture> {
        if self.native_minimap_now().is_some()
            || self.player.is_none()
            || self.title.is_some()
            || self.hud_hidden()
        {
            return None;
        }
        self.mod_minimap_image()
    }
    /// Scanned chunks' versions, from the updates the minimap
    /// just made (capability `map`, API 25): a chunk's version
    /// starts at 1 with its first tile, grows when a rescan
    /// changes the tile's content, and dies with the tile —
    /// the table is pruned where the cache is.
    fn bump_tile_versions(&mut self) {
        for key in self.extras.minimap.take_changed_tiles() {
            *self.extras.tile_versions.entry(key).or_insert(0) += 1;
        }
        self.extras
            .tile_versions
            .retain(|key, _| self.extras.minimap.has_tile(*key));
    }
    /// Keep the minimap's cache (and the tile versions) warm
    /// for a tile reader alone (capability `map`, API 25),
    /// with no map on screen: the same layer rule as the shown
    /// minimap's, the render distance as the radius, nothing
    /// composed or uploaded.
    fn warm_map_tiles(&mut self) {
        if self.extras.minimap_dim != Some(self.dim) {
            self.extras.minimap.clear();
            self.extras.tile_versions.clear();
            self.extras.minimap_dim = Some(self.dim);
        }
        let Some(p) = self.player.as_ref() else {
            return;
        };
        let head = (p.position.y + f64::from(p.dimensions.eye_height)).floor() as i32;
        let (bx, bz) = (p.position.x.floor() as i32, p.position.z.floor() as i32);
        let covered = self.world.light(bx, head, bz) >> 4 == 0;
        let layer = if self.dim == Dim::Nether || covered {
            Layer::Below(head)
        } else {
            Layer::Surface
        };
        let radius = self.settings.render_distance.max(2);
        self.extras.minimap.update(
            &self.world,
            self.res.facts(),
            (bx >> 4, bz >> 4),
            radius,
            layer,
            MINIMAP_BUDGET,
        );
        self.bump_tile_versions();
    }
    /// The cache's tiles as capability `map` reads them (API
    /// 25), for the view the game lends: every versioned chunk
    /// (the version table's keys are the cache's — bumps add
    /// them and the prune keeps them level) with its version.
    /// A copy, as the lent world view's columns are.
    #[cfg(feature = "mods")]
    pub(super) fn map_tile_data(
        &self,
    ) -> std::collections::HashMap<(i32, i32), horizon_mods::MapTileData> {
        let mut out = std::collections::HashMap::new();
        for (key, version) in &self.extras.tile_versions {
            let Some(tile) = self.extras.minimap.tile(*key) else {
                continue;
            };
            let (layer, cut_y) = match tile.layer {
                Layer::Surface => (horizon_mods::ModMapLayer::Surface, None),
                Layer::Below(head) => (horizon_mods::ModMapLayer::Cave, Some(head)),
            };
            out.insert(
                *key,
                horizon_mods::MapTileData {
                    layer,
                    cut_y,
                    version: *version,
                    colors: tile.color,
                    heights: tile.height,
                    depths: tile.depth,
                },
            );
        }
        out
    }
    /// Refresh the minimap's tiles within the frame budget, recompose its
    /// image when the view or a tile changed, and queue the upload.
    fn update_minimap(&mut self) {
        let picture = self.minimap_picture();
        let Some((_, _, view)) = self.minimap_shown() else {
            if picture.is_some() {
                // A mod's own image shows with no terrain ask:
                // the cache warms as for a tile reader, and the
                // picture uploads.
                self.warm_map_tiles();
                if let Some(picture) = &picture {
                    self.upload_minimap_picture(picture);
                }
            } else if self.player.is_none() {
                self.extras.minimap.clear();
                self.extras.tile_versions.clear();
                self.extras.minimap_dim = None;
            } else if self.minimap_wanted().is_none() && self.mod_wants_map_tiles() {
                // No map on screen, but a mod reads tiles
                // (capability `map`, API 25): the cache stays
                // warm for it — the engine change the district
                // asked for.
                self.warm_map_tiles();
            } else if self.minimap_wanted().is_none() {
                self.extras.minimap.clear();
                self.extras.tile_versions.clear();
                self.extras.minimap_dim = None;
            }
            // A wanted minimap kept off screen by the title or
            // a hidden HUD keeps its cache, as before.
            return;
        };
        if self.extras.minimap_dim != Some(self.dim) {
            self.extras.minimap.clear();
            self.extras.tile_versions.clear();
            self.extras.minimap_dim = Some(self.dim);
        }
        let Some(p) = self.player.as_ref() else {
            return;
        };
        let head = (p.position.y + f64::from(p.dimensions.eye_height)).floor() as i32;
        let (bx, bz) = (view.x.floor() as i32, view.z.floor() as i32);
        // Underground, under a roof or in the Nether: the floor around the player.
        let covered = self.world.light(bx, head, bz) >> 4 == 0;
        let layer = if self.dim == Dim::Nether || covered {
            Layer::Below(head)
        } else {
            Layer::Surface
        };
        // The chunks the image can reach (its corners when it turns).
        let reach = minimap::SIZE as f32 / 2.0 * view.blocks_per_pixel * std::f32::consts::SQRT_2;
        let radius = ((reach / 16.0).ceil() as i32 + 1).min(self.settings.render_distance.max(2));
        self.extras.minimap.update(
            &self.world,
            self.res.facts(),
            (bx >> 4, bz >> 4),
            radius,
            layer,
            MINIMAP_BUDGET,
        );
        self.bump_tile_versions();
        if let Some(picture) = &picture {
            // A mod's picture stands in for the terrain image:
            // uploaded instead of composed.
            self.upload_minimap_picture(picture);
            return;
        }
        if !self.extras.minimap.compose(view) {
            return;
        }
        let Some(rect) = self.gui.atlas.sprites.get(MINIMAP_SPRITE).copied() else {
            return;
        };
        let (w, h) = (self.gui.atlas.width as f32, self.gui.atlas.height as f32);
        self.map_patches.push(horizon_client::font::Patch {
            x: (rect.u0 * w).round() as u32,
            y: (rect.v0 * h).round() as u32,
            width: minimap::SIZE as u32,
            height: minimap::SIZE as u32,
            rgba: self.extras.minimap.image.clone(),
        });
    }
    /// A mod's zoom in effect (`render.set-zoom`): (factor, smooth, turning
    /// scale); none is 1.
    fn zoom_now(&self) -> (f32, bool, f64) {
        if let Some(z) = self.native_zoom_now() {
            return (z.factor, z.smooth, f64::from(z.turn));
        }
        self.mod_zoom().map_or((1.0, true, 1.0), |z| {
            (z.factor, z.smooth, f64::from(z.turn))
        })
    }
    /// Ease the tracked zoom ratio (API 19, `render.set-hand-fov`):
    /// toward 1/factor with the same halfway lerp `Camera::tick`
    /// gives the FOV modifier whenever the winning zoom is smooth,
    /// snapped to 1/factor for an instant zoom (whose division is
    /// exact each frame). No zoom eases it back to 1. Called from
    /// `tick_fov`, once per tick.
    #[cfg(feature = "mods")]
    pub(super) fn tick_mod_zoom_ratio(&mut self) {
        let (factor, smooth, _) = self.zoom_now();
        let target = 1.0 / factor;
        let current = self.mods.zoom_ratio.unwrap_or(1.0);
        self.mods.zoom_ratio = Some(if smooth {
            current + (target - current) * 0.5
        } else {
            target
        });
    }
    /// The field-of-view modifier target, divided by a smooth zoom.
    pub(super) fn zoomed_fov_target(&self, vanilla: f32) -> f32 {
        match self.zoom_now() {
            (factor, true, _) => fov_target(vanilla, factor),
            _ => vanilla,
        }
    }
    /// The field of view in degrees, divided by an instant zoom.
    pub(super) fn zoomed_fov_degrees(&self, fov: f32) -> f32 {
        match self.zoom_now() {
            (factor, false, _) => fov / factor,
            _ => fov,
        }
    }
    /// Mouse turning while zoomed.
    pub(super) fn zoom_turn_scale(&self) -> f64 {
        self.zoom_now().2
    }
    /// The minimap and mods' corner panels, in the HUD layer (hidden with F1
    /// by the caller). The top left corner gives way to the debug screen.
    pub(super) fn draw_extras(&mut self, batch: &mut HudBatch) {
        let mod_panels = self.mods.has_panels();
        let map = self.minimap_shown();
        let visible = |corner| !(self.debug && corner == Corner::TopLeft);
        let mut corners = Corners::new(batch.width, batch.height, self.effect_rows_height());
        #[cfg(target_os = "android")]
        corners.reserve(Corner::TopLeft, 26.0);
        let atlas = &self.gui.atlas;
        // A mod's own minimap image (capability `map`, API
        // 25) stands in for the terrain image at its corner:
        // the uploaded picture drawn at its declared side,
        // with no player arrow over it — the mod's shading is
        // the whole content.
        let picture = self.minimap_picture();
        if let Some(picture) = &picture
            && visible(picture.corner)
            && let Some((_, pw, ph)) = self.extras.minimap_image_shown
            && let Some(slot) = atlas.sprites.get(MOD_MINIMAP_SPRITE).copied()
        {
            let (x, y) = corners.place(picture.corner, picture.size, picture.size);
            let sub = horizon_client::hud::Rect {
                u0: slot.u0,
                v0: slot.v0,
                u1: slot.u0 + (slot.u1 - slot.u0) * (pw as f32 / 256.0),
                v1: slot.v0 + (slot.v1 - slot.v0) * (ph as f32 / 256.0),
                w: pw,
                h: ph,
            };
            batch.rect_uv(x, y, picture.size, picture.size, sub, u32::MAX);
        } else if let Some((map_corner, map_size, view)) = map
            && visible(map_corner)
            && !self.extras.minimap.image.is_empty()
            && let Some(image) = atlas.sprites.get(MINIMAP_SPRITE).copied()
        {
            let (arrow, north) = if view.rotate {
                (0.0, Some(view.north()))
            } else {
                (180.0 + view.yaw, None)
            };
            let look = extras_hud::MinimapLook {
                corner: map_corner,
                size: map_size,
                image,
                arrow,
                north,
            };
            extras_hud::draw_minimap(batch, atlas, &mut corners, &look);
        }
        if mod_panels {
            self.draw_mod_panels(batch, &mut corners);
        }
        self.native_hud(batch, &mut corners);
    }
    /// The height the status effect icons take at the top right
    /// (`Hud.extractEffects`: beneficial at y 1, harmful at y 26).
    fn effect_rows_height(&self) -> f32 {
        let effects = &self.potions.effects.list;
        if effects.is_empty() || self.screen != Screen::Playing {
            return 0.0;
        }
        let harmful = effects.iter().any(|e| {
            horizon_client::brewing::effect(e.id)
                .is_some_and(|d| d.category != horizon_client::brewing::Category::Beneficial)
        });
        if harmful { 50.0 } else { 25.0 }
    }
}
