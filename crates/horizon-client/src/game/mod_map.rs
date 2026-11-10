//! Capability `map` (API 25): the game's map as the mods
//! read and show it — the snapshot's context and last-death
//! reads, the died/respawned pushes, the lent tile view's
//! source data (with the lending itself, beside the world
//! view's, in `game/mods.rs`), the full-screen map the game
//! draws and fields input over, and the mod minimap image's
//! upload. The extras own the cache and its warming
//! (`game/extras.rs`); the runtime owns the stores and the
//! aggregates (`horizon_mods::Mods`).
//!
//! The map screen's state lives here, in
//! [`ModMapScreenState`]: which mod owns the open screen, the
//! set's stamp, the game's view of it (the runtime's store
//! holds the same view — pans and zooms write back at once,
//! so `map.poll-map`'s view events and the game's reading
//! agree), and the press in flight.

use super::Game;
#[cfg(feature = "mods")]
use super::Screen;
use horizon_client::hud::HudBatch;
#[cfg(feature = "mods")]
use horizon_client::{
    extras::Corner,
    hud::{MOD_MAP_SPRITE, Rect, rgba},
};
#[cfg(feature = "mods")]
use horizon_mods::{
    ModMapContext, ModMapCorner, ModMapEvent, ModMapImage, ModPosition, ModPositionEvent,
};

/// A mod's own minimap image (`map.set-minimap-image`), as
/// the extras read it: its corner and side in GUI pixels,
/// its roundness, the set's stamp, and the image itself.
pub(super) struct ModMinimapPicture {
    pub(super) corner: horizon_client::extras::Corner,
    pub(super) size: f32,
    #[cfg(feature = "mods")]
    pub(super) round: bool,
    /// The set's stamp: the upload beside it changes only
    /// when this does.
    #[cfg(feature = "mods")]
    pub(super) seq: u64,
    #[cfg(feature = "mods")]
    pub(super) image: ModMapImage,
}

/// The open map screen's game-side state (API 25), in
/// `ModsState`: the owning mod's entry index, the set's
/// stamp (a newer stamp is a new screen; the same stamp is
/// the same screen, open or player-closed), whether the
/// screen stands open, the game's view of it, the stamp of
/// the image last uploaded to its atlas slot, and a press in
/// flight (where it went down in GUI pixels, the view's
/// centre then, and whether it has dragged).
#[cfg(feature = "mods")]
pub(super) struct ModMapScreenState {
    pub(super) owner: usize,
    pub(super) seq: u64,
    pub(super) open: bool,
    pub(super) view: horizon_mods::ModMapView,
    pub(super) uploaded: Option<u64>,
    pub(super) press: Option<((f32, f32), (f64, f64), bool)>,
}

#[cfg(not(feature = "mods"))]
impl Game<'_> {
    pub(super) fn mod_minimap_image(&self) -> Option<ModMinimapPicture> {
        None
    }
    pub(super) fn mod_wants_map_tiles(&self) -> bool {
        false
    }
    pub(super) fn upload_minimap_picture(&mut self, _picture: &ModMinimapPicture) {}
    pub(super) fn note_mod_position_event(&mut self, _died: bool) {}
    pub(super) fn arm_mod_respawn(&mut self, _was_dead: bool) {}
    pub(super) fn note_mod_respawned(&mut self) {}
    pub(super) fn draw_mod_map_screen(&mut self, _batch: &mut HudBatch) {}
    pub(super) fn mod_map_close(&mut self) {}
    pub(super) fn mod_map_mouse(&mut self, _pressed: bool) {}
    pub(super) fn mod_map_drag(&mut self) {}
    pub(super) fn mod_map_wheel(&mut self, _steps: f32) {}
}

/// A map image's pixels as RGBA bytes (API 25): the palette
/// expanded, an index past the palette's end drawn as clear
/// — checked images only (the runtime's door check bounds
/// the sides), so the expansion is the size it claims.
#[cfg(feature = "mods")]
pub(super) fn expand_map_image(image: &ModMapImage) -> Vec<u8> {
    let mut rgba = vec![0u8; image.pixels.len() * 4];
    for (i, index) in image.pixels.iter().enumerate() {
        let Some(colour) = image.palette.get(*index as usize) else {
            continue;
        };
        rgba[i * 4] = (colour >> 16) as u8;
        rgba[i * 4 + 1] = (colour >> 8) as u8;
        rgba[i * 4 + 2] = *colour as u8;
        rgba[i * 4 + 3] = (colour >> 24) as u8;
    }
    rgba
}

