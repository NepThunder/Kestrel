//! Capability `render`'s world shapes (docs/mods.md): the lines and boxes
//! a mod submits with `render.set-world`, drained into the frame's
//! depth-tested lines at the fishing-line/debug-gizmos merge point, and
//! only while a mod draws them (`Mods::wants_world_shapes`). With no
//! world-shape mod on, this file does nothing.
//!
//! Depth-tested only in v1: the shapes join `lines`, never `on_top`, so
//! a wall hides them exactly like the game's own F3+B boxes. They hide
//! with the HUD (F1), like the block outline: the drain runs only while
//! the HUD shows.
use super::Game;
use horizon_client::gizmos::Line;

#[cfg(feature = "mods")]
impl Game<'_> {
    /// The running mods' world shapes, appended to the frame's
    /// depth-tested lines: nothing when the HUD is hidden (F1) or no
    /// running mod draws world shapes.
    pub(super) fn mod_world_lines(&self, lines: &mut Vec<Line>) {
        if self.ui.hide_gui || !self.mods.mods.wants_world_shapes() {
            return;
        }
        lines.extend(
            self.mods
                .mods
                .world_lines()
                .into_iter()
                .map(|(start, end, color, width)| Line {
                    start,
                    end,
                    color,
                    width,
                }),
        );
    }
}
