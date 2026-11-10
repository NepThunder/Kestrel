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
//! slot clicks, drags and wheel poll (API 24); `tick` and
//! `frame` do nothing, to measure the cost of a call.
use horizon_mod_sdk::{
    ActionHand, ActionTarget, Anchor, BlockDisplay, BlockTarget, ClickKind, ComponentMatch,
    Decoration, DecorationPlace, DecorationTarget, DisplayContext, DragButton, DroppedDisplay,
    Face, FlatDisplay, HandFov, HandFovMode, HandTransform, Highlight, Mod, ModelMatch, ModelRule,
    PartOffset, PoseOffset, PoseTarget, Registration, ScreenButton, ScreenEvent, SoundAdjustment,
    SoundQuery, SwingAnim, actions, audio, chat, entity_pose, game, hand, input, inventory_actions,
    item_model, render, screen, storage,
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
            _ => {
                chat::show(&format!("hello {args} after {} ticks", self.ticks));
                Ok(None)
            }
        }
    }
}

horizon_mod_sdk::export_mod!(Guest);
