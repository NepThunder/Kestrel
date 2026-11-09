//! A mod for the runtime's tests: `/spin` loops forever, `/panic` panics,
//! `/grow` asks for a gigabyte, `/hello` chats, `/save` and `/load` exercise
//! capability `storage` on a fixed key, `/save_key <key> <text>` exercises
//! it on an arbitrary key (for the quota and key-count limits); `/handstate`,
//! `/handset` and `/handclear` exercise capability `render`'s hand (API 16);
//! `/swinganim` and `/swingscale` its swing terms (API 17); `/effects`,
//! `/selfstate` and `/toggles` capability `game-read`'s self reads and the
//! movement toggle write (API 18); `/zoomstate`, `/hidehud`, `/handfov`
//! and `/fpscap` capability `render`'s zoom truth, HUD hide, hand FOV
//! and frame-rate cap, and `/idle` capability `input`'s idle clock
//! (API 19); `tick` and
//! `frame` do nothing, to measure the cost of a call.
use horizon_mod_sdk::{
    HandFov, HandFovMode, HandTransform, Mod, Registration, SoundAdjustment, SoundQuery, SwingAnim,
    audio, chat, game, hand, input, render, storage,
};

struct Guest {
    ticks: u64,
}

impl Mod for Guest {
    fn init() -> Result<(Self, Registration), String> {
        let hooks = [
            "spin",
            "panic",
            "grow",
            "hello",
            "save",
            "load",
            "save_key",
            "handstate",
            "handset",
            "handclear",
            "swinganim",
            "swingscale",
            "effects",
            "selfstate",
            "toggles",
            "zoomstate",
            "hidehud",
            "handfov",
            "fpscap",
            "idle",
        ]
        .into_iter()
        .fold(Registration::new(), Registration::command)
        .on_tick()
        .on_frame();
        Ok((Guest { ticks: 0 }, hooks))
    }
    fn tick(&mut self, _tick: u64) {
        self.ticks += 1;
    }
    /// Capability `audio` (API 15): a fixed answer, and the shared
    /// reverb's character set on the way through.
    fn sound(&mut self, _query: &SoundQuery) -> SoundAdjustment {
        audio::set_environment(0.3, 2.5);
        SoundAdjustment { gain: 0.5, lowpass: 0.25, reverb: 0.75 }
    }
    fn command(&mut self, name: &str, args: &str) -> Result<Option<String>, String> {
        match name {
            "spin" => {
                let mut n = 0u64;
                while std::hint::black_box(n) != u64::MAX {
                    n = n.wrapping_add(2);
                }
                Ok(None)
            }
            "panic" => panic!("asked to"),
            "grow" => {
                let big = std::hint::black_box(vec![1u8; 1 << 30]);
                Ok(Some(format!("{}", big.len())))
            }
            "save" => {
                storage::set("note", args.as_bytes())?;
                Ok(Some("saved".into()))
            }
            "load" => {
                let text =
                    storage::get("note").map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
                chat::show(text.as_deref().unwrap_or("(empty)"));
                Ok(None)
            }
            "save_key" => {
                let (key, text) = args
                    .split_once(' ')
                    .ok_or("Usage: /save_key <key> <text>")?;
                storage::set(key, text.as_bytes())?;
                Ok(Some("saved".into()))
            }
            "handstate" => {
                let s = hand::get_state();
                Ok(Some(format!(
                    "swing {} {:.2} {:?} equip {:.2} look {:.1} {:.1} d {:.1} {:.1} \
                     bob {:.1} {:.1} item {} using {} two {}",
                    s.swinging,
                    s.swing_progress,
                    s.swing_kind,
                    s.equip_progress,
                    s.yaw,
                    s.pitch,
                    s.look_delta_yaw,
                    s.look_delta_pitch,
                    s.bob_phase,
                    s.bob_amount,
                    s.held_item.as_deref().unwrap_or("none"),
                    s.using_item,
                    s.two_handed
                )))
            }
            "handset" => {
                // Past every limit the host clamps to, and a NaN.
                hand::set_offset(HandTransform {
                    x: 2.5,
                    y: -0.25,
                    z: 0.5,
                    pitch: 120.0,
                    yaw: f32::NAN,
                    roll: -30.0,
                });
                Ok(Some("set".into()))
            }
            "handclear" => {
                hand::clear_offset();
                Ok(Some("cleared".into()))
            }
            "swinganim" => {
                let anim = match hand::get_swing_anim() {
                    SwingAnim::None => "none",
                    SwingAnim::Whack => "whack",
                    SwingAnim::Stab => "stab",
                };
                Ok(Some(format!("anim {anim}")))
            }
            "swingscale" => {
                let scale: f32 = args.parse().map_err(|_| "Usage: /swingscale <f32>")?;
                hand::set_swing_scale(scale);
                Ok(Some("scaled".into()))
            }
            "effects" => {
                let list: Vec<String> = game::get_effects()
                    .iter()
                    .map(|e| {
                        format!(
                            "{} amp {} {} ticks ambient {} particles {} icon {}",
                            e.id, e.amplifier, e.duration, e.ambient, e.show_particles, e.show_icon
                        )
                    })
                    .collect();
                Ok(Some(format!("effects [{}]", list.join("; "))))
            }
            "selfstate" => {
                let Some(s) = game::get_self_state() else {
                    return Ok(Some("self none".into()));
                };
                let mount = s.mount.as_ref().map_or("none".into(), |m| {
                    format!("{} {} {:.1}/{:.1}", m.id, m.kind, m.health, m.max_health)
                });
                Ok(Some(format!(
                    "self sprint {} sneak {} fly {} glide {} mount {} toggles {} {}",
                    s.sprinting,
                    s.sneaking,
                    s.flying,
                    s.gliding,
                    mount,
                    s.sprint_toggled,
                    s.sneak_toggled
                )))
            }
            "toggles" => {
                // `/toggles on off`: each half is on, off or keep.
                let half = |word: Option<&str>| -> Result<Option<bool>, String> {
                    match word {
                        Some("on") => Ok(Some(true)),
                        Some("off") => Ok(Some(false)),
                        Some("keep") => Ok(None),
                        _ => Err("Usage: /toggles <on|off|keep> <on|off|keep>".into()),
                    }
                };
                let mut words = args.split_whitespace();
                let sprint = half(words.next())?;
                let sneak = half(words.next())?;
                game::set_movement_toggles(sprint, sneak);
                Ok(Some("toggled".into()))
            }
            "zoomstate" => {
                let spy = render::get_spyglass_zoom()
                    .map_or("none".into(), |f| format!("{f}"));
                Ok(Some(format!(
                    "zoom can {} spy {}",
                    render::can_zoom(),
                    spy
                )))
            }
            "hidehud" => {
                let hidden = match args {
                    "on" => true,
                    "off" => false,
                    _ => return Err("Usage: /hidehud <on|off>".into()),
                };
                render::set_hide_hud(hidden);
                Ok(Some(if hidden {
                    "hud hidden".into()
                } else {
                    "hud shown".into()
                }))
            }
            "handfov" => {
                // `/handfov off`, or `/handfov <multiply|add|keep> <base>`.
                if args == "off" {
                    render::set_hand_fov(None);
                    return Ok(Some("handfov off".into()));
                }
                let mut words = args.split_whitespace();
                let mode = match words.next() {
                    Some("multiply") => HandFovMode::Multiply,
                    Some("add") => HandFovMode::Add,
                    Some("keep") => HandFovMode::Keep,
                    _ => {
                        return Err(
                            "Usage: /handfov <multiply|add|keep> <base>, or /handfov off".into()
                        )
                    }
                };
                let base: f32 = words
                    .next()
                    .and_then(|w| w.parse().ok())
                    .ok_or("Usage: /handfov <multiply|add|keep> <base>, or /handfov off")?;
                render::set_hand_fov(Some(HandFov { mode, base }));
                Ok(Some("handfov set".into()))
            }
            "fpscap" => {
                // `/fpscap off`, or `/fpscap <fps>`.
                if args == "off" {
                    render::set_fps_cap(None);
                    return Ok(Some("cap off".into()));
                }
                let fps: u32 = args
                    .parse()
                    .map_err(|_| "Usage: /fpscap <fps>, or /fpscap off")?;
                render::set_fps_cap(Some(fps));
                Ok(Some("capped".into()))
            }
            "idle" => Ok(Some(format!("idle {}", input::last_input_ms()))),
            _ => {
                chat::show(&format!("hello {args} after {} ticks", self.ticks));
                Ok(None)
            }
        }
    }
}

horizon_mod_sdk::export_mod!(Guest);
