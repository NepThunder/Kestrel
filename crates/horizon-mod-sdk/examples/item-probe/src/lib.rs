//! Prototype probe for item models (API 23): at load it
//! declares an apple's display as the golden apple's model
//! definition (`item-model.set-model-rules`) — the shape a
//! frame-animated food declares per food, the definition
//! standing in for its own content pack's frames — a mark on
//! every rare stack and the hotbar's first slot
//! (`item-model.set-decorations`), and dropped flat items
//! lying as sprites (`item-model.set-dropped-display`). A HUD
//! panel reports the declarations, in the shape the harness
//! asserts.
use horizon_mod_sdk::{
    BlockDisplay, ComponentMatch, Decoration, DecorationPlace, DecorationTarget, DisplayContext,
    DroppedDisplay, FlatDisplay, Mod, ModelMatch, ModelRule, Registration,
    hud::{self, Corner, Line},
    item_model,
};

struct ItemProbe;

impl Mod for ItemProbe {
    fn init() -> Result<(Self, Registration), String> {
        item_model::set_model_rules(&[ModelRule {
            match_: ModelMatch {
                item: "minecraft:apple".into(),
                components: Vec::new(),
            },
            contexts: vec![
                DisplayContext::FirstPerson,
                DisplayContext::ThirdPerson,
                DisplayContext::Gui,
            ],
            model: "minecraft:golden_apple".into(),
        }]);
        item_model::set_decorations(&[
            Decoration {
                target: DecorationTarget::Stack(ModelMatch {
                    item: String::new(),
                    components: vec![ComponentMatch {
                        name: "minecraft:rarity".into(),
                        value: "rare".into(),
                    }],
                }),
                places: vec![DecorationPlace::Screens, DecorationPlace::Hotbar],
                fill: None,
                border: Some(0x5555ff),
                star: false,
            },
            Decoration {
                target: DecorationTarget::Slot(0),
                places: vec![DecorationPlace::Hotbar],
                fill: Some(0xffaa00),
                border: None,
                star: false,
            },
        ]);
        item_model::set_dropped_display(DroppedDisplay {
            flat: FlatDisplay::Sprite,
            blocks: BlockDisplay::Vanilla,
            shadows: true,
        });
        Ok((ItemProbe, Registration::new().on_tick()))
    }

    fn tick(&mut self, _tick: u64) {
        hud::set_panel(
            Corner::TopLeft,
            &[
                Line {
                    text: "apple -> minecraft:golden_apple".into(),
                    color: 0xFFFFFF,
                },
                Line {
                    text: "dropped flat sprite".into(),
                    color: 0xFFFFFF,
                },
            ],
        );
    }
}

horizon_mod_sdk::export_mod!(ItemProbe);
