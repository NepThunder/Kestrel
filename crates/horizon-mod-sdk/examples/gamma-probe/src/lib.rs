//! Prototype probe for mod gamma and fog (`docs/mods.md`, "Brightness and
//! fog"): each tick it shows the effective gamma and the distance-fog flag
//! in chat when they change; `/gamma` prints the effective gamma,
//! `/gamma <value>` sets this mod's gamma (clamped host-side to
//! 0.0..=10.0), `/gamma-nan` sets a non-finite gamma (reads back as 1.0),
//! and `/fog-off` / `/fog-on` turn distance fog off and back on. Reloading
//! the mod restores the player's own Brightness and Fog settings.
use horizon_mod_sdk::{Mod, Registration, chat, render};

struct GammaProbe {
    last: String,
}

fn line() -> String {
    format!(
        "[Gamma Probe] gamma={} fog={}",
        render::get_gamma(),
        if render::get_fog() { "on" } else { "off" }
    )
}

impl Mod for GammaProbe {
    fn init() -> Result<(Self, Registration), String> {
        let hooks = Registration::new()
            .command("gamma")
            .command("gamma-nan")
            .command("fog-off")
            .command("fog-on")
            .on_tick();
        Ok((
            GammaProbe {
                last: String::new(),
            },
            hooks,
        ))
    }

    fn tick(&mut self, _tick: u64) {
        let now = line();
        if now != self.last {
            self.last = now.clone();
            chat::show(&now);
        }
    }

    fn command(&mut self, name: &str, args: &str) -> Result<Option<String>, String> {
        match name {
            "gamma" => {
                let args = args.trim();
                if !args.is_empty() {
                    let value: f32 = args
                        .parse()
                        .map_err(|_| "gamma: give a number".to_string())?;
                    render::set_gamma(value);
                }
                Ok(Some(format!("gamma {}", render::get_gamma())))
            }
            "gamma-nan" => {
                render::set_gamma(f32::NAN);
                Ok(Some(format!("gamma {}", render::get_gamma())))
            }
            "fog-off" => {
                render::set_fog(false);
                Ok(Some(format!(
                    "fog {}",
                    if render::get_fog() { "on" } else { "off" }
                )))
            }
            "fog-on" => {
                render::set_fog(true);
                Ok(Some(format!(
                    "fog {}",
                    if render::get_fog() { "on" } else { "off" }
                )))
            }
            _ => Err("This command does nothing".into()),
        }
    }
}

horizon_mod_sdk::export_mod!(GammaProbe);
