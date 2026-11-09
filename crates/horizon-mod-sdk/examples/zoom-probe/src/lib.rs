//! Prototype probe for the render/input extensions (API 19): each
//! tick it shows whether a mod's zoom would take effect now
//! (`render.can-zoom`), the spyglass's own zoom while scoping
//! (`render.get-spyglass-zoom`) and the time since the player's
//! last input (`input.last-input-ms`) in a HUD panel, and it makes
//! the hand's field of view follow the view's zoom
//! (`render.set-hand-fov`, multiply over the vanilla 70).
use horizon_mod_sdk::{
    HandFov, HandFovMode, Mod, Registration,
    hud::{self, Corner, Line},
    input, render,
};

struct ZoomProbe {
    last_panel: Vec<String>,
}

/// The panel's lines for this tick: the zoom's truth, the scope's
/// factor, and the idle clock in milliseconds.
fn panel_lines() -> Vec<String> {
    let zoom = if render::can_zoom() {
        "zoom live"
    } else {
        "zoom kept"
    };
    let spy = match render::get_spyglass_zoom() {
        Some(factor) => format!("spy {factor}"),
        None => "spy none".into(),
    };
    vec![
        zoom.into(),
        spy,
        format!("idle {}", input::last_input_ms()),
    ]
}

impl Mod for ZoomProbe {
    fn init() -> Result<(Self, Registration), String> {
        Ok((
            ZoomProbe {
                last_panel: Vec::new(),
            },
            Registration::new().on_tick(),
        ))
    }

    fn tick(&mut self, _tick: u64) {
        // The hand follows the view's zoom, the original Zoomify
        // behaviour: scaled by the zoom ratio over vanilla's 70.
        render::set_hand_fov(Some(HandFov {
            mode: HandFovMode::Multiply,
            base: 70.0,
        }));
        let panel = panel_lines();
        if panel != self.last_panel {
            let lines: Vec<Line> = panel
                .iter()
                .map(|text| Line {
                    text: text.clone(),
                    color: 0xFFFFFF,
                })
                .collect();
            hud::set_panel(Corner::TopLeft, &lines);
            self.last_panel = panel;
        }
    }
}

horizon_mod_sdk::export_mod!(ZoomProbe);
