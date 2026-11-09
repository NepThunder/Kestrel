//! A mod for the runtime's tests: `/spin` loops forever, `/panic` panics,
//! `/grow` asks for a gigabyte, `/hello` chats, `/save` and `/load` exercise
//! capability `storage` on a fixed key, `/save_key <key> <text>` exercises
//! it on an arbitrary key (for the quota and key-count limits); `/handstate`,
//! `/handset` and `/handclear` exercise capability `render`'s hand (API 16);
//! `/swinganim` and `/swingscale` its swing terms (API 17); `tick` and
//! `frame` do nothing, to measure the cost of a call.
use horizon_mod_sdk::{
    HandTransform, Mod, Registration, SoundAdjustment, SoundQuery, SwingAnim, audio, chat, hand,
    storage,
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
            _ => {
                chat::show(&format!("hello {args} after {} ticks", self.ticks));
                Ok(None)
            }
        }
    }
}

horizon_mod_sdk::export_mod!(Guest);
