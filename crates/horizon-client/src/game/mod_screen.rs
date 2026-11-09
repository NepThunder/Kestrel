//! Capability `screen` (API 22): the game's own screens as the
//! mods see and decorate them — the snapshot's read of the open
//! screen and the merchant's offers, the buttons the game
//! places for the mods at the screens' own anchors, the slot
//! highlights, the middle click's observation and the title
//! screen's wiring. The debug claim's read and the trade
//! choices' drain live with the mods' state (`game/mods.rs`);
//! everything here is geometry and reads.
//!
//! Resolution lives in the runtime (`Mods::screen_buttons`,
//! load order, first eight per context); placement lives here,
//! and draw and press share the one placement function, so a
//! drawn button is the button a press hits.

use super::{Game, Screen};
use horizon_client::{
    hud::{HudBatch, rgba},
    items::ItemStack,
    merchant::MerchantOffer,
    text::Text,
    ui,
};
use horizon_mods::{
    ModAnchor, ModScreenButton, ModScreenEvent, ModScreenState, ModTradeList, ModTradeOffer,
    ModTradeStack,
};

/// A mod button's height on an inventory screen, and the title
/// row's inset from the panel's right edge: the game's own sort
/// button is a 9px square at the title's line; a labelled button
/// needs a little more room, at the same line (its row starts
/// 3px under the panel's top, the sort button's 5px row minus
/// the height's growth).
const BUTTON_HEIGHT: f32 = 12.0;

