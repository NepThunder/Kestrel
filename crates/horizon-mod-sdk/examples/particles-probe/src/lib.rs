//! Prototype probe for particles (API 26): on its key it
//! emits a heart burst over the entity under the crosshair
//! (or over the player, when nothing is targeted), and
//! while the player glides it holds two `end_rod` emitters
//! at the wingtips — self-anchored, in the player's own
//! facing frame — clearing them on landing. Its `/burst`
//! command does the key's work for the test harness, which
//! cannot press keys.
use horizon_mod_sdk::{
    Burst, Emitter, EmitterAnchor, Mod, ParticleParams, Registration, WorldPoint, chat, entity,
    game,
    input::{self, Event},
    particles,
};

const KEY: &str = "burst";

fn point(x: f64, y: f64, z: f64) -> WorldPoint {
    WorldPoint { x, y, z }
}

/// The heart burst, at `at`: seven hearts in a small cloud.
fn hearts(at: WorldPoint) {
    particles::emit(&Burst {
        particle: "minecraft:heart".into(),
        params: ParticleParams {
            color: None,
            to_color: None,
            scale: None,
            block: None,
            target: None,
            duration: None,
        },
        at,
        spread: point(0.3, 0.3, 0.3),
        speed: 0.05,
        count: 7,
    });
}

/// The wingtip emitters: an `end_rod` stream at each side
/// of the gliding player, in its facing frame.
fn wingtips() -> Vec<Emitter> {
    [-1.0, 1.0]
        .into_iter()
        .map(|side| Emitter {
            anchor: EmitterAnchor::SelfPlayer,
            offset: point(side, 1.0, 0.0),
            burst: Burst {
                particle: "minecraft:end_rod".into(),
                params: ParticleParams {
                    color: None,
                    to_color: None,
                    scale: None,
                    block: None,
                    target: None,
                    duration: None,
                },
                at: point(0.0, 0.0, 0.0),
                spread: point(0.05, 0.05, 0.05),
                speed: 0.0,
                count: 1,
            },
            rate: 40.0,
        })
        .collect()
}

struct ParticlesProbe {
    /// The wingtip emitters are declared: set on the first
    /// gliding tick, cleared on landing.
    emitters_on: bool,
}

impl ParticlesProbe {
    /// The burst, over the targeted entity or the player.
    /// Answers who it was over, for the chat and the command.
    fn burst(&self) -> String {
        if let Some(target) = entity::targeted(32.0) {
            let e = &target.entity;
            hearts(point(e.x, e.y + f64::from(e.height), e.z));
            return format!("burst over {}", e.kind);
        }
        if let Some(p) = game::get_player() {
            hearts(point(p.x, p.y + 2.0, p.z));
        }
        "burst over you".into()
    }
}

impl Mod for ParticlesProbe {
    fn init() -> Result<(Self, Registration), String> {
        input::register_key(KEY, "Burst of hearts", "key.keyboard.p")?;
        Ok((
            ParticlesProbe { emitters_on: false },
            Registration::new().on_tick().command("burst"),
        ))
    }

    fn tick(&mut self, _tick: u64) {
        for event in input::poll() {
            if let Event::KeyDown(k) = event
                && k == KEY
            {
                let over = self.burst();
                chat::show(&over);
            }
        }
        let gliding = game::get_self_state().is_some_and(|s| s.gliding);
        if gliding && !self.emitters_on {
            particles::set_emitters(Some(&wingtips()));
            self.emitters_on = true;
        } else if !gliding && self.emitters_on {
            particles::set_emitters(None);
            self.emitters_on = false;
        }
    }

    fn command(&mut self, _name: &str, _args: &str) -> Result<Option<String>, String> {
        Ok(Some(self.burst()))
    }
}

horizon_mod_sdk::export_mod!(ParticlesProbe);
