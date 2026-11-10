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
//! (API 19); `/posestates`, `/poseoffset`, `/poseclear`, `/posescale`
//! and `/ownbody` capability `render`'s entity poses (API 20);
//! `/acttarget`, `/actuse`, `/actattack`, `/acton`, `/actheld`,
//! `/actslot` and `/actspam` capability `actions`' clicks (API 21);
//! `/scrnstate`, `/scrnbuttons`, `/scrnhighlight`, `/scrntrade`,
//! `/scrndebug` and `/scrnpoll` capability `screen`'s reads,
//! declarations and events (API 22); `/imrules`, `/immarks` and
//! `/imdropped` capability `render`'s item models, slot marks
//! and dropped display (API 23); `/invclick`, `/invswap`,
//! `/invthrow`, `/invdrag`, `/invdragspam`, `/invbad`,
//! `/invspam` and `/invwheel` capability `inventory-actions`'
//! slot clicks, drags and wheel poll (API 24); `/maptiles`,
//! `/mapversions`, `/mapcontext`, `/mapdeath`, `/mappoll`,
//! `/mapshow`, `/mapmini`, `/maphide`, `/mapbad` and
//! `/mappollmap` capability `map`'s tile reads, context,
//! death place, position events, map screen and minimap
//! image (API 25); `/emitburst`, `/emitdust`, `/emitbad`,
//! `/emitblock`, `/setemitter` and `/clearemitter`
//! capability `render`'s particles — bursts and emitters
//! (API 26); `tick` and
//! `frame` do nothing, to measure the cost of a call.
use horizon_mod_sdk::{
    ActionHand, ActionTarget, Anchor, BlockDisplay, BlockTarget, Burst, ChunkPos, ClickKind,
    ComponentMatch, Corner, Decoration, DecorationPlace, DecorationTarget, DisplayContext,
    DragButton, DroppedDisplay, Emitter, EmitterAnchor, Face, FlatDisplay, HandFov, HandFovMode,
    HandTransform, Highlight, MapEvent, MapImage, MapLayer, MapMarker, MapScreen, MinimapImage,
    Mod, ModelMatch, ModelRule, PartOffset, ParticleParams, PoseOffset, PoseTarget, PositionEvent,
    Registration, ScreenButton, ScreenEvent, SoundAdjustment, SoundQuery, SwingAnim, WorldPoint,
    actions, audio, chat, entity_pose, game, hand, input, inventory_actions, item_model, map,
    particles, render, screen, storage,
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
            "posestates",
            "poseoffset",
            "poseclear",
            "posescale",
            "ownbody",
            "acttarget",
            "actuse",
            "actattack",
            "acton",
            "actheld",
            "actslot",
            "actspam",
            "scrnstate",
            "scrnbuttons",
            "scrnhighlight",
            "scrntrade",
            "scrndebug",
            "scrnpoll",
            "imrules",
            "immarks",
            "imdropped",
            "invclick",
            "invswap",
            "invthrow",
            "invdrag",
            "invdragspam",
            "invbad",
            "invspam",
            "invwheel",
            "maptiles",
            "mapversions",
            "mapcontext",
            "mapdeath",
            "mappoll",
            "mapshow",
            "mapmini",
            "maphide",
            "mapbad",
            "mappollmap",
            "emitburst",
            "emitdust",
            "emitbad",
            "emitblock",
            "setemitter",
            "clearemitter",
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
        SoundAdjustment {
            gain: 0.5,
            lowpass: 0.25,
            reverb: 0.75,
        }
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
                let spy = render::get_spyglass_zoom().map_or("none".into(), |f| format!("{f}"));
                Ok(Some(format!("zoom can {} spy {}", render::can_zoom(), spy)))
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
                            "Usage: /handfov <multiply|add|keep> <base>, or /handfov off".into(),
                        );
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
            "posestates" => {
                let lines: Vec<String> = entity_pose::get_pose_states()
                    .iter()
                    .map(|s| {
                        let using = s.using.as_ref().map_or("none".into(), |u| {
                            format!("{}:{}:{}", u.off_hand, u.item, u.ticks)
                        });
                        format!(
                            "{} {} self {} walk {:.2} {:.2} head {:.1} {:.1} swing {:.2} {:?} \
                             off {} arms {:?} {:?} hands {} {} using {} flags {} {} {:.2} {} {} {} {}",
                            s.id,
                            s.kind,
                            s.self_,
                            s.walk_pos,
                            s.walk_speed,
                            s.head_yaw,
                            s.head_pitch,
                            s.swing_progress,
                            s.swing_anim,
                            s.swing_off_hand,
                            s.main_arm_pose,
                            s.off_arm_pose,
                            s.main_hand.as_deref().unwrap_or("none"),
                            s.off_hand.as_deref().unwrap_or("none"),
                            using,
                            s.crouching,
                            s.riding,
                            s.swim_amount,
                            s.gliding,
                            s.sleeping,
                            s.sprinting,
                            s.on_ground
                        )
                    })
                    .collect();
                Ok(Some(format!("poses [{}]", lines.join("; "))))
            }
            "poseoffset" => {
                // `/poseoffset <yaw>`: past every limit the host
                // clamps to on self (a 270° pitch, a NaN pivot,
                // a 2-block pivot), and a plain one on entity 42.
                let yaw: f32 = args.parse().map_err(|_| "Usage: /poseoffset <yaw>")?;
                entity_pose::set_pose_offset(
                    PoseTarget::SelfPlayer,
                    &PoseOffset {
                        parts: vec![
                            PartOffset {
                                part: "right_arm".into(),
                                x: 2.0,
                                y: 0.25,
                                z: f32::NAN,
                                pitch: 270.0,
                                yaw,
                                roll: -30.0,
                            },
                            PartOffset {
                                part: "head".into(),
                                x: 0.0,
                                y: 0.0,
                                z: 0.0,
                                pitch: 0.0,
                                yaw: 0.0,
                                roll: 0.0,
                            },
                        ],
                        hidden: vec!["left_leg".into()],
                    },
                );
                entity_pose::set_pose_offset(
                    PoseTarget::Entity(42),
                    &PoseOffset {
                        parts: vec![PartOffset {
                            part: "body".into(),
                            x: 0.0,
                            y: 0.0,
                            z: 0.0,
                            pitch: 90.0,
                            yaw: 0.0,
                            roll: 0.0,
                        }],
                        hidden: Vec::new(),
                    },
                );
                Ok(Some("set".into()))
            }
            "poseclear" => {
                entity_pose::clear_pose_offset(PoseTarget::SelfPlayer);
                Ok(Some("cleared".into()))
            }
            "posescale" => {
                let scale: f32 = args.parse().map_err(|_| "Usage: /posescale <f32>")?;
                entity_pose::set_pose_scale(PoseTarget::SelfPlayer, scale);
                Ok(Some("scaled".into()))
            }
            "ownbody" => {
                let shown = match args {
                    "on" => true,
                    "off" => false,
                    _ => return Err("Usage: /ownbody <on|off>".into()),
                };
                entity_pose::set_first_person_body(shown);
                Ok(Some(if shown {
                    "body on".into()
                } else {
                    "body off".into()
                }))
            }
            "acttarget" => {
                let text = match actions::target() {
                    None => "target none".into(),
                    Some(ActionTarget::Block(b)) => format!(
                        "target block {} {} {} {} {} {:.2} {:.2} {:.2}",
                        b.block_x,
                        b.block_y,
                        b.block_z,
                        b.block,
                        match b.face {
                            Face::Down => "down",
                            Face::Up => "up",
                            Face::North => "north",
                            Face::South => "south",
                            Face::West => "west",
                            Face::East => "east",
                        },
                        b.cursor_x,
                        b.cursor_y,
                        b.cursor_z
                    ),
                    Some(ActionTarget::Entity(e)) => {
                        format!("target entity {} {} {:.2}", e.id, e.kind, e.distance)
                    }
                };
                Ok(Some(text))
            }
            "actuse" => {
                actions::use_();
                Ok(Some("used".into()))
            }
            "actattack" => {
                actions::attack();
                Ok(Some("attacked".into()))
            }
            "acton" => {
                // A malformed cursor (1.5) errs at call time and
                // queues nothing; the well-formed one queues.
                let mut target = BlockTarget {
                    block_x: 3,
                    block_y: 64,
                    block_z: -2,
                    block: "minecraft:wheat[age=7]".into(),
                    face: Face::Up,
                    cursor_x: 1.5,
                    cursor_y: 1.0,
                    cursor_z: 0.5,
                };
                let bad = actions::use_on_block(&target, ActionHand::Main)
                    .expect_err("a cursor of 1.5 is outside the block");
                target.cursor_x = 0.5;
                actions::use_on_block(&target, ActionHand::Main)?;
                Ok(Some(format!("bad {bad}; queued")))
            }
            "actheld" => {
                match args {
                    "attack" => actions::set_held(true, false),
                    "off" => actions::set_held(false, false),
                    _ => return Err("Usage: /actheld <attack|off>".into()),
                }
                Ok(Some("held set".into()))
            }
            "actslot" => {
                // Slot 9 errs at call time; slot 3 queues.
                let bad = actions::select_slot(9).expect_err("slot 9 is outside 0 to 8");
                actions::select_slot(3)?;
                Ok(Some(format!("bad {bad}; queued")))
            }
            "actspam" => {
                // Ten uses in one call: the host keeps eight.
                for _ in 0..10 {
                    actions::use_();
                }
                Ok(Some("spammed".into()))
            }
            "scrnstate" => {
                let s = screen::state();
                let (x, y, w, h) = s.rect;
                let trades = match screen::trades() {
                    None => "trades - selected -".to_string(),
                    Some(t) => {
                        let first = t.offers.first().map_or("-".to_string(), |o| {
                            format!(
                                "first {} {} -> {} avail {}",
                                o.cost_a.item, o.cost_a.count, o.result.item, o.available
                            )
                        });
                        format!(
                            "trades {} selected {} {first}",
                            t.offers.len(),
                            t.selected.map_or("-".into(), |i| i.to_string())
                        )
                    }
                };
                Ok(Some(format!(
                    "screen {} menu {} title {} rect {:.1} {:.1} {:.1} {:.1} hovered {} \
                     debug {} {trades}",
                    s.id,
                    s.menu.as_deref().unwrap_or("-"),
                    s.title.as_deref().unwrap_or("-"),
                    x,
                    y,
                    w,
                    h,
                    s.hovered_slot.map_or("-".into(), |i| i.to_string()),
                    s.debug_open,
                )))
            }
            "scrnbuttons" => {
                // Seventeen declarations: the host stores sixteen.
                let buttons: Vec<ScreenButton> = (1..=17)
                    .map(|id| ScreenButton {
                        id,
                        screen: "inventory".into(),
                        menu: None,
                        anchor: Anchor::ContainerHead,
                        label: format!("B{id}"),
                        icon: None,
                    })
                    .collect();
                screen::set_buttons(&buttons);
                Ok(Some("declared".into()))
            }
            "scrnhighlight" => {
                // 513 marks: the host keeps 512.
                let marks: Vec<Highlight> = (0..=512)
                    .map(|slot| Highlight {
                        slot,
                        color: 0xFF0000,
                    })
                    .collect();
                screen::set_highlights(&marks);
                Ok(Some("marked".into()))
            }
            "scrntrade" => {
                // No merchant open: the call errs, and the error
                // is the command's. One open: trade 0 is queued.
                screen::select_trade(0)?;
                Ok(Some("chose".into()))
            }
            "scrndebug" => {
                let claimed = match args {
                    "on" => true,
                    "off" => false,
                    _ => return Err("Usage: /scrndebug <on|off>".into()),
                };
                screen::claim_debug(claimed);
                Ok(Some(if claimed {
                    "debug on".into()
                } else {
                    "debug off".into()
                }))
            }
            "scrnpoll" => {
                let parts: Vec<String> = screen::poll()
                    .iter()
                    .map(|e| match e {
                        ScreenEvent::Button(id) => format!("button {id}"),
                        ScreenEvent::MiddleClick(Some(slot)) => format!("middle {slot}"),
                        ScreenEvent::MiddleClick(None) => "middle none".into(),
                    })
                    .collect();
                Ok(Some(format!("events [{}]", parts.join("; "))))
            }
            "imrules" => {
                // 65 declared rules: the host keeps 64. The last
                // one (on `minecraft:dropped_item`) is the dropped
                // sixty-fifth; the stick's five matches are the
                // fifth dropped; the stone's value never parses.
                let matched = |item: &str, name: &str, value: &str| ModelRule {
                    match_: ModelMatch {
                        item: item.into(),
                        components: vec![ComponentMatch {
                            name: name.into(),
                            value: value.into(),
                        }],
                    },
                    contexts: vec![DisplayContext::Gui],
                    model: format!("test:{item}"),
                };
                let mut rules = vec![
                    ModelRule {
                        match_: ModelMatch {
                            item: "minecraft:apple".into(),
                            components: vec![ComponentMatch {
                                name: "minecraft:rarity".into(),
                                value: "rare".into(),
                            }],
                        },
                        contexts: vec![DisplayContext::Gui],
                        model: "test:rare_apple".into(),
                    },
                    ModelRule {
                        match_: ModelMatch {
                            item: "minecraft:apple".into(),
                            components: vec![ComponentMatch {
                                name: "minecraft:custom_name".into(),
                                value: "{text:\"Bob\"}".into(),
                            }],
                        },
                        contexts: vec![DisplayContext::Gui, DisplayContext::FirstPerson],
                        model: "test:bob_apple".into(),
                    },
                    ModelRule {
                        match_: ModelMatch {
                            item: "minecraft:stone".into(),
                            components: vec![ComponentMatch {
                                name: "minecraft:custom_data".into(),
                                value: "{oops".into(),
                            }],
                        },
                        contexts: vec![DisplayContext::Gui],
                        model: "test:broken".into(),
                    },
                    ModelRule {
                        match_: ModelMatch {
                            item: "minecraft:stick".into(),
                            components: ["a", "b", "c", "d", "e"]
                                .iter()
                                .enumerate()
                                .map(|(i, name)| ComponentMatch {
                                    name: (*name).into(),
                                    value: format!("{}", i + 1),
                                })
                                .collect(),
                        },
                        contexts: vec![DisplayContext::Gui],
                        model: "test:stick".into(),
                    },
                ];
                for i in 0..60 {
                    rules.push(matched(
                        &format!("minecraft:filler_{i}"),
                        "minecraft:rarity",
                        "common",
                    ));
                }
                rules.push(matched(
                    "minecraft:dropped_item",
                    "minecraft:rarity",
                    "common",
                ));
                item_model::set_model_rules(&rules);
                Ok(Some("rules set".into()))
            }
            "immarks" => {
                // 257 declared marks: the host keeps 256. The
                // last one (slot 9999) is the dropped 257th.
                let mut marks = vec![
                    Decoration {
                        target: DecorationTarget::Slot(5),
                        places: vec![DecorationPlace::Hotbar, DecorationPlace::Screens],
                        fill: Some(0xff0000),
                        border: None,
                        star: false,
                    },
                    Decoration {
                        target: DecorationTarget::Stack(ModelMatch {
                            item: "minecraft:apple".into(),
                            components: vec![ComponentMatch {
                                name: "minecraft:rarity".into(),
                                value: "rare".into(),
                            }],
                        }),
                        places: vec![DecorationPlace::Screens],
                        fill: None,
                        border: Some(0x00ff00),
                        star: true,
                    },
                ];
                for i in 0..254u32 {
                    marks.push(Decoration {
                        target: DecorationTarget::Slot(1000 + i),
                        places: vec![DecorationPlace::Screens],
                        fill: Some(0x0000ff),
                        border: None,
                        star: false,
                    });
                }
                marks.push(Decoration {
                    target: DecorationTarget::Slot(9999),
                    places: vec![DecorationPlace::Screens],
                    fill: Some(0xffffff),
                    border: None,
                    star: false,
                });
                item_model::set_decorations(&marks);
                Ok(Some("marks set".into()))
            }
            "imdropped" => {
                let display = match args {
                    "sprite" => DroppedDisplay {
                        flat: FlatDisplay::Sprite,
                        blocks: BlockDisplay::Vanilla,
                        shadows: true,
                    },
                    "full" => DroppedDisplay {
                        flat: FlatDisplay::Sprite,
                        blocks: BlockDisplay::Billboard,
                        shadows: false,
                    },
                    "off" => DroppedDisplay {
                        flat: FlatDisplay::Vanilla,
                        blocks: BlockDisplay::Vanilla,
                        shadows: true,
                    },
                    _ => return Err("Usage: /imdropped <sprite|full|off>".into()),
                };
                item_model::set_dropped_display(display);
                Ok(Some("dropped set".into()))
            }
            "invclick" => {
                inventory_actions::click(Some(9), 0, ClickKind::Pickup)?;
                Ok(Some("clicked".into()))
            }
            "invswap" => {
                // Button 9 errs at call time; button 3 queues.
                let bad = inventory_actions::click(Some(9), 9, ClickKind::Swap)
                    .expect_err("swap takes a hotbar slot");
                inventory_actions::click(Some(9), 3, ClickKind::Swap)?;
                Ok(Some(format!("bad {bad}; queued")))
            }
            "invthrow" => {
                inventory_actions::click(Some(9), 0, ClickKind::Throw)?;
                Ok(Some("threw".into()))
            }
            "invdrag" => {
                inventory_actions::drag(DragButton::Right, &[9, 10, 11])?;
                Ok(Some("dragged".into()))
            }
            "invdragspam" => {
                // Five drags in one call: each weighs 8, so the
                // host keeps four (32) and drops the fifth.
                for _ in 0..5 {
                    inventory_actions::drag(DragButton::Right, &[9])?;
                }
                Ok(Some("drag spammed".into()))
            }
            "invbad" => {
                // All malformed at call time: a slot past 999, an
                // empty drag, a drag past 64 slots. Nothing queues.
                let slot = inventory_actions::click(Some(1000), 0, ClickKind::Pickup)
                    .expect_err("slot 1000 is past 999");
                let empty = inventory_actions::drag(DragButton::Right, &[]).expect_err("no slots");
                let many = inventory_actions::drag(DragButton::Left, &vec![9; 65])
                    .expect_err("65 slots is past 64");
                Ok(Some(format!("bad {slot}; {empty}; {many}")))
            }
            "invspam" => {
                // 33 clicks in one call: the host keeps 32.
                for _ in 0..33 {
                    inventory_actions::click(Some(9), 0, ClickKind::Pickup)?;
                }
                Ok(Some("spammed".into()))
            }
            "invwheel" => {
                let parts: Vec<String> = inventory_actions::poll_wheel()
                    .iter()
                    .map(|e| match e.slot {
                        Some(slot) => format!("{slot} {}", e.steps),
                        None => format!("none {}", e.steps),
                    })
                    .collect();
                Ok(Some(format!("wheel [{}]", parts.join("; "))))
            }
            "maptiles" => {
                // A 65-chunk ask first: over the cap, no tiles.
                let wide: Vec<ChunkPos> = (0..65).map(|i| ChunkPos { x: i, z: 0 }).collect();
                let over = map::tiles(&wide).len();
                let ask = [
                    ChunkPos { x: 0, z: 0 },
                    ChunkPos { x: 1, z: 0 },
                    ChunkPos { x: 0, z: 1 },
                    ChunkPos { x: 5, z: 5 },
                ];
                let tiles = map::tiles(&ask);
                let mut out = format!("tiles {} over {over}", tiles.len());
                for t in &tiles {
                    let (layer, cut) = match t.layer {
                        MapLayer::Surface => ("surface", "-".to_string()),
                        MapLayer::Cave => ("cave", t.cut_y.unwrap_or(0).to_string()),
                    };
                    let sum: u32 = t.colors.iter().map(|c| u32::from(*c)).sum();
                    out += &format!(
                        "; {},{} {layer} {cut} v{} c{sum} h{}/{} d{}",
                        t.chunk_x, t.chunk_z, t.version, t.heights[0], t.heights[255], t.depths[0]
                    );
                }
                Ok(Some(out))
            }
            "mapversions" => {
                // A 257-chunk ask first: over the cap, nothing.
                let wide: Vec<ChunkPos> = (0..257).map(|i| ChunkPos { x: i, z: 0 }).collect();
                let over = map::tile_versions(&wide).len();
                let ask = [
                    ChunkPos { x: 0, z: 0 },
                    ChunkPos { x: 1, z: 0 },
                    ChunkPos { x: 0, z: 1 },
                ];
                let versions = map::tile_versions(&ask);
                let mut out = format!("versions {} over {over}", versions.len());
                for v in &versions {
                    out += &format!("; {},{} v{}", v.chunk_x, v.chunk_z, v.version);
                }
                Ok(Some(out))
            }
            "mapcontext" => match map::context() {
                Some(c) => Ok(Some(format!("context {} {}", c.dimension, c.world))),
                None => Ok(Some("context none".into())),
            },
            "mapdeath" => match map::last_death() {
                Some(p) => Ok(Some(format!(
                    "death {} {} {} {}",
                    p.dimension, p.x, p.y, p.z
                ))),
                None => Ok(Some("death none".into())),
            },
            "mappoll" => {
                let parts: Vec<String> = map::poll_events()
                    .iter()
                    .map(|e| match e {
                        PositionEvent::Died(p) => {
                            format!("died {} {} {} {}", p.dimension, p.x, p.y, p.z)
                        }
                        PositionEvent::Respawned(p) => {
                            format!("respawned {} {} {} {}", p.dimension, p.x, p.y, p.z)
                        }
                    })
                    .collect();
                Ok(Some(format!("events [{}]", parts.join("; "))))
            }
            "mapshow" => {
                // 129 markers: the host keeps 128, and the
                // first's 100-character label is cut to 64.
                let mut markers: Vec<MapMarker> = (0..129)
                    .map(|i| MapMarker {
                        x: i,
                        z: 0,
                        color: 0xFF00_FF00,
                        label: String::new(),
                    })
                    .collect();
                markers[0] = MapMarker {
                    x: 100,
                    z: -200,
                    color: 0xFFFF_0000,
                    label: "m".repeat(100),
                };
                map::set_map_screen(Some(&MapScreen {
                    image: MapImage {
                        center_x: 100,
                        center_z: -200,
                        blocks_per_pixel: 2.0,
                        width: 4,
                        height: 3,
                        palette: vec![0xFF11_2233, 0xFF44_5566],
                        // Index 2 is past the palette: clear.
                        pixels: vec![0, 1, 2, 0, 1, 0, 0, 1, 2, 1, 0, 1],
                    },
                    markers,
                }));
                Ok(Some("shown".into()))
            }
            "mapmini" => {
                map::set_minimap_image(Some(&MinimapImage {
                    corner: Corner::TopLeft,
                    size: 100.0,
                    round: true,
                    image: MapImage {
                        center_x: 0,
                        center_z: 0,
                        blocks_per_pixel: 1.0,
                        width: 2,
                        height: 2,
                        palette: vec![0xFF00_00FF],
                        pixels: vec![0, 0, 0, 0],
                    },
                }));
                Ok(Some("mini".into()))
            }
            "maphide" => {
                map::set_map_screen(None);
                map::set_minimap_image(None);
                Ok(Some("hidden".into()))
            }
            "mapbad" => {
                // A 4x3 image with 5 pixels: malformed — the
                // host drops it and sets nothing.
                let image = MapImage {
                    center_x: 0,
                    center_z: 0,
                    blocks_per_pixel: 1.0,
                    width: 4,
                    height: 3,
                    palette: vec![0xFF00_0000],
                    pixels: vec![0; 5],
                };
                map::set_map_screen(Some(&MapScreen {
                    image: image.clone(),
                    markers: Vec::new(),
                }));
                map::set_minimap_image(Some(&MinimapImage {
                    corner: Corner::TopLeft,
                    size: 100.0,
                    round: false,
                    image,
                }));
                Ok(Some("bad".into()))
            }
            "mappollmap" => {
                let parts: Vec<String> = map::poll_map()
                    .iter()
                    .map(|e| match e {
                        MapEvent::Clicked(c) => format!("clicked {} {} b{}", c.x, c.z, c.button),
                        MapEvent::View(v) => {
                            format!("view {} {} {}", v.center_x, v.center_z, v.blocks_per_pixel)
                        }
                        MapEvent::Closed => "closed".into(),
                    })
                    .collect();
                Ok(Some(format!("map [{}]", parts.join("; "))))
            }
            "emitburst" => {
                let p = game::get_player().ok_or("no player")?;
                let at = point(p.x, p.y, p.z);
                particles::emit(&burst("minecraft:heart", at, point(0.5, 0.5, 0.5), 0.1, 12));
                // A count of 0: the directional single,
                // straight up.
                particles::emit(&burst("minecraft:flame", at, point(0.0, 1.0, 0.0), 0.5, 0));
                Ok(Some("burst".into()))
            }
            "emitdust" => {
                let p = game::get_player().ok_or("no player")?;
                // A count of 300: the host cuts it to 256.
                let mut b = burst(
                    "dust",
                    point(p.x, p.y, p.z),
                    point(0.3, 0.3, 0.3),
                    0.05,
                    300,
                );
                b.params.color = Some(0xFFAA_0000);
                b.params.scale = Some(2.0);
                particles::emit(&b);
                Ok(Some("dust".into()))
            }
            "emitbad" => {
                let p = game::get_player().ok_or("no player")?;
                let at = point(p.x, p.y, p.z);
                // A non-finite spread: the whole burst is
                // refused at the door and never drains.
                particles::emit(&burst(
                    "minecraft:flame",
                    at,
                    point(f64::NAN, 0.0, 0.0),
                    0.1,
                    4,
                ));
                // An unknown type and a pack's type both
                // drain as asked: what they draw is the
                // game's side.
                particles::emit(&burst(
                    "minecraft:nosuchthing",
                    at,
                    point(0.0, 0.0, 0.0),
                    0.0,
                    4,
                ));
                particles::emit(&burst("test:spark", at, point(0.0, 0.0, 0.0), 0.0, 2));
                Ok(Some("bad".into()))
            }
            "emitblock" => {
                let p = game::get_player().ok_or("no player")?;
                let at = point(p.x, p.y, p.z);
                let mut with = burst("block", at, point(0.2, 0.2, 0.2), 0.0, 6);
                with.params.block = Some("minecraft:stone".into());
                particles::emit(&with);
                // Without its block the burst still drains:
                // drawing nothing for it is the game's side.
                particles::emit(&burst("block", at, point(0.2, 0.2, 0.2), 0.0, 6));
                Ok(Some("blocks".into()))
            }
            "setemitter" => {
                // Nine declared: the host keeps eight. The
                // second's rate (500) and offset (20) sit
                // past their clamps and are cut at the door.
                let mut emitters = vec![Emitter {
                    anchor: EmitterAnchor::SelfPlayer,
                    offset: point(0.0, 1.0, 0.0),
                    burst: burst(
                        "minecraft:end_rod",
                        point(0.0, 0.0, 0.0),
                        point(0.0, 0.0, 0.0),
                        0.0,
                        1,
                    ),
                    rate: 7.0,
                }];
                emitters.push(Emitter {
                    anchor: EmitterAnchor::Position(point(1.0, 2.0, 3.0)),
                    offset: point(20.0, 0.0, 0.0),
                    burst: burst(
                        "minecraft:flame",
                        point(0.0, 0.0, 0.0),
                        point(0.0, 0.0, 0.0),
                        0.0,
                        1,
                    ),
                    rate: 500.0,
                });
                for i in 0..7 {
                    emitters.push(Emitter {
                        anchor: EmitterAnchor::Position(point(f64::from(i), 64.0, 0.0)),
                        offset: point(0.0, 0.0, 0.0),
                        burst: burst(
                            "minecraft:flame",
                            point(0.0, 0.0, 0.0),
                            point(0.0, 0.0, 0.0),
                            0.0,
                            1,
                        ),
                        rate: 20.0,
                    });
                }
                particles::set_emitters(Some(&emitters));
                Ok(Some("emitters".into()))
            }
            "clearemitter" => {
                particles::set_emitters(None);
                Ok(Some("cleared".into()))
            }
            _ => {
                chat::show(&format!("hello {args} after {} ticks", self.ticks));
                Ok(None)
            }
        }
    }
}

/// A world point (capability `particles`, API 26).
fn point(x: f64, y: f64, z: f64) -> WorldPoint {
    WorldPoint { x, y, z }
}

/// A burst with no parameters (capability `particles`,
/// API 26).
fn burst(particle: &str, at: WorldPoint, spread: WorldPoint, speed: f64, count: u32) -> Burst {
    Burst {
        particle: particle.into(),
        params: ParticleParams {
            color: None,
            to_color: None,
            scale: None,
            block: None,
            target: None,
            duration: None,
        },
        at,
        spread,
        speed,
        count,
    }
}

horizon_mod_sdk::export_mod!(Guest);