impl Game<'_> {
    /// The buttons the running mods declared for one open
    /// context (API 22): the runtime's aggregate, which the
    /// game's draw, press and sort-suppression paths all read,
    /// so they can never disagree about which buttons serve a
    /// screen.
    pub(super) fn mod_screen_buttons(
        &self,
        screen: &str,
        menu: Option<&str>,
    ) -> Vec<(usize, ModScreenButton)> {
        self.mods.mods.screen_buttons(screen, menu)
    }

    /// A press on mod `owner`'s declared button `id` (API 22):
    /// queued for its `screen.poll`.
    pub(super) fn mod_screen_event_button(&mut self, owner: usize, id: u32) {
        self.mods
            .mods
            .screen_event(owner, ModScreenEvent::Button(id));
    }

    /// Whether any running mod stands in for the game's debug
    /// overlay (API 22): the gate `draw_debug_overlay_under`
    /// asks beside its own.
    pub(super) fn mod_debug_claimed(&self) -> bool {
        self.mods.mods.debug_claimed()
    }

    /// The snapshot's screen read (API 22), filled beside the
    /// open container: the open screen's id in `Screen::id`'s
    /// stable strings (the title screen is its own state, as
    /// the native hooks read it), the open menu's type and
    /// plain-text title, the panel's rect by the native view's
    /// own recipe (`screen_rect`), the hovered slot in
    /// `inventory`'s numbering, and whether the debug overlay
    /// is open — the draw's own predicate, so a mod standing
    /// in for it reads the state it replaced. The merchant's
    /// offers ride along, from the session the screen shows.
    /// Nothing is read while no mod imports the interface.
    pub(super) fn mod_screen_reads(&mut self) -> (ModScreenState, Option<ModTradeList>) {
        let mut state = ModScreenState {
            id: "playing".into(),
            menu: None,
            title: None,
            rect: (0.0, 0.0, 0.0, 0.0),
            hovered_slot: None,
            debug_open: false,
        };
        if !self.mods.mods.wants_screen() {
            return (state, None);
        }
        state.debug_open = self
            .debug_entries
            .any_enabled(self.debug, self.reduced_debug_info());
        // The window the rects are measured in: the renderer's,
        // or the GUI's own size before the first frame.
        let batch = self
            .renderer
            .as_ref()
            .map(|r| HudBatch::new([r.config.width, r.config.height]));
        if self.title.is_some() {
            state.id = "title".into();
            state.rect = match &batch {
                Some(batch) => (0.0, 0.0, batch.width, batch.height),
                None => {
                    let (w, h) = self.gui_size();
                    (0.0, 0.0, w, h)
                }
            };
            return (state, None);
        }
        state.id = self.screen.id().into();
        state.menu = self
            .remote_menu_type()
            .filter(|m| !m.is_empty())
            .map(str::to_owned);
        state.title = self.remote_menu_title().map(|t| t.plain());
        state.rect = match &batch {
            Some(batch) => {
                let r = self.screen_rect(batch, self.screen);
                (r[0], r[1], r[2], r[3])
            }
            None => {
                let (w, h) = self.gui_size();
                (0.0, 0.0, w, h)
            }
        };
        if matches!(self.screen, Screen::Inventory | Screen::Creative) {
            state.hovered_slot = self.inventory_target().0.map(|s| s as u32);
        }
        let trades = self.mod_trades();
        (state, trades)
    }

    /// The open merchant's offers for the snapshot (API 22):
    /// each at the count the screen charges — cost A adjusted
    /// by the villager (`cost_a_count`), not the base count —
    /// with the screen's selected offer, and whether the offer
    /// reads as one the player can trade: not locked, and the
    /// payments staged in the trade slots (either way round,
    /// as the menu's own match reads them) or takeable from
    /// the player's inventory now, cost A paid before cost B
    /// from the one pool.
    fn mod_trades(&mut self) -> Option<ModTradeList> {
        // The staged payments first: reading a slot wants
        // `&mut self`, the offers below only `&self`.
        let staged_a = self.slot_view(super::stations::INPUT);
        let staged_b = self.slot_view(super::stations::INPUT + 1);
        let session = self
            .station
            .as_ref()
            .and_then(|s| s.merchant.as_ref())
            .map(|m| &m.session)?;
        let offers = session.offers.clone();
        let selected =
            (session.selection_hint < offers.len()).then_some(session.selection_hint as u32);
        let items = self.items();
        // The pool a payment may be taken from: the player's
        // own slots, then what is already staged.
        let mut pool: Vec<&ItemStack> = self.inventory.iter().filter_map(|s| s.as_ref()).collect();
        if let Some(s) = &staged_a {
            pool.push(s);
        }
        if let Some(s) = &staged_b {
            pool.push(s);
        }
        let offers = offers
            .iter()
            .map(|offer| {
                let staged = offer.satisfied_by(items, staged_a.as_ref(), staged_b.as_ref())
                    || offer.satisfied_by(items, staged_b.as_ref(), staged_a.as_ref());
                ModTradeOffer {
                    cost_a: trade_stack(&offer.cost_a(items)),
                    cost_b: offer.cost_b().as_ref().map(trade_stack),
                    result: trade_stack(&offer.result),
                    uses: offer.uses.max(0) as u32,
                    max_uses: offer.max_uses.max(0) as u32,
                    available: !offer.out_of_stock() && (staged || can_pay(offer, items, &pool)),
                }
            })
            .collect();
        Some(ModTradeList { offers, selected })
    }

    /// This mod context's placed buttons on the open inventory
    /// screen (API 22): the resolved declarations laid at their
    /// anchors against the screen's own panel (`container_image`,
    /// so a station's wider or shifted panel is the panel the
    /// buttons follow). Anchors that belong to other screens
    /// place nothing here.
    fn mod_placed_buttons(&self, batch: &HudBatch) -> Vec<(usize, ModScreenButton, [f32; 4])> {
        if self.screen != Screen::Inventory {
            return Vec::new();
        }
        let resolved = self.mod_screen_buttons(self.screen.id(), self.remote_menu_type());
        if resolved.is_empty() {
            return Vec::new();
        }
        let image = self.container_image(batch);
        let panel = (
            image.left as f32,
            image.top as f32,
            image.width as f32,
            image.height as f32,
        );
        screen_button_rects(&resolved, panel, |label| {
            HudBatch::text_width(&self.gui.atlas, label)
        })
    }

    /// Draws the open inventory screen's mod buttons (API 22),
    /// after the game's own sort buttons: the sort button's own
    /// dress — a dark border, a grey face, lighter under the
    /// cursor — widened to the label, with the declared item's
    /// icon leading it.
    pub(super) fn draw_mod_screen_buttons(&self, batch: &mut HudBatch) {
        let atlas = &self.gui.atlas;
        for (_, button, [x, y, w, h]) in self.mod_placed_buttons(batch) {
            let face = if ui::hovered(self.cursor, x, y, w, h) {
                rgba(220, 220, 220, 255)
            } else {
                rgba(160, 160, 160, 255)
            };
            batch.fill(atlas, x, y, w, h, rgba(55, 55, 55, 255));
            batch.fill(atlas, x + 1.0, y + 1.0, w - 2.0, h - 2.0, face);
            let mut tx = x + 3.0;
            if let Some(icon) = &button.icon {
                let stack = ItemStack {
                    item: icon.as_str().into(),
                    count: 1,
                    damage: 0,
                    extra: None,
                };
                // An item icon is 16px on a 12px button: drawn at
                // its natural size, centred on the row.
                self.draw_item(batch, &stack, tx - 1.0, y - 2.0);
                tx += 14.0;
            }
            batch.text(
                atlas,
                &button.label,
                tx,
                y + 2.0,
                rgba(64, 64, 64, 255),
                false,
            );
        }
    }

    /// A left press on one of the open inventory screen's mod
    /// buttons (API 22): the press is the player's own, queued
    /// for the button's mod, and the screen's slots never see
    /// it — the `sort_press` shape, at the same point of
    /// `inventory_press`.
    pub(super) fn mod_screen_press(&mut self) -> bool {
        let Some(r) = self.renderer.as_ref() else {
            return false;
        };
        let batch = HudBatch::new([r.config.width, r.config.height]);
        for (owner, button, rect) in self.mod_placed_buttons(&batch) {
            if ui::hovered(self.cursor, rect[0], rect[1], rect[2], rect[3]) {
                self.mod_screen_event_button(owner, button.id);
                return true;
            }
        }
        false
    }

    /// Draws the mods' slot marks on the open screen (API 22):
    /// each mark's colour translucent over its slot, in the
    /// runtime's union order, over the slot's stack and under
    /// its tooltip — the caller places this after the slots
    /// and before the carried stack and the tooltips. A slot
    /// the open screen does not show is not marked.
    pub(super) fn draw_mod_screen_highlights(&self, batch: &mut HudBatch, ox: f32, oy: f32) {
        let marks = self.mods.mods.screen_highlights();
        if marks.is_empty() {
            return;
        }
        let slots = self.slots();
        let atlas = &self.gui.atlas;
        for mark in marks {
            let slot = mark.slot as usize;
            if !slots.contains(&slot) {
                continue;
            }
            let Some((sx, sy)) = self.slot_position_any(slot) else {
                continue;
            };
            let (r, g, b) = (
                (mark.color >> 16) as u8,
                (mark.color >> 8) as u8,
                mark.color as u8,
            );
            batch.fill(atlas, ox + sx, oy + sy, 16.0, 16.0, rgba(r, g, b, 96));
        }
    }

    /// The middle button went down over the open
    /// inventory/container screen (API 22): every running
    /// `screen` mod hears it, with the slot under the pointer
    /// or none. Observation only — the press itself is the
    /// game's to use as it always has (a creative clone, a
    /// pick), and nothing here consumes it.
    pub(super) fn mod_screen_middle_click(&mut self) {
        if !self.mods.mods.wants_screen() {
            return;
        }
        let hovered = self.inventory_target().0.map(|s| s as u32);
        self.mods.mods.screen_middle_click(hovered);
    }

    /// Whether a mod's buttons serve the area the game's sort
    /// button at `container_area` serves (API 22): while they
    /// do, the game's own sort button for that area is not
    /// drawn and does not take presses — the mods' buttons sit
    /// where it sat. The sort *key* is untouched: it reads
    /// `sort_buttons`, which this never narrows.
    pub(super) fn mod_sort_suppressed(&self, container_area: bool) -> bool {
        self.mod_screen_buttons("inventory", self.remote_menu_type())
            .iter()
            .any(|(_, b)| match b.anchor {
                ModAnchor::ContainerHead => container_area,
                ModAnchor::InventoryHead => !container_area,
                ModAnchor::MenuColumn | ModAnchor::TitleRow => false,
            })
    }

    /// The title screen's mod rows (API 22): below the main
    /// menu's own column — its last row ends 116px under the
    /// column's top (`Title::buttons`), the mods' rows follow
    /// at the column's own 24px step and 200px width. Only on
    /// the main menu: a button declared for the title is not
    /// drawn over its world list or dialogs.
    fn mod_title_rows(&self, w: f32, h: f32) -> Vec<(usize, ModScreenButton, [f32; 4])> {
        if !self
            .title
            .as_ref()
            .is_some_and(|t| matches!(t.mode, horizon_client::title::Mode::Main))
        {
            return Vec::new();
        }
        let cx = (w / 2.0).floor();
        let y0 = (h / 4.0).floor() + 48.0;
        self.mod_screen_buttons("title", None)
            .into_iter()
            .enumerate()
            .map(|(i, (owner, button))| {
                (
                    owner,
                    button,
                    [cx - 100.0, y0 + 120.0 + 24.0 * i as f32, 200.0, 20.0],
                )
            })
            .collect()
    }

    /// Draws the title screen's mod buttons (API 22) under the
    /// title's own, in its own button dress.
    pub(super) fn draw_mod_title_buttons(&self, batch: &mut HudBatch) {
        let atlas = &self.gui.atlas;
        for (_, button, [x, y, w, h]) in self.mod_title_rows(batch.width, batch.height) {
            let hover = ui::hovered(self.cursor, x, y, w, h);
            ui::button(
                batch,
                atlas,
                x,
                y,
                w,
                h,
                &Text::literal(button.label.clone()),
                true,
                hover,
            );
        }
    }

    /// A left press on one of the title screen's mod buttons
    /// (API 22): the press is queued for the button's mod and
    /// the title never sees it. The mod hears it at its next
    /// frame hook — the title's frames run the mods' frame
    /// hooks as the world's do (`mods_frame`).
    pub(super) fn mod_title_press(&mut self, w: f32, h: f32) -> bool {
        for (owner, button, rect) in self.mod_title_rows(w, h) {
            if ui::hovered(self.cursor, rect[0], rect[1], rect[2], rect[3]) {
                self.mod_screen_event_button(owner, button.id);
                return true;
            }
        }
        false
    }

    /// The pause menu's mod rows (API 22): below the menu's own
    /// last row (the grid's y + 146 at height 20), full width,
    /// at the column's own 24px step.
    fn mod_pause_rows(
        &self,
        w: f32,
        h: f32,
        row3: bool,
    ) -> Vec<(usize, ModScreenButton, [f32; 4])> {
        let (x0, y0) = super::screens::pause_grid(w, h, row3);
        let first_y = y0 + 170.0 + if row3 { 24.0 } else { 0.0 };
        self.mod_screen_buttons("paused", None)
            .into_iter()
            .enumerate()
            .map(|(i, (owner, button))| {
                (
                    owner,
                    button,
                    [x0 + 4.0, first_y + 24.0 * i as f32, 204.0, 20.0],
                )
            })
            .collect()
    }

    /// Draws the pause menu's mod buttons (API 22) after its
    /// own rows, in its own button dress; their labels are the
    /// mods' words, untranslated.
    pub(super) fn draw_mod_pause_buttons(&self, batch: &mut HudBatch, row3: bool) {
        let atlas = &self.gui.atlas;
        for (_, button, [x, y, w, h]) in self.mod_pause_rows(batch.width, batch.height, row3) {
            let hover = ui::hovered(self.cursor, x, y, w, h);
            ui::button(
                batch,
                atlas,
                x,
                y,
                w,
                h,
                &Text::literal(button.label.clone()),
                true,
                hover,
            );
        }
    }

    /// A left press on one of the pause menu's mod buttons
    /// (API 22): the press is queued for the button's mod; the
    /// caller plays the click, as it does for its own buttons.
    pub(super) fn mod_pause_press(&mut self, w: f32, h: f32, row3: bool) -> bool {
        for (owner, button, rect) in self.mod_pause_rows(w, h, row3) {
            if ui::hovered(self.cursor, rect[0], rect[1], rect[2], rect[3]) {
                self.mod_screen_event_button(owner, button.id);
                return true;
            }
        }
        false
    }
}

