//! Prototype probe for screen (API 22): each tick it shows the
//! open screen (`screen.state`) in a HUD panel — its id, and
//! the open menu's type — and it declares two buttons in
//! `init` (`screen.set-buttons`): one in the pause menu's
//! column, one beside an open container's title. The game
//! places them; what the player does with them comes back
//! through `screen.poll`, and the last event shows under the
//! screen's line. A middle click over the open screen is
//! polled the same way, observed and never consumed.
use horizon_mod_sdk::{
    Anchor, Mod, Registration, ScreenButton, ScreenEvent,
    hud::{self, Corner, Line},
    screen,
};

struct ScreenProbe {
    last_panel: Vec<String>,
    last_event: Option<String>,
}

impl Mod for ScreenProbe {
    fn init() -> Result<(Self, Registration), String> {
        screen::set_buttons(&[
            ScreenButton {
                id: 1,
                screen: "paused".into(),
                menu: None,
                anchor: Anchor::MenuColumn,
                label: "Screen Probe".into(),
                icon: None,
            },
            ScreenButton {
                id: 2,
                screen: "inventory".into(),
                menu: None,
                anchor: Anchor::ContainerHead,
                label: "Probe".into(),
                icon: None,
            },
        ]);
        Ok((
            ScreenProbe {
                last_panel: Vec::new(),
                last_event: None,
            },
            Registration::new().on_tick(),
        ))
    }

    fn tick(&mut self, _tick: u64) {
        for event in screen::poll() {
            self.last_event = Some(match event {
                ScreenEvent::Button(id) => format!("pressed {id}"),
                ScreenEvent::MiddleClick(Some(slot)) => format!("middle {slot}"),
                ScreenEvent::MiddleClick(None) => "middle none".into(),
            });
        }
        let state = screen::state();
        let mut panel = vec![match &state.menu {
            Some(menu) => format!("screen {} {}", state.id, menu),
            None => format!("screen {}", state.id),
        }];
        if let Some(event) = &self.last_event {
            panel.push(event.clone());
        }
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

horizon_mod_sdk::export_mod!(ScreenProbe);
