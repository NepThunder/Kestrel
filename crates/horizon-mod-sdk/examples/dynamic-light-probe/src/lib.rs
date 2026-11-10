//! Prototype probe for dynamic light (API 28): while the
//! player holds a torch or a lantern — in the selected
//! hotbar slot or the off hand — it registers a light
//! source at the player naming that item, so the game
//! sheds the item's own glow at hand height, and clears
//! the source when the item leaves the hand. Its key, or
//! its `/lights` command for the test harness, which
//! cannot press keys, toggles its policy over the game's
//! own gather: held and dropped items and burning
//! entities lit, blazes and glow squids glowing by kind,
//! fire projectiles lit, and water putting fire out.
use horizon_mod_sdk::{
    DynamicLightPolicy, GlowingKind, LightSource, Mod, Registration, SourceAnchor, chat,
    dynamic_light,
    input::{self, Event},
    inventory,
};

const KEY: &str = "lights";

/// The policy the probe stands when toggled on: the
/// district's classes whole, at the game's own numbers.
fn policy() -> DynamicLightPolicy {
    DynamicLightPolicy {
        held_items: true,
        dropped_items: true,
        burning: true,
        glowing: vec![
            GlowingKind {
                kind: "minecraft:blaze".into(),
                level: 15,
            },
            GlowingKind {
                kind: "minecraft:glow_squid".into(),
                level: 15,
            },
        ],
        projectiles: true,
        water_extinguishes: true,
        brightness: 100,
        range: 96,
        max_lights: 16,
    }
}

/// The glowing item in the player's hands: the selected
/// stack's first, then the off hand's (slot 504) — a
/// torch or a lantern, the two a hand carries for light.
fn hand_light() -> Option<String> {
    let inv = inventory::inventory();
    let at = |slot: u32| {
        inv.slots
            .iter()
            .find(|s| s.slot == slot)
            .map(|s| s.id.clone())
    };
    [at(inv.selected), at(504)]
        .into_iter()
        .flatten()
        .find(|id| id == "minecraft:torch" || id == "minecraft:lantern")
}

struct DynamicLightProbe {
    /// The item the standing source names, while one stands.
    lit: Option<String>,
    /// The policy stands.
    policy_on: bool,
}

impl DynamicLightProbe {
    fn toggle_policy(&mut self) {
        self.policy_on = !self.policy_on;
        if self.policy_on {
            dynamic_light::set_policy(Some(&policy()));
        } else {
            dynamic_light::set_policy(None);
        }
    }
}

impl Mod for DynamicLightProbe {
    fn init() -> Result<(Self, Registration), String> {
        input::register_key(KEY, "Toggle dynamic light policy", "key.keyboard.l")?;
        Ok((
            DynamicLightProbe {
                lit: None,
                policy_on: false,
            },
            Registration::new().on_tick().command("lights"),
        ))
    }

    fn tick(&mut self, _tick: u64) {
        for event in input::poll() {
            if let Event::KeyDown(k) = event
                && k == KEY
            {
                self.toggle_policy();
                chat::show(if self.policy_on {
                    "lights policy on"
                } else {
                    "lights policy off"
                });
            }
        }
        let want = hand_light();
        if want != self.lit {
            match &want {
                Some(item) => dynamic_light::set_sources(Some(&[LightSource {
                    anchor: SourceAnchor::SelfPlayer,
                    level: 0,
                    item: Some(item.clone()),
                }])),
                None => dynamic_light::set_sources(None),
            }
            self.lit = want;
        }
    }

    fn command(&mut self, _name: &str, _args: &str) -> Result<Option<String>, String> {
        self.toggle_policy();
        Ok(Some(if self.policy_on {
            "policy on".into()
        } else {
            "policy off".into()
        }))
    }
}

horizon_mod_sdk::export_mod!(DynamicLightProbe);