/// One trade's stack as the WIT record spells it: the charged
/// stack itself (`cost_a` already adjusted), its components in
/// the `browser.stack` spelling.
fn trade_stack(stack: &ItemStack) -> ModTradeStack {
    ModTradeStack {
        item: stack.item.to_string(),
        count: stack.count,
        components: super::mod_inventory::components(stack),
    }
}

/// Whether `offer` can be paid from `pool` now (API 22): cost
/// A's charged count first, then cost B, each stack spent at
/// most once across both — the menu's own `take`, read-only.
fn can_pay(
    offer: &MerchantOffer,
    items: &horizon_client::items::Items,
    pool: &[&ItemStack],
) -> bool {
    let mut remaining: Vec<u32> = pool.iter().map(|s| s.count).collect();
    let mut pay = |cost: &horizon_client::merchant::ItemCost, count: u32| {
        let mut need = count;
        for (i, stack) in pool.iter().enumerate() {
            if need == 0 {
                break;
            }
            if remaining[i] > 0 && cost.test(stack) {
                let take = remaining[i].min(need);
                remaining[i] -= take;
                need -= take;
            }
        }
        need == 0
    };
    pay(&offer.base_cost_a, offer.cost_a_count(items))
        && offer
            .cost_b
            .as_ref()
            .is_none_or(|cost_b| pay(cost_b, cost_b.stack.count))
}