#[cfg(feature = "mods")]
impl Game<'_> {
    /// The first loaded mod's minimap image (API 25), in the
    /// extras' reading.
    pub(super) fn mod_minimap_image(&self) -> Option<ModMinimapPicture> {
        let (_, seq, image) = self.mods.mods.minimap_image()?;
        Some(ModMinimapPicture {
            corner: match image.corner {
                ModMapCorner::TopLeft => Corner::TopLeft,
                ModMapCorner::TopRight => Corner::TopRight,
                ModMapCorner::BottomLeft => Corner::BottomLeft,
                ModMapCorner::BottomRight => Corner::BottomRight,
            },
            size: image.size,
            round: image.round,
            seq,
            image: image.image,
        })
    }

    /// Whether a running mod reads map tiles (API 25): the
    /// extras keep the cache warm for it.
    pub(super) fn mod_wants_map_tiles(&self) -> bool {
        self.mods.mods.wants_map_tiles()
    }

    /// The mod minimap image into its atlas slot, on a change
    /// of the set's stamp: the palette expanded, resampled to
    /// fit the slot when larger, its corners cut away to
    /// clear when the asking mod wants it round. The terrain
    /// image's roundness comes from its composed pixels; a
    /// mod's image gets the same cut here, an ellipse over
    /// its own rectangle. Update-only — the drawn picture is
    /// draw_extras'.
    pub(super) fn upload_minimap_picture(&mut self, picture: &ModMinimapPicture) {
        if self
            .extras
            .minimap_image_shown
            .is_some_and(|(seq, _, _)| seq == picture.seq)
        {
            return;
        }
        let image = &picture.image;
        let (mut w, mut h) = (image.width, image.height);
        let mut pixels = expand_map_image(image);
        if w > 256 || h > 256 {
            // Nearest-sample down to the slot, keeping the
            // aspect: the larger side lands on 256.
            let fit = 256.0 / f32::max(w as f32, h as f32);
            let (nw, nh) = (
                ((w as f32 * fit).round() as u32).max(1),
                ((h as f32 * fit).round() as u32).max(1),
            );
            let mut out = vec![0u8; (nw * nh * 4) as usize];
            for y in 0..nh {
                for x in 0..nw {
                    let (sx, sy) = (x * w / nw, y * h / nh);
                    out[((y * nw + x) * 4) as usize..][..4]
                        .copy_from_slice(&pixels[((sy * w + sx) * 4) as usize..][..4]);
                }
            }
            (w, h, pixels) = (nw, nh, out);
        }
        if picture.round {
            let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
            for y in 0..h {
                for x in 0..w {
                    let (dx, dy) = ((x as f32 + 0.5 - cx) / cx, (y as f32 + 0.5 - cy) / cy);
                    if dx * dx + dy * dy > 1.0 {
                        pixels[((y * w + x) * 4 + 3) as usize] = 0;
                    }
                }
            }
        }
        let Some(rect) = self
            .gui
            .atlas
            .sprites
            .get(horizon_client::hud::MOD_MINIMAP_SPRITE)
            .copied()
        else {
            return;
        };
        let (aw, ah) = (self.gui.atlas.width as f32, self.gui.atlas.height as f32);
        self.map_patches.push(horizon_client::font::Patch {
            x: (rect.u0 * aw).round() as u32,
            y: (rect.v0 * ah).round() as u32,
            width: w,
            height: h,
            rgba: pixels,
        });
        self.extras.minimap_image_shown = Some((picture.seq, w, h));
    }

    /// The snapshot's map reads (API 25), filled beside the
    /// self-state reads: the place the player is in — the
    /// dimension, and the single-player world's name or the
    /// server's address as the player typed it — and the
    /// game's own last-death place. Nothing is read while no
    /// mod imports either read, and the context is none with
    /// no player in a world (the title screen).
    pub(super) fn mod_map_reads(&self) -> (Option<ModMapContext>, Option<ModPosition>) {
        if !self.mods.mods.wants_map() || self.player.is_none() {
            return (None, None);
        }
        let world = match self.server_address() {
            Some(address) => address.to_owned(),
            None => match &self.integrated {
                Some(server) if !server.world_name.is_empty() => server.world_name.clone(),
                _ => self.level.name.clone(),
            },
        };
        let context = ModMapContext {
            dimension: self.dim.id().to_owned(),
            world,
        };
        let last_death = self
            .level
            .last_death
            .as_ref()
            .map(|(dimension, at)| ModPosition {
                dimension: dimension.clone(),
                x: at[0],
                y: at[1],
                z: at[2],
            });
        (Some(context), last_death)
    }

    /// The player died or came back (API 25): pushed to the
    /// running `map` mods at the game's own moments — a death
    /// where its screen opens (the callers' guard is the
    /// dedupe law, `mods::death_opens_screen`), a respawn
    /// where one is applied. The place is the player's feet,
    /// floored, in the dimension they stand in. A fresh death
    /// also spends any armed respawn: the pair stays a pair.
    pub(super) fn note_mod_position_event(&mut self, died: bool) {
        if died {
            self.mods.respawn_armed = false;
        }
        let Some(p) = self.player.as_ref() else {
            return;
        };
        let at = ModPosition {
            dimension: self.dim.id().to_owned(),
            x: p.position.x.floor() as i32,
            y: p.position.y.floor() as i32,
            z: p.position.z.floor() as i32,
        };
        let event = if died {
            ModPositionEvent::Died(at)
        } else {
            ModPositionEvent::Respawned(at)
        };
        self.mods.mods.push_position_event(event);
    }

    /// A dimension (re)entry on a server (API 25): the respawn
    /// it may carry is owed a `respawned` push only when the
    /// player died into it — remembered from the screen that
    /// stood when the entry began, and spent where the
    /// replacement player is made (`note_mod_respawned`).
    pub(super) fn arm_mod_respawn(&mut self, was_dead: bool) {
        self.mods.respawn_armed = was_dead;
    }

    /// The replacement player a respawn entry armed for is
    /// made (API 25): the respawn is applied — push it, once.
    pub(super) fn note_mod_respawned(&mut self) {
        if self.mods.respawn_armed {
            self.mods.respawn_armed = false;
            self.note_mod_position_event(false);
        }
    }

    /// The map screen's bookkeeping, once a frame before the
    /// frame hooks' outputs settle (API 25): the runtime's
    /// winning screen against the game's state. A newer set
    /// opens the screen (an older owner's open screen hears
    /// `closed` first); the same set keeps the player's own
    /// open or closed word; a screen another screen replaced
    /// hears `closed`; a screen the mod cleared or lost by
    /// stopping closes silently, the `set-camera` override's
    /// own law.
    pub(super) fn sync_mod_map_screen(&mut self) {
        let aggregate = self.mods.mods.map_screen();
        let mut open = false;
        let mut close_screen = false;
        let mut closed_for: Option<usize> = None;
        {
            let state = &mut self.mods.map_screen;
            match (aggregate.as_ref(), state.as_mut()) {
                (Some((owner, seq, _, view)), Some(held)) if *seq > held.seq => {
                    if held.open && held.owner != *owner {
                        closed_for = Some(held.owner);
                    }
                    *held = ModMapScreenState {
                        owner: *owner,
                        seq: *seq,
                        open: true,
                        view: *view,
                        uploaded: None,
                        press: None,
                    };
                    open = true;
                }
                (Some((owner, seq, _, view)), None) => {
                    *state = Some(ModMapScreenState {
                        owner: *owner,
                        seq: *seq,
                        open: true,
                        view: *view,
                        uploaded: None,
                        press: None,
                    });
                    open = true;
                }
                (Some((_, seq, _, _)), Some(held)) if *seq == held.seq => {
                    // The same set: the player's own open or
                    // closed word stands — unless another
                    // screen took the open one's place.
                    if held.open && self.screen != Screen::Map {
                        closed_for = Some(held.owner);
                        held.open = false;
                        held.press = None;
                    }
                }
                (Some(_), Some(held)) => {
                    // An older set resurfaced (the winner
                    // cleared its screen, or stopped): never
                    // reopen — the held screen closes silently.
                    if held.open {
                        close_screen = self.screen == Screen::Map;
                        held.open = false;
                        held.press = None;
                    }
                }
                (None, Some(held)) => {
                    if held.open {
                        close_screen = self.screen == Screen::Map;
                        held.open = false;
                        held.press = None;
                    }
                }
                (None, None) => {}
            }
        }
        if let Some(owner) = closed_for {
            self.mods.mods.push_map_event(owner, ModMapEvent::Closed);
        }
        if close_screen {
            self.screen = Screen::Playing;
            self.capture(true);
        }
        if open {
            self.screen = Screen::Map;
            self.capture(false);
        }
        // The winning screen's image into its atlas slot, on
        // the set's change.
        if let Some((_, seq, screen, _)) = self.mods.mods.map_screen()
            && let Some(held) = &mut self.mods.map_screen
            && held.seq == seq
            && held.uploaded != Some(seq)
        {
            self.upload_mod_map_image(&screen.image);
            if let Some(held) = &mut self.mods.map_screen {
                held.uploaded = Some(seq);
            }
        }
    }

    /// A map screen's image into its atlas slot (API 25): the
    /// palette expanded at the slot's top left, at its own
    /// size — the slot is the interface's largest image, so
    /// nothing is resampled.
    fn upload_mod_map_image(&mut self, image: &ModMapImage) {
        let Some(rect) = self.gui.atlas.sprites.get(MOD_MAP_SPRITE).copied() else {
            return;
        };
        let (aw, ah) = (self.gui.atlas.width as f32, self.gui.atlas.height as f32);
        self.map_patches.push(horizon_client::font::Patch {
            x: (rect.u0 * aw).round() as u32,
            y: (rect.v0 * ah).round() as u32,
            width: image.width,
            height: image.height,
            rgba: expand_map_image(image),
        });
    }

    /// Draws the open map screen (API 25): the mod's image
    /// under the game's view of it, full-window over a dark
    /// ground, its markers as squares with their letter and
    /// label. The view's window into the image is clamped to
    /// the image, so the edges show the image's edge, never
    /// past it.
    pub(super) fn draw_mod_map_screen(&mut self, batch: &mut HudBatch) {
        let Some(held) = &self.mods.map_screen else {
            return;
        };
        if !held.open {
            return;
        }
        let Some((_, _, screen, _)) = self.mods.mods.map_screen() else {
            return;
        };
        if screen.image.width == 0 || screen.image.height == 0 {
            return;
        }
        let atlas = &self.gui.atlas;
        let (w, h) = (batch.width, batch.height);
        batch.fill_gradient(
            atlas,
            (0.0, 0.0, w, h),
            rgba(16, 16, 20, 255),
            rgba(8, 8, 12, 255),
        );
        let Some(slot) = atlas.sprites.get(MOD_MAP_SPRITE).copied() else {
            return;
        };
        let view = held.view;
        let image = &screen.image;
        // The image's top-left corner in the world, and the
        // view window's edges in image pixels.
        let (iw, ih) = (f64::from(image.width), f64::from(image.height));
        let ibpp = f64::from(image.blocks_per_pixel);
        let left = f64::from(image.center_x) - iw / 2.0 * ibpp;
        let top = f64::from(image.center_z) - ih / 2.0 * ibpp;
        let bpp = f64::from(view.blocks_per_pixel);
        let (cx, cy) = ((w / 2.0) as f64, (h / 2.0) as f64);
        let x0 = ((view.center_x - cx * bpp - left) / ibpp).clamp(0.0, iw);
        let x1 = ((view.center_x + cx * bpp - left) / ibpp).clamp(0.0, iw);
        let y0 = ((view.center_z - cy * bpp - top) / ibpp).clamp(0.0, ih);
        let y1 = ((view.center_z + cy * bpp - top) / ibpp).clamp(0.0, ih);
        if x1 > x0 && y1 > y0 {
            let sub = Rect {
                u0: slot.u0 + (slot.u1 - slot.u0) * (x0 as f32 / 2048.0),
                v0: slot.v0 + (slot.v1 - slot.v0) * (y0 as f32 / 2048.0),
                u1: slot.u0 + (slot.u1 - slot.u0) * (x1 as f32 / 2048.0),
                v1: slot.v0 + (slot.v1 - slot.v0) * (y1 as f32 / 2048.0),
                w: image.width,
                h: image.height,
            };
            // The window of the world the sub-rect shows, back
            // in GUI pixels: the image's edge may cut it short
            // of the window's edge.
            let gx = (left + x0 * ibpp - view.center_x) / bpp + cx;
            let gy = (top + y0 * ibpp - view.center_z) / bpp + cy;
            let gw = (x1 - x0) * ibpp / bpp;
            let gh = (y1 - y0) * ibpp / bpp;
            batch.rect_uv(gx as f32, gy as f32, gw as f32, gh as f32, sub, u32::MAX);
        }
        // The markers over it: a 5px square in the mark's
        // colour, the label's first letter centred on it, and
        // the label centred under it.
        for marker in &screen.markers {
            let sx = cx + (f64::from(marker.x) - view.center_x) / bpp;
            let sy = cy + (f64::from(marker.z) - view.center_z) / bpp;
            if sx < -16.0 || sy < -16.0 || sx > f64::from(w) + 16.0 || sy > f64::from(h) + 16.0 {
                continue;
            }
            let colour = rgba(
                (marker.color >> 16) as u8,
                (marker.color >> 8) as u8,
                marker.color as u8,
                (marker.color >> 24) as u8,
            );
            batch.fill(atlas, sx as f32 - 2.5, sy as f32 - 2.5, 5.0, 5.0, colour);
            if let Some(letter) = marker.label.chars().next() {
                let text = letter.to_string();
                let tw = HudBatch::text_width(atlas, &text);
                batch.text(
                    atlas,
                    &text,
                    (sx as f32 - tw / 2.0).floor(),
                    (sy as f32 - 4.0).floor(),
                    u32::MAX,
                    true,
                );
                let tw = HudBatch::text_width(atlas, &marker.label);
                batch.text(
                    atlas,
                    &marker.label,
                    (sx as f32 - tw / 2.0).floor(),
                    (sy as f32 + 4.0).floor(),
                    u32::MAX,
                    true,
                );
            }
        }
    }

    /// Escape on the map screen (API 25): it closes, the mod
    /// hears `closed`, and the pointer is the game's again.
    pub(super) fn mod_map_close(&mut self) {
        let Some(held) = &mut self.mods.map_screen else {
            return;
        };
        if !held.open {
            return;
        }
        held.open = false;
        held.press = None;
        let owner = held.owner;
        self.mods.mods.push_map_event(owner, ModMapEvent::Closed);
        if self.screen == Screen::Map {
            self.screen = Screen::Playing;
            self.capture(true);
        }
    }

    /// The left button on the map screen (API 25): a press
    /// starts a possible drag (or a click); a release ends it
    /// — a drag reports the view it settled on, a click the
    /// world place under the pointer.
    pub(super) fn mod_map_mouse(&mut self, pressed: bool) {
        let size = self.gui_size();
        let Some(held) = &mut self.mods.map_screen else {
            return;
        };
        if !held.open || self.screen != Screen::Map {
            return;
        }
        if pressed {
            held.press = Some((self.cursor, (held.view.center_x, held.view.center_z), false));
            return;
        }
        let Some((at, _, dragged)) = held.press.take() else {
            return;
        };
        let owner = held.owner;
        if dragged {
            let view = held.view;
            self.mods.mods.set_map_view(owner, view);
            self.mods
                .mods
                .push_map_event(owner, ModMapEvent::View(view));
        } else {
            let (x, z) = super::mods::map_click_world(held.view, at, size);
            self.mods.mods.push_map_event(
                owner,
                ModMapEvent::Clicked(horizon_mods::ModMapClick { x, z, button: 0 }),
            );
        }
    }

    /// The pointer moved on the map screen (API 25): a press
    /// in flight pans the view with it, once it has moved
    /// past a few pixels — a smaller move is still a click.
    pub(super) fn mod_map_drag(&mut self) {
        let Some(held) = &mut self.mods.map_screen else {
            return;
        };
        if !held.open || self.screen != Screen::Map {
            return;
        }
        let Some((at, centre, dragged)) = &mut held.press else {
            return;
        };
        let delta = (self.cursor.0 - at.0, self.cursor.1 - at.1);
        if !*dragged && delta.0 * delta.0 + delta.1 * delta.1 < 16.0 {
            return;
        }
        *dragged = true;
        let Some((_, _, screen, _)) = self.mods.mods.map_screen() else {
            return;
        };
        let mut view = held.view;
        view.center_x = centre.0;
        view.center_z = centre.1;
        held.view = super::mods::map_pan(view, &screen.image, delta);
    }

    /// The wheel on the map screen (API 25): the view zooms
    /// toward the pointer, a step a doubling, and the mod
    /// hears the view it settled on.
    pub(super) fn mod_map_wheel(&mut self, steps: f32) {
        let size = self.gui_size();
        let cursor = self.cursor;
        let Some(held) = &mut self.mods.map_screen else {
            return;
        };
        if !held.open || self.screen != Screen::Map {
            return;
        }
        let view = super::mods::map_zoom(held.view, cursor, size, steps);
        held.view = view;
        let owner = held.owner;
        self.mods.mods.set_map_view(owner, view);
        self.mods
            .mods
            .push_map_event(owner, ModMapEvent::View(view));
    }
}