/// Places resolved buttons on an inventory screen's panel
/// (API 22): each anchor's row is laid right-to-left from the
/// panel's right edge — the sort button's own side — in
/// declaration order, the first declared at the edge; a button
/// that would cross the panel's left inset is not placed, and
/// neither are the ones after it on that row. The container
/// row rides the panel's title line, the inventory row the
/// "Inventory" label's line (the sort buttons' own rows: 5px
/// and `height - 95`, the labels' baselines just under them).
fn screen_button_rects(
    resolved: &[(usize, ModScreenButton)],
    panel: (f32, f32, f32, f32),
    text_width: impl Fn(&str) -> f32,
) -> Vec<(usize, ModScreenButton, [f32; 4])> {
    let (left, top, width, height) = panel;
    let mut out = Vec::new();
    for (container_row, row_y) in [(true, top + 3.0), (false, top + height - 96.0)] {
        let mut right = left + width - 7.0;
        for (owner, button) in resolved {
            let on_row = match button.anchor {
                ModAnchor::ContainerHead => container_row,
                ModAnchor::InventoryHead => !container_row,
                ModAnchor::MenuColumn | ModAnchor::TitleRow => false,
            };
            if !on_row {
                continue;
            }
            let icon_w = if button.icon.is_some() { 14.0 } else { 0.0 };
            let w = (icon_w + text_width(&button.label) + 6.0).max(14.0);
            let x = right - w;
            if x < left + 8.0 {
                break;
            }
            out.push((*owner, button.clone(), [x, row_y, w, BUTTON_HEIGHT]));
            right = x - 2.0;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{ModAnchor, ModScreenButton, screen_button_rects};

    fn button(id: u32, anchor: ModAnchor, label: &str) -> (usize, ModScreenButton) {
        (
            0,
            ModScreenButton {
                id,
                screen: "inventory".into(),
                menu: None,
                anchor,
                label: label.into(),
                icon: None,
            },
        )
    }

    /// Button placement on an inventory panel (API 22): the
    /// container row rides the title line, the inventory row
    /// the label's line, both laid from the panel's right edge
    /// in declaration order, and a row stops at the panel's
    /// left inset instead of overflowing it.
    #[test]
    fn buttons_lay_out_from_the_right_edge_per_anchor_row() {
        // A chest's panel, 176 wide at (75, 17); labels are 6px
        // a character in this fixture, so each button is
        // label + 6 wide.
        let panel = (75.0, 17.0, 176.0, 166.0);
        let width = |label: &str| label.len() as f32 * 6.0;
        let resolved = vec![
            button(1, ModAnchor::ContainerHead, "Sort"),
            button(2, ModAnchor::ContainerHead, "All"),
            button(3, ModAnchor::InventoryHead, "Mine"),
            button(4, ModAnchor::MenuColumn, "Nope"),
        ];
        let placed = screen_button_rects(&resolved, panel, width);
        // The menu-column button places nothing here.
        assert_eq!(placed.len(), 3);
        // Container row at the title line (17 + 3), first
        // declared at the right edge (75 + 176 - 7 = 244):
        // "Sort" is 30 wide, "All" 24, 2px apart.
        assert_eq!(placed[0].2, [214.0, 20.0, 30.0, 12.0]);
        assert_eq!(placed[1].2, [188.0, 20.0, 24.0, 12.0]);
        // Inventory row at the label's line (17 + 166 - 96).
        assert_eq!(placed[2].2, [214.0, 87.0, 30.0, 12.0]);
        // A row that would cross the left inset (75 + 8) stops:
        // five 30-wide buttons fit in 244 - 83, the sixth not.
        let many: Vec<_> = (0..8)
            .map(|i| button(i, ModAnchor::ContainerHead, "Sort"))
            .collect();
        let placed = screen_button_rects(&many, panel, width);
        assert_eq!(placed.len(), 5);
        assert!(placed.iter().all(|(_, _, r)| r[0] >= 83.0));
    }
}
