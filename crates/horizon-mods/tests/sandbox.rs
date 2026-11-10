//! Loads real mods built with the SDK: the coords-hud and predator-missile
//! examples and a guest
//! that misbehaves on command. Needs `rustup target add wasm32-unknown-unknown`.
use horizon_mods::{
    BlockPos, Clock, Corner, GameMode, HeldItem, Hit, ModEffect, ModHandFov, ModHandFovMode,
    ModMount, ModSelfState, Mods, Output, Package, Player, Runs, ServerPlayer, Shape, Snapshot,
    Status,
    manifest::componentize,
    testing::{Script, TestGame},
};
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
    time::Instant,
};

const GUEST_MANIFEST: &str = r#"
id = "test_guest"
name = "Test Guest"
version = "0.1.0"
api = "0.1"
capabilities = ["game-read", "chat", "storage", "audio", "render", "input", "actions", "screen", "inventory-actions"]
experimental = ["audio", "render", "input", "actions", "screen", "inventory-actions"]
"#;
const UI_MANIFEST: &str = r#"
id = "test_ui_guest"
name = "Test UI Guest"
version = "0.1.0"
api = "0.1"
capabilities = ["game-read", "chat", "command", "ui", "render", "input", "recipes"]
experimental = ["ui", "render", "input", "recipes"]
"#;

/// Builds the mods once and writes their `.kmod` files; returns the folder.
fn mods_dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let target = root.join("target/mods-test");
        let status = Command::new(env!("CARGO"))
            .current_dir(&root)
            .args(["build", "--release", "--target", "wasm32-unknown-unknown"])
            .args([
                "-p",
                "coords-hud",
                "-p",
                "predator-missile",
                "-p",
                "welcome",
                "-p",
                "beacons",
                "-p",
                "world-probe",
                "-p",
                "inventory-probe",
                "-p",
                "entity-probe",
                "-p",
                "draw-probe",
                "-p",
                "light-probe",
                "-p",
                "gamma-probe",
                "-p",
                "camera-probe",
                "-p",
                "self-probe",
                "-p",
                "zoom-probe",
                "-p",
                "pose-probe",
                "-p",
                "action-probe",
                "-p",
                "screen-probe",
                "-p",
                "item-probe",
                "-p",
                "inventory-actions-probe",
                "-p",
                "horizon-mods-test-guest",
                "-p",
                "horizon-mods-test-ui-guest",
                "--target-dir",
            ])
            .arg(&target)
            .status()
            .expect("cargo runs");
        assert!(
            status.success(),
            "building the test mods needs `rustup target add wasm32-unknown-unknown`"
        );
        let built = target.join("wasm32-unknown-unknown/release");
        let dir = target.join("mods");
        std::fs::create_dir_all(&dir).unwrap();
        let example = |name: &str| {
            let path = format!("crates/horizon-mod-sdk/examples/{name}/horizon-mod.toml");
            std::fs::read_to_string(root.join(path)).unwrap()
        };
        let mods = [
            ("coords_hud", example("coords-hud")),
            ("predator_missile", example("predator-missile")),
            ("welcome", example("welcome")),
            ("beacons", example("beacons")),
            ("world_probe", example("world-probe")),
            ("inventory_probe", example("inventory-probe")),
            ("entity_probe", example("entity-probe")),
            ("draw_probe", example("draw-probe")),
            ("light_probe", example("light-probe")),
            ("gamma_probe", example("gamma-probe")),
            ("camera_probe", example("camera-probe")),
            ("self_probe", example("self-probe")),
            ("zoom_probe", example("zoom-probe")),
            ("pose_probe", example("pose-probe")),
            ("action_probe", example("action-probe")),
            ("screen_probe", example("screen-probe")),
            ("item_probe", example("item-probe")),
            (
                "inventory_actions_probe",
                example("inventory-actions-probe"),
            ),
            ("horizon_mods_test_guest", GUEST_MANIFEST.to_string()),
            ("horizon_mods_test_ui_guest", UI_MANIFEST.to_string()),
        ];
        for (name, manifest) in mods {
            let core = std::fs::read(built.join(format!("{name}.wasm"))).unwrap();
            let kmod = Package::write(&manifest, &componentize(&core).unwrap()).unwrap();
            std::fs::write(dir.join(format!("{name}.kmod")), kmod).unwrap();
        }
        dir
    })
}

fn load(name: &str) -> Mods {
    Mods::load(&[mods_dir().join(name)], |_| true, |c| c == "give")
}

fn player() -> Snapshot {
    Snapshot {
        player: Some(Player {
            x: 12.5,
            y: 64.0,
            z: -3.25,
            yaw: 180.0,
            pitch: 0.0,
            health: 20.0,
            food: 20,
            dimension: "minecraft:overworld".into(),
        }),
        clock: Some(Clock {
            game_time: 100,
            day_time: 24000 + 6000,
        }),
        gui_size: (427.0, 240.0),
        ..Snapshot::default()
    }
}

/// Capability `command`: `/missile` queues vanilla commands, and the chat
/// line its impact check prints sets off the warhead.
#[test]
fn predator_missile_runs_commands() {
    let mut mods = load("predator_missile.kmod");
    assert_eq!(mods.entries()[0].status, Status::Running);
    mods.set_snapshot(player());
    let commands = |mods: &mut Mods| -> Vec<String> {
        mods.drain()
            .into_iter()
            .filter_map(|o| match o {
                Output::Command { line } => Some(line),
                _ => None,
            })
            .collect()
    };
    assert!(mods.command("missile"));
    let launch = commands(&mut mods);
    assert_eq!(launch.len(), 6);
    assert!(launch.contains(&"gamemode spectator".to_string()));
    mods.tick(1);
    mods.tick(2);
    let flight = commands(&mut mods);
    assert!(flight.iter().any(|l| l.ends_with("run tp @s ^ ^ ^1.5")));
    mods.chat("Predator: impact");
    let blast = commands(&mut mods);
    assert_eq!(blast.len(), 4);
    assert!(blast.iter().all(|l| l.contains("summon tnt")));
}

/// Capability `audio` (API 15): the game asks the first mod with an
/// `audio-events` export about a world sound, and the mod's answer
/// and its reverb character come back.
#[test]
fn sound_hook_adjusts_and_sets_the_environment() {
    let mut mods = load("horizon_mods_test_guest.kmod");
    assert_eq!(mods.entries()[0].status, Status::Running);
    assert_eq!(mods.sound_environment(), None);
    let query = horizon_mods::ModSoundQuery {
        event: "block.stone.break".into(),
        category: 4,
        position: horizon_mods::ModSoundPosition {
            x: 1.0,
            y: 64.0,
            z: 2.0,
        },
        looping: false,
        volume: 1.0,
        pitch: 1.0,
    };
    let answer = mods.adjust_sound(&query);
    assert_eq!(answer.gain, 0.5);
    assert_eq!(answer.lowpass, 0.25);
    assert_eq!(answer.reverb, 0.75);
    assert_eq!(mods.sound_environment(), Some((0.3, 2.5)));
}

/// Capability `render`'s hand (API 16): the game pushes the hand's
/// state before the hooks, the mod reads it back as pushed, and the
/// offset it sets comes back clamped until it clears it.
#[test]
fn hand_state_flows_and_the_offset_is_clamped_and_cleared() {
    let mut mods = load("horizon_mods_test_guest.kmod");
    assert_eq!(mods.entries()[0].status, Status::Running);
    assert_eq!(mods.hand_offset(), None);
    mods.set_hand_state(horizon_mods::ModHandState {
        swinging: true,
        swing_progress: 0.5,
        swing_kind: horizon_mods::ModSwingKind::Attack,
        equip_progress: 0.75,
        yaw: 170.5,
        pitch: -20.5,
        look_delta_yaw: 3.5,
        look_delta_pitch: -1.5,
        bob_phase: -12.5,
        bob_amount: 0.5,
        held_item: Some("minecraft:diamond_sword".into()),
        using_item: false,
        two_handed: false,
    });
    assert!(mods.command("handstate"));
    assert_eq!(
        mods.drain(),
        [Output::Feedback {
            text: "swing true 0.50 SwingKind::Attack equip 0.75 look 170.5 -20.5 d 3.5 -1.5 \
                   bob -12.5 0.5 item minecraft:diamond_sword using false two false"
                .into(),
            error: false,
        }]
    );
    // The guest sets an x of 2.5 blocks, a pitch of 120 degrees and
    // a NaN yaw: the game clamps the offset it hands back.
    assert!(mods.command("handset"));
    mods.drain();
    let offset = mods.hand_offset().expect("the guest set an offset");
    assert_eq!(
        (
            offset.x,
            offset.y,
            offset.z,
            offset.pitch,
            offset.yaw,
            offset.roll
        ),
        (1.0, -0.25, 0.5, 60.0, 0.0, -30.0)
    );
    assert!(mods.command("handclear"));
    mods.drain();
    assert_eq!(mods.hand_offset(), None);
}

/// The hand's swing terms (API 17): the game pushes the swing
/// animation with the hand state and the mod reads it back, and
/// the scale it sets comes back clamped — 1.0, vanilla in full,
/// when no running mod sets one, and again once the mod stops.
#[test]
fn swing_anim_flows_and_the_scale_is_clamped_and_dropped() {
    let mut mods = load("horizon_mods_test_guest.kmod");
    assert_eq!(mods.entries()[0].status, Status::Running);
    assert_eq!(mods.swing_scale(), 1.0);
    // Nothing pushed yet: no swing runs.
    assert!(mods.command("swinganim"));
    assert_eq!(
        mods.drain(),
        [Output::Feedback {
            text: "anim none".into(),
            error: false,
        }]
    );
    mods.set_swing_anim(horizon_mods::ModSwingAnim::Whack);
    assert!(mods.command("swinganim"));
    assert_eq!(
        mods.drain(),
        [Output::Feedback {
            text: "anim whack".into(),
            error: false,
        }]
    );
    // Past the top of the range: the host clamps to 2.0.
    assert!(mods.command("swingscale 2.7"));
    mods.drain();
    assert_eq!(mods.swing_scale(), 2.0);
    // A non-finite value reads as 1.0.
    assert!(mods.command("swingscale nan"));
    mods.drain();
    assert_eq!(mods.swing_scale(), 1.0);
    // In range, the scale round-trips.
    assert!(mods.command("swingscale 0.35"));
    mods.drain();
    assert_eq!(mods.swing_scale(), 0.35);
    // Clearing the hand state resets the pushed animation with it.
    mods.clear_hand_state();
    assert!(mods.command("swinganim"));
    assert_eq!(
        mods.drain(),
        [Output::Feedback {
            text: "anim none".into(),
            error: false,
        }]
    );
    // A stopped mod's scale is dropped with its store.
    assert!(mods.command("panic"));
    assert_eq!(
        mods.drain(),
        [Output::Stopped {
            name: "Test Guest".into(),
            reason: "it crashed (a panic)".into(),
        }]
    );
    assert_eq!(mods.swing_scale(), 1.0);
}

/// Capability `render`'s entity poses (API 20): the game pushes
/// the pose states before the hooks and the mod reads them back
/// verbatim; offsets come back clamped, one writer owns a target
/// (the probe's later write wins, the guest's clear does not
/// touch it), scales clamp, the own-body ask aggregates, and a
/// stopped mod's writes drop with it.
#[test]
fn pose_states_flow_and_offsets_clamp_and_drop() {
    use horizon_mods::{
        ModArmPose, ModEntityPoseState, ModItemUse, ModPoseSwingAnim, ModPoseTarget,
    };
    let mut mods = Mods::load(
        &[
            mods_dir().join("horizon_mods_test_guest.kmod"),
            mods_dir().join("pose_probe.kmod"),
        ],
        |_| true,
        |_| false,
    );
    assert_eq!(mods.entries()[0].status, Status::Running);
    assert_eq!(mods.entries()[1].status, Status::Running);
    assert_eq!(mods.pose_offset(ModPoseTarget::SelfPlayer), None);
    assert!(!mods.own_body());
    // Nothing pushed yet: no states.
    assert!(mods.command("posestates"));
    assert_eq!(
        mods.drain(),
        [Output::Feedback {
            text: "poses []".into(),
            error: false,
        }]
    );
    // Two pushed states read back verbatim.
    let state = |id: u32, self_: bool| ModEntityPoseState {
        id,
        kind: "minecraft:player".into(),
        self_,
        walk_pos: 0.0,
        walk_speed: 0.0,
        head_yaw: 0.0,
        head_pitch: 0.0,
        swing_progress: 0.0,
        swing_anim: ModPoseSwingAnim::None,
        swing_off_hand: false,
        main_arm_pose: ModArmPose::Empty,
        off_arm_pose: ModArmPose::Empty,
        main_hand: None,
        off_hand: None,
        using: None,
        crouching: false,
        riding: false,
        swim_amount: 0.0,
        gliding: false,
        sleeping: false,
        sprinting: false,
        on_ground: false,
    };
    let mut me = state(7, true);
    me.walk_pos = 1.25;
    me.walk_speed = 0.5;
    me.head_yaw = -30.5;
    me.head_pitch = 12.5;
    me.swing_progress = 0.5;
    me.swing_anim = ModPoseSwingAnim::Whack;
    me.main_arm_pose = ModArmPose::Item;
    me.main_hand = Some("minecraft:iron_sword".into());
    me.using = Some(ModItemUse {
        off_hand: false,
        item: "minecraft:iron_sword".into(),
        ticks: 3,
    });
    me.swim_amount = 0.25;
    me.sprinting = true;
    me.on_ground = true;
    mods.set_pose_states(vec![me, state(42, false)]);
    assert!(mods.command("posestates"));
    assert_eq!(
        mods.drain(),
        [Output::Feedback {
            text: "poses [7 minecraft:player self true walk 1.25 0.50 head -30.5 12.5 \
                   swing 0.50 SwingAnim::Whack off false arms ArmPose::Item ArmPose::Empty \
                   hands minecraft:iron_sword none \
                   using false:minecraft:iron_sword:3 flags false false 0.25 false false true true; \
                   42 minecraft:player self false walk 0.00 0.00 head 0.0 0.0 swing 0.00 SwingAnim::None \
                   off false arms ArmPose::Empty ArmPose::Empty hands none none using none \
                   flags false false 0.00 false false false false]"
                .into(),
            error: false,
        }]
    );
    // The guest sets an x of 2 blocks, a pitch of 270 degrees and
    // a NaN z on self: the aggregate hands back the clamps. Its
    // entity-42 offset stands as set.
    assert!(mods.command("poseoffset 33"));
    mods.drain();
    let (offset, scale) = mods
        .pose_offset(ModPoseTarget::SelfPlayer)
        .expect("the guest set an offset");
    assert_eq!(scale, 1.0);
    assert_eq!(offset.parts.len(), 2);
    let arm = &offset.parts[0];
    assert_eq!(arm.part, "right_arm");
    assert_eq!(
        (arm.x, arm.y, arm.z, arm.pitch, arm.yaw, arm.roll),
        (0.5, 0.25, 0.0, 180.0, 33.0, -30.0)
    );
    assert_eq!(offset.hidden, ["left_leg"]);
    let (entity, _) = mods
        .pose_offset(ModPoseTarget::Entity(42))
        .expect("the guest set an entity offset");
    assert_eq!(entity.parts[0].part, "body");
    assert_eq!(entity.parts[0].pitch, 90.0);
    // The probe ticks on the pushed swing: its later write wins
    // the self target (the mirror of a 0.5 whack: pitch -45,
    // roll 10 on the right arm).
    mods.tick(1);
    mods.drain();
    let (offset, _) = mods
        .pose_offset(ModPoseTarget::SelfPlayer)
        .expect("the probe set an offset");
    assert_eq!(offset.parts.len(), 1);
    assert_eq!(offset.parts[0].part, "right_arm");
    assert_eq!((offset.parts[0].pitch, offset.parts[0].roll), (-45.0, 10.0));
    // The guest clears its own offset: the holder's is untouched.
    assert!(mods.command("poseclear"));
    mods.drain();
    let (offset, _) = mods
        .pose_offset(ModPoseTarget::SelfPlayer)
        .expect("the probe's offset stands");
    assert_eq!(offset.parts[0].pitch, -45.0);
    // The guest's scale write makes it the holder, wholly: the
    // resolved offset is its (empty) one, the scale clamped.
    assert!(mods.command("posescale 2.7"));
    mods.drain();
    let (offset, scale) = mods
        .pose_offset(ModPoseTarget::SelfPlayer)
        .expect("the guest holds the target");
    assert_eq!(scale, 2.0);
    assert!(offset.parts.is_empty() && offset.hidden.is_empty());
    // A non-finite scale reads as 1.0.
    assert!(mods.command("posescale nan"));
    mods.drain();
    assert_eq!(
        mods.pose_offset(ModPoseTarget::SelfPlayer)
            .expect("the guest still holds the target")
            .1,
        1.0
    );
    // The own-body ask flips.
    assert!(mods.command("ownbody on"));
    mods.drain();
    assert!(mods.own_body());
    // Clearing the pushed states: back to none.
    mods.clear_pose_states();
    assert!(mods.command("posestates"));
    assert_eq!(
        mods.drain(),
        [Output::Feedback {
            text: "poses []".into(),
            error: false,
        }]
    );
    // Stopping the guest drops its scale, its entity offset and
    // its body ask; the probe's earlier write stands again.
    assert!(mods.command("panic"));
    assert_eq!(
        mods.drain(),
        [Output::Stopped {
            name: "Test Guest".into(),
            reason: "it crashed (a panic)".into(),
        }]
    );
    assert!(!mods.own_body());
    assert_eq!(mods.pose_offset(ModPoseTarget::Entity(42)), None);
    let (offset, scale) = mods
        .pose_offset(ModPoseTarget::SelfPlayer)
        .expect("the probe's offset stands");
    assert_eq!(scale, 1.0);
    assert_eq!(offset.parts[0].pitch, -45.0);
}

/// Capability `render`'s item models (API 23): the index
/// evaluates newest-loaded-mod first against a stack's merged
/// components — a component held from the item's defaults
/// holds, a patch value holds, an unparseable value never
/// does; a rule naming a missing definition is inert and the
/// walk goes on; the declaration clamps land (64 rules, 4
/// matches a rule, 256 marks); the dropped aggregate follows
/// the newest write; and a stopped mod's declarations drop
/// with it.
#[test]
fn item_model_rules_clamp_resolve_and_drop() {
    use horizon_mods::{ItemDecorationTarget, ModBlockDisplay, ModDisplayContext, ModFlatDisplay};
    use std::collections::BTreeMap;
    let strings = |pairs: &[(&str, &str)]| -> BTreeMap<String, serde_json::Value> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), serde_json::Value::String(v.to_string())))
            .collect()
    };
    let mut mods = Mods::load(
        &[
            mods_dir().join("horizon_mods_test_guest.kmod"),
            mods_dir().join("item_probe.kmod"),
        ],
        |_| true,
        |_| false,
    );
    assert_eq!(mods.entries()[0].status, Status::Running);
    assert_eq!(mods.entries()[1].status, Status::Running);
    // The probe declared at load: its component-less apple rule
    // wins the apple in every group it names, and no other.
    let index = mods.item_model_index();
    assert_eq!(
        index.evaluate(
            "minecraft:apple",
            &strings(&[]),
            ModDisplayContext::Gui,
            |_| true
        ),
        Some("minecraft:golden_apple")
    );
    assert_eq!(
        index.evaluate(
            "minecraft:apple",
            &strings(&[]),
            ModDisplayContext::Ground,
            |_| true
        ),
        None
    );
    assert!(!index.rules_item("minecraft:dirt"));
    // The guest declares 65 rules: the host keeps 64, the last
    // (on `minecraft:dropped_item`) dropped — and precedence is
    // load order, not write order: the probe's rule still wins.
    assert!(mods.command("imrules"));
    mods.drain();
    let index = mods.item_model_index();
    assert_eq!(index.rules().count(), 65);
    assert!(!index.rules_item("minecraft:dropped_item"));
    assert_eq!(
        index.evaluate(
            "minecraft:apple",
            &strings(&[]),
            ModDisplayContext::Gui,
            |_| true
        ),
        Some("minecraft:golden_apple")
    );
    // A definition that does not exist is inert, and the walk
    // goes on to the guest's rarity rule: a rarity the stack
    // holds from its item's defaults (the fixture map IS the
    // merged set) holds.
    let no_golden = |id: &str| id != "minecraft:golden_apple";
    assert_eq!(
        index.evaluate(
            "minecraft:apple",
            &strings(&[("minecraft:rarity", "rare")]),
            ModDisplayContext::Gui,
            no_golden,
        ),
        Some("test:rare_apple")
    );
    // No definitions at all: nothing swaps.
    assert_eq!(
        index.evaluate(
            "minecraft:apple",
            &strings(&[("minecraft:rarity", "rare")]),
            ModDisplayContext::Gui,
            |_| false,
        ),
        None
    );
    // A patch value holds: the renamed apple matches the
    // compound the guest keyed on, in the hand's group too.
    let mut renamed = BTreeMap::new();
    renamed.insert(
        "minecraft:custom_name".into(),
        serde_json::json!({"text": "Bob"}),
    );
    assert_eq!(
        index.evaluate(
            "minecraft:apple",
            &renamed,
            ModDisplayContext::FirstPerson,
            no_golden,
        ),
        Some("test:bob_apple")
    );
    // The stone's value never parsed: the rule never holds,
    // whatever the stack carries.
    assert_eq!(
        index.evaluate(
            "minecraft:stone",
            &strings(&[("minecraft:custom_data", "anything")]),
            ModDisplayContext::Gui,
            |_| true,
        ),
        None
    );
    // The stick declared five matches; the host kept four: it
    // holds when a to d hold, and only then.
    let abcd: BTreeMap<String, serde_json::Value> = [("a", 1), ("b", 2), ("c", 3), ("d", 4)]
        .iter()
        .map(|(k, v)| (k.to_string(), serde_json::json!(v)))
        .collect();
    assert_eq!(
        index.evaluate("minecraft:stick", &abcd, ModDisplayContext::Gui, |_| true),
        Some("test:stick")
    );
    let abc = abcd
        .iter()
        .take(3)
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    assert_eq!(
        index.evaluate("minecraft:stick", &abc, ModDisplayContext::Gui, |_| true),
        None
    );
    // Marks: the probe's two stand; the guest's 257 declared
    // are 256 kept, the slot-9999 mark dropped, the union in
    // load order (the guest's marks first, the probe's last).
    assert_eq!(mods.decorations().len(), 2);
    assert!(mods.command("immarks"));
    mods.drain();
    let marks = mods.decorations();
    assert_eq!(marks.len(), 256 + 2);
    assert!(
        !marks
            .iter()
            .any(|d| matches!(d.target, ItemDecorationTarget::Slot(9999)))
    );
    assert!(matches!(marks[257].target, ItemDecorationTarget::Slot(0)));
    // The dropped aggregate is the probe's until the guest
    // writes, then the guest's newest write — an all-vanilla
    // record still counts as a write.
    assert_eq!(mods.dropped_display().flat, ModFlatDisplay::Sprite);
    assert!(mods.command("imdropped full"));
    mods.drain();
    let d = mods.dropped_display();
    assert_eq!(
        (d.flat, d.blocks, d.shadows),
        (ModFlatDisplay::Sprite, ModBlockDisplay::Billboard, false)
    );
    assert!(mods.command("imdropped off"));
    mods.drain();
    assert_eq!(mods.dropped_display().flat, ModFlatDisplay::Vanilla);
    // Stopping the guest drops its rules, marks and policy;
    // the probe's declarations stand.
    assert!(mods.command("panic"));
    assert_eq!(
        mods.drain(),
        [Output::Stopped {
            name: "Test Guest".into(),
            reason: "it crashed (a panic)".into(),
        }]
    );
    assert_eq!(mods.item_model_index().rules().count(), 1);
    assert_eq!(mods.decorations().len(), 2);
    assert_eq!(mods.dropped_display().flat, ModFlatDisplay::Sprite);
}

/// Capability `actions` (API 21): the snapshot's target reaches
/// the mod verbatim; clicks queue as outputs in call order, the
/// ninth in one call dropped; a malformed use-on-block errs at
/// call time and queues nothing, as does a slot outside 0 to 8;
/// the held aggregate reads back, latches off on the game's
/// release, re-arms on the mod's own call, and drops with a
/// stopped mod.
#[test]
fn actions_queue_in_order_clamp_and_drop() {
    use horizon_mods::{
        ModAction, ModActionTarget, ModBlockTarget, ModEntityTarget, ModFace, ModHand,
    };
    let mut mods = load("horizon_mods_test_guest.kmod");
    assert_eq!(mods.entries()[0].status, Status::Running);
    let actions = |mods: &mut Mods| -> Vec<ModAction> {
        mods.drain()
            .into_iter()
            .filter_map(|o| match o {
                Output::Action { action, .. } => Some(action),
                _ => None,
            })
            .collect()
    };
    // No target in the snapshot: none under the crosshair.
    mods.set_snapshot(player());
    assert!(mods.command("acttarget"));
    assert_eq!(
        mods.drain(),
        [Output::Feedback {
            text: "target none".into(),
            error: false,
        }]
    );
    // The fixture's block target reads back verbatim: position,
    // state, face and cursor.
    let wheat = ModBlockTarget {
        block_x: 3,
        block_y: 64,
        block_z: -2,
        block: "minecraft:wheat[age=7]".into(),
        face: ModFace::Up,
        cursor_x: 0.5,
        cursor_y: 1.0,
        cursor_z: 0.5,
    };
    let mut snapshot = player();
    snapshot.action_target = Some(ModActionTarget::Block(wheat.clone()));
    mods.set_snapshot(snapshot);
    assert!(mods.command("acttarget"));
    assert_eq!(
        mods.drain(),
        [Output::Feedback {
            text: "target block 3 64 -2 minecraft:wheat[age=7] up 0.50 1.00 0.50".into(),
            error: false,
        }]
    );
    // And the entity in front of a block.
    let mut snapshot = player();
    snapshot.action_target = Some(ModActionTarget::Entity(ModEntityTarget {
        id: 42,
        kind: "minecraft:zombie".into(),
        distance: 3.25,
    }));
    mods.set_snapshot(snapshot);
    assert!(mods.command("acttarget"));
    assert_eq!(
        mods.drain(),
        [Output::Feedback {
            text: "target entity 42 minecraft:zombie 3.25".into(),
            error: false,
        }]
    );
    // Clicks queue in the order they were made.
    assert!(mods.command("actuse"));
    assert!(mods.command("actattack"));
    assert_eq!(actions(&mut mods), [ModAction::Use, ModAction::Attack]);
    // The malformed use-on-block errs at call time and queues
    // nothing; the well-formed one queues behind nothing.
    assert!(mods.command("acton"));
    let out = mods.drain();
    assert_eq!(
        out.iter()
            .filter_map(|o| match o {
                Output::Feedback { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>(),
        ["bad the target's cursor is outside its block; queued"]
    );
    assert_eq!(
        out.iter()
            .filter_map(|o| match o {
                Output::Action { action, .. } => Some(action.clone()),
                _ => None,
            })
            .collect::<Vec<_>>(),
        [ModAction::UseOnBlock {
            target: wheat,
            hand: ModHand::Main,
        }]
    );
    // Slot 9 errs and queues nothing; slot 3 queues.
    assert!(mods.command("actslot"));
    let out = mods.drain();
    assert_eq!(
        out.iter()
            .filter_map(|o| match o {
                Output::Feedback { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>(),
        ["bad slot 9 is outside 0 to 8; queued"]
    );
    assert_eq!(
        out.iter()
            .filter_map(|o| match o {
                Output::Action { action, .. } => Some(action.clone()),
                _ => None,
            })
            .collect::<Vec<_>>(),
        [ModAction::SelectSlot(3)]
    );
    // The ninth action in one call is dropped: eight are kept.
    assert!(mods.command("actspam"));
    assert_eq!(actions(&mut mods), vec![ModAction::Use; 8]);
    // The held aggregate: set, latched off by the game's
    // release, re-armed by the mod's own call, and gone when
    // the mod stops.
    assert_eq!(mods.held_buttons(), (false, false));
    assert!(mods.command("actheld attack"));
    mods.drain();
    assert_eq!(mods.held_buttons(), (true, false));
    mods.release_held_buttons();
    assert_eq!(mods.held_buttons(), (false, false));
    assert!(mods.command("actheld attack"));
    mods.drain();
    assert_eq!(mods.held_buttons(), (true, false));
    assert!(mods.command("panic"));
    assert_eq!(
        mods.drain(),
        [Output::Stopped {
            name: "Test Guest".into(),
            reason: "it crashed (a panic)".into(),
        }]
    );
    assert_eq!(mods.held_buttons(), (false, false));
}

/// Capability `inventory-actions` (API 24): clicks and drags queue
/// as outputs in call order with their kind, button and slots;
/// the malformed errs at call time and queues nothing — a button
/// the kind does not take, a slot past 999, a drag of 0 or 65
/// slots; one call keeps 32 weighed verbs, a drag weighing 8
/// (four drags fill a call, the fifth dropped); the wheel queue
/// returns its turns in order behind the 256 cap, the oldest
/// dropped.
#[test]
fn slot_moves_queue_clamp_and_drop() {
    use horizon_mods::{
        ModClickKind, ModDragButton, ModSlotClick, ModSlotDrag, ModSlotMove, ModWheelEvent,
    };
    let mut mods = load("horizon_mods_test_guest.kmod");
    assert_eq!(mods.entries()[0].status, Status::Running);
    mods.set_snapshot(player());
    let moves = |mods: &mut Mods| -> Vec<ModSlotMove> {
        mods.drain()
            .into_iter()
            .filter_map(|o| match o {
                Output::SlotClick { click, .. } => Some(ModSlotMove::Click(click)),
                Output::SlotDrag { drag, .. } => Some(ModSlotMove::Drag(drag)),
                _ => None,
            })
            .collect()
    };
    let feedback = |mods: &mut Mods| -> Vec<(String, bool)> {
        mods.drain()
            .into_iter()
            .filter_map(|o| match o {
                Output::Feedback { text, error } => Some((text, error)),
                _ => None,
            })
            .collect()
    };
    // A click queues with its slot, button and kind, ahead of
    // the command's success feedback.
    assert!(mods.command("invclick"));
    assert_eq!(
        mods.drain(),
        [
            Output::SlotClick {
                mod_id: "test_guest".into(),
                click: ModSlotClick {
                    slot: Some(9),
                    button: 0,
                    kind: ModClickKind::Pickup,
                },
            },
            Output::Feedback {
                text: "clicked".into(),
                error: false,
            },
        ]
    );
    // A swap with button 9 errs at call time and queues nothing;
    // button 3 queues.
    assert!(mods.command("invswap"));
    let out = mods.drain();
    assert_eq!(
        out.iter()
            .filter_map(|o| match o {
                Output::Feedback { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>(),
        ["bad swap takes a hotbar slot, 0 to 8 or 40, not button 9; queued"]
    );
    assert_eq!(
        out.iter()
            .filter_map(|o| match o {
                Output::SlotClick { click, .. } => Some(click.clone()),
                _ => None,
            })
            .collect::<Vec<_>>(),
        [ModSlotClick {
            slot: Some(9),
            button: 3,
            kind: ModClickKind::Swap,
        }]
    );
    // A throw queues; a drag queues whole, in slot order.
    assert!(mods.command("invthrow"));
    assert!(mods.command("invdrag"));
    assert_eq!(
        moves(&mut mods),
        [
            ModSlotMove::Click(ModSlotClick {
                slot: Some(9),
                button: 0,
                kind: ModClickKind::Throw,
            }),
            ModSlotMove::Drag(ModSlotDrag {
                button: ModDragButton::Right,
                slots: vec![9, 10, 11],
            }),
        ]
    );
    // The malformed three err at call time; nothing queues.
    assert!(mods.command("invbad"));
    assert_eq!(
        feedback(&mut mods),
        [(
            "bad slot 1000 is past 999; a drag names no slots; \
             a drag names 65 slots, past 64"
                .to_string(),
            false
        )]
    );
    // The 33rd click in one call is dropped: 32 are kept.
    assert!(mods.command("invspam"));
    assert_eq!(
        moves(&mut mods),
        vec![
            ModSlotMove::Click(ModSlotClick {
                slot: Some(9),
                button: 0,
                kind: ModClickKind::Pickup,
            });
            32
        ]
    );
    // A drag weighs 8: four fill a call's 32, the fifth drops.
    assert!(mods.command("invdragspam"));
    assert_eq!(
        moves(&mut mods),
        vec![
            ModSlotMove::Drag(ModSlotDrag {
                button: ModDragButton::Right,
                slots: vec![9],
            });
            4
        ]
    );
    // The wheel queue: 300 turns pushed, 256 kept, the oldest
    // dropped, the rest in order; a second poll reads empty.
    for steps in 0..300 {
        mods.push_wheel(ModWheelEvent {
            slot: Some(9),
            steps,
        });
    }
    assert!(mods.command("invwheel"));
    let text = feedback(&mut mods).remove(0).0;
    assert!(text.starts_with("wheel [9 44;"), "{text}");
    assert!(text.ends_with("9 299]"), "{text}");
    assert_eq!(text.matches("; ").count(), 255, "{text}");
    assert!(mods.command("invwheel"));
    assert_eq!(feedback(&mut mods), [("wheel []".to_string(), false)]);
}

/// Capability `screen` (API 22): the snapshot's screen state and
/// merchant offers reach the mod verbatim, at the charged counts;
/// button declarations clamp at the store and resolve per
/// context (the first eight in load order), highlights clamp at
/// 512, presses and middle clicks drain through `poll` in order
/// behind the 256 cap, a trade choice queues as its own output
/// and errs at call time with no merchant open, and the debug
/// claim and the declarations die with a stopped mod.
#[test]
fn screen_state_buttons_and_poll_flow() {
    use horizon_mods::{
        ModScreenEvent, ModScreenState, ModTradeList, ModTradeOffer, ModTradeStack,
    };
    let mut mods = load("horizon_mods_test_guest.kmod");
    assert_eq!(mods.entries()[0].status, Status::Running);
    let feedback = |mods: &mut Mods| -> Vec<(String, bool)> {
        mods.drain()
            .into_iter()
            .filter_map(|o| match o {
                Output::Feedback { text, error } => Some((text, error)),
                _ => None,
            })
            .collect()
    };
    // The default snapshot: no screen open, no merchant.
    mods.set_snapshot(player());
    assert!(mods.command("scrnstate"));
    assert_eq!(
        feedback(&mut mods),
        [(
            "screen playing menu - title - rect 0.0 0.0 0.0 0.0 hovered - debug false \
             trades - selected -"
                .to_string(),
            false
        )]
    );
    // A trade choice with no merchant open errs at call time
    // and queues nothing.
    assert!(mods.command("scrntrade"));
    assert_eq!(
        mods.drain(),
        [Output::Feedback {
            text: "no merchant screen is open".into(),
            error: true,
        }]
    );
    // The fixture's merchant state reads back verbatim: the
    // menu type, the plain title, the panel, the hovered slot,
    // and the offers at their charged counts.
    let stack = |item: &str, count: u32| ModTradeStack {
        item: item.into(),
        count,
        components: String::new(),
    };
    let mut snapshot = player();
    snapshot.screen_state = ModScreenState {
        id: "inventory".into(),
        menu: Some("minecraft:merchant".into()),
        title: Some("Novice".into()),
        rect: (75.0, 17.0, 276.0, 166.0),
        hovered_slot: Some(300),
        debug_open: false,
    };
    snapshot.trades = Some(ModTradeList {
        offers: vec![
            ModTradeOffer {
                cost_a: stack("minecraft:emerald", 5),
                cost_b: None,
                result: stack("minecraft:bread", 1),
                uses: 0,
                max_uses: 12,
                available: true,
            },
            ModTradeOffer {
                cost_a: stack("minecraft:emerald", 9),
                cost_b: Some(stack("minecraft:book", 1)),
                result: stack("minecraft:enchanted_book", 1),
                uses: 12,
                max_uses: 12,
                available: false,
            },
        ],
        selected: Some(0),
    });
    mods.set_snapshot(snapshot);
    assert!(mods.command("scrnstate"));
    assert_eq!(
        feedback(&mut mods),
        [(
            "screen inventory menu minecraft:merchant title Novice rect 75.0 17.0 276.0 \
             166.0 hovered 300 debug false trades 2 selected 0 first minecraft:emerald 5 \
             -> minecraft:bread avail true"
                .to_string(),
            false
        )]
    );
    // With the merchant open, the choice queues as its own
    // output, ahead of the command's success feedback.
    assert!(mods.command("scrntrade"));
    assert_eq!(
        mods.drain(),
        [
            Output::SelectTrade {
                mod_id: "test_guest".into(),
                index: 0,
            },
            Output::Feedback {
                text: "chose".into(),
                error: false,
            },
        ]
    );
    // Seventeen declarations store sixteen; the context
    // aggregate draws the first eight, for the screen they
    // name and no other.
    assert!(mods.command("scrnbuttons"));
    mods.drain();
    let resolved = mods.screen_buttons("inventory", None);
    assert_eq!(
        resolved.iter().map(|(_, b)| b.id).collect::<Vec<_>>(),
        [1, 2, 3, 4, 5, 6, 7, 8]
    );
    assert!(resolved.iter().all(|(owner, _)| *owner == 0));
    // A menu-less button matches a menu context too.
    assert_eq!(
        mods.screen_buttons("inventory", Some("minecraft:merchant"))
            .len(),
        8
    );
    assert!(mods.screen_buttons("paused", None).is_empty());
    // 513 marks keep 512.
    assert!(mods.command("scrnhighlight"));
    mods.drain();
    let marks = mods.screen_highlights();
    assert_eq!(marks.len(), 512);
    assert_eq!((marks[0].slot, marks[0].color), (0, 0xFF0000));
    assert_eq!(marks[511].slot, 511);
    // Presses and middle clicks drain in order; a second poll
    // is empty.
    mods.screen_event(0, ModScreenEvent::Button(7));
    mods.screen_middle_click(Some(300));
    mods.screen_middle_click(None);
    assert!(mods.command("scrnpoll"));
    assert_eq!(
        feedback(&mut mods),
        [(
            "events [button 7; middle 300; middle none]".to_string(),
            false
        )]
    );
    assert!(mods.command("scrnpoll"));
    assert_eq!(feedback(&mut mods), [("events []".to_string(), false)]);
    // The queue keeps the newest 256: of 300 presses the first
    // polled is the 45th pushed.
    for id in 0..300 {
        mods.screen_event(0, ModScreenEvent::Button(id));
    }
    assert!(mods.command("scrnpoll"));
    let (text, error) = &feedback(&mut mods)[0];
    assert!(!error);
    assert!(text.starts_with("events [button 44;"), "{text}");
    assert_eq!(text.matches("button").count(), 256);
    // The debug claim reads back, and a stopped mod's claim
    // and declarations die with it.
    assert!(!mods.debug_claimed());
    assert!(mods.command("scrndebug on"));
    mods.drain();
    assert!(mods.debug_claimed());
    assert!(mods.command("panic"));
    assert_eq!(
        mods.drain(),
        [Output::Stopped {
            name: "Test Guest".into(),
            reason: "it crashed (a panic)".into(),
        }]
    );
    assert!(!mods.debug_claimed());
    assert!(mods.screen_buttons("inventory", None).is_empty());
    assert!(mods.screen_highlights().is_empty());
}

/// Capability `game-read`'s self reads (API 18): the snapshot's
/// effects and self-state reach the mod verbatim — an infinite
/// effect's -1 among them, and the mount's health —, both read
/// empty/none from a snapshot that carries neither, and the
/// movement toggle write drains as its own output.
#[test]
fn effects_and_self_state_flow_and_the_toggles_queue() {
    let mut mods = load("horizon_mods_test_guest.kmod");
    assert_eq!(mods.entries()[0].status, Status::Running);
    let mut snapshot = player();
    snapshot.effects = vec![
        ModEffect {
            id: "minecraft:speed".into(),
            amplifier: 1,
            duration: 1234,
            ambient: false,
            show_particles: true,
            show_icon: true,
        },
        ModEffect {
            id: "minecraft:regeneration".into(),
            amplifier: 0,
            duration: -1,
            ambient: true,
            show_particles: false,
            show_icon: true,
        },
    ];
    snapshot.self_state = Some(ModSelfState {
        sprinting: true,
        sneaking: false,
        flying: false,
        gliding: false,
        mount: Some(ModMount {
            id: 42,
            kind: "minecraft:horse".into(),
            health: 24.5,
            max_health: 30.0,
        }),
        sprint_toggled: true,
        sneak_toggled: false,
    });
    mods.set_snapshot(snapshot);
    assert!(mods.command("effects"));
    assert_eq!(
        mods.drain(),
        [Output::Feedback {
            text: "effects [minecraft:speed amp 1 1234 ticks ambient false particles true \
                   icon true; minecraft:regeneration amp 0 -1 ticks ambient true particles \
                   false icon true]"
                .into(),
            error: false,
        }]
    );
    assert!(mods.command("selfstate"));
    assert_eq!(
        mods.drain(),
        [Output::Feedback {
            text: "self sprint true sneak false fly false glide false mount 42 \
                   minecraft:horse 24.5/30.0 toggles true false"
                .into(),
            error: false,
        }]
    );
    // No snapshot data (the title screen's shape): no effects, no self.
    mods.set_snapshot(player());
    assert!(mods.command("effects"));
    assert_eq!(
        mods.drain(),
        [Output::Feedback {
            text: "effects []".into(),
            error: false,
        }]
    );
    assert!(mods.command("selfstate"));
    assert_eq!(
        mods.drain(),
        [Output::Feedback {
            text: "self none".into(),
            error: false,
        }]
    );
    // The toggle write: each half set or left, drained as one output
    // ahead of the command's feedback (as a `give` lands before the
    // feedback of the command that asked for it).
    assert!(mods.command("toggles on keep"));
    assert_eq!(
        mods.drain(),
        [
            Output::SetMovementToggles {
                sprint: Some(true),
                sneak: None,
            },
            Output::Feedback {
                text: "toggled".into(),
                error: false,
            },
        ]
    );
    assert!(mods.command("toggles off on"));
    assert_eq!(
        mods.drain(),
        [
            Output::SetMovementToggles {
                sprint: Some(false),
                sneak: Some(true),
            },
            Output::Feedback {
                text: "toggled".into(),
                error: false,
            },
        ]
    );
}

/// The zoom's truth and the idle clock (API 19): the snapshot's
/// `can-zoom`, spyglass factor and last-input milliseconds reach
/// the mod verbatim — the spyglass's 10 among them — and read as
/// the not-in-a-world defaults from a snapshot that carries none.
#[test]
fn zoom_truth_and_the_idle_clock_flow() {
    let mut mods = load("horizon_mods_test_guest.kmod");
    assert_eq!(mods.entries()[0].status, Status::Running);
    let mut snapshot = player();
    snapshot.can_zoom = true;
    snapshot.spyglass_zoom = Some(10.0);
    snapshot.last_input_ms = 123_456;
    mods.set_snapshot(snapshot);
    assert!(mods.command("zoomstate"));
    assert_eq!(
        mods.drain(),
        [Output::Feedback {
            text: "zoom can true spy 10".into(),
            error: false,
        }]
    );
    assert!(mods.command("idle"));
    assert_eq!(
        mods.drain(),
        [Output::Feedback {
            text: "idle 123456".into(),
            error: false,
        }]
    );
    // A plain snapshot (the title screen's shape): kept zoom, no
    // scope, no idle time.
    mods.set_snapshot(player());
    assert!(mods.command("zoomstate"));
    assert_eq!(
        mods.drain(),
        [Output::Feedback {
            text: "zoom can false spy none".into(),
            error: false,
        }]
    );
    assert!(mods.command("idle"));
    assert_eq!(
        mods.drain(),
        [Output::Feedback {
            text: "idle 0".into(),
            error: false,
        }]
    );
}

/// `render.set-zoom`'s widened cap (API 19): a factor of 300 clamps
/// to 250, where it clamped to 10 before; an in-range factor and
/// the old cap round-trip.
#[test]
fn the_zoom_factor_clamps_at_250() {
    let mut mods = load("horizon_mods_test_ui_guest.kmod");
    assert_eq!(mods.entries()[0].status, Status::Running);
    assert!(mods.command("zoom 300 0 1"));
    mods.drain();
    assert_eq!(mods.zoom().map(|z| z.factor), Some(250.0));
    assert!(mods.command("zoom 200 0 1"));
    mods.drain();
    assert_eq!(mods.zoom().map(|z| z.factor), Some(200.0));
    assert!(mods.command("zoom 10 0 1"));
    mods.drain();
    assert_eq!(mods.zoom().map(|z| z.factor), Some(10.0));
}

/// `render.set-hide-hud` (API 19): any running mod's flag hides the
/// HUD; one mod letting go while another holds changes nothing, and
/// a stopped mod's ask is dropped with its store.
#[test]
fn hide_hud_aggregates_across_mods_and_drops_on_stop() {
    let paths = [
        mods_dir().join("horizon_mods_test_guest.kmod"),
        mods_dir().join("horizon_mods_test_ui_guest.kmod"),
    ];
    let mut mods = Mods::load(&paths, |_| true, |_| false);
    assert!(!mods.hide_hud());
    assert!(mods.command("hidehud on"));
    mods.drain();
    assert!(mods.hide_hud());
    assert!(mods.command("hide_hud on"));
    mods.drain();
    assert!(mods.hide_hud());
    // The guest lets go; the UI guest still holds.
    assert!(mods.command("hidehud off"));
    mods.drain();
    assert!(mods.hide_hud());
    assert!(mods.command("hide_hud off"));
    mods.drain();
    assert!(!mods.hide_hud());
    // A stopped mod's flag is dropped with its store.
    assert!(mods.command("hidehud on"));
    mods.drain();
    assert!(mods.hide_hud());
    assert!(mods.command("panic"));
    assert_eq!(
        mods.drain(),
        [Output::Stopped {
            name: "Test Guest".into(),
            reason: "it crashed (a panic)".into(),
        }]
    );
    assert!(!mods.hide_hud());
}

/// `render.set-hand-fov` (API 19): the first loaded mod that sets
/// one wins — a later mod's set waits behind it — a non-finite base
/// reads as 70 through the host's sanitize, and the winner stopping
/// hands the setting to the next mod that set one.
#[test]
fn hand_fov_is_first_loaded_wins_and_drops_on_stop() {
    let paths = [
        mods_dir().join("horizon_mods_test_guest.kmod"),
        mods_dir().join("horizon_mods_test_ui_guest.kmod"),
    ];
    let mut mods = Mods::load(&paths, |_| true, |_| false);
    assert_eq!(mods.hand_fov(), None);
    // The second mod sets first: with the first mod silent, its
    // setting is the one that applies.
    assert!(mods.command("hand_fov add 80"));
    mods.drain();
    assert_eq!(
        mods.hand_fov(),
        Some(ModHandFov {
            mode: ModHandFovMode::Add,
            base: 80.0,
        })
    );
    // The first loaded mod sets: it wins, over the waiting one.
    assert!(mods.command("handfov multiply 60"));
    mods.drain();
    assert_eq!(
        mods.hand_fov(),
        Some(ModHandFov {
            mode: ModHandFovMode::Multiply,
            base: 60.0,
        })
    );
    // A non-finite base is sanitized to vanilla's 70.
    assert!(mods.command("handfov keep nan"));
    mods.drain();
    assert_eq!(
        mods.hand_fov(),
        Some(ModHandFov {
            mode: ModHandFovMode::Keep,
            base: 70.0,
        })
    );
    // The winner stops: the waiting mod's setting takes over.
    assert!(mods.command("panic"));
    mods.drain();
    assert_eq!(
        mods.hand_fov(),
        Some(ModHandFov {
            mode: ModHandFovMode::Add,
            base: 80.0,
        })
    );
    assert!(mods.command("hand_fov off"));
    mods.drain();
    assert_eq!(mods.hand_fov(), None);
}

/// `render.set-fps-cap` (API 19): the aggregate is the lowest cap
/// any running mod set, values sanitize to 1..=240 at the host
/// (0 reads as 1, 999 as 240), and a stopped mod's cap is dropped.
#[test]
fn fps_cap_is_the_minimum_and_drops_on_stop() {
    let paths = [
        mods_dir().join("horizon_mods_test_guest.kmod"),
        mods_dir().join("horizon_mods_test_ui_guest.kmod"),
    ];
    let mut mods = Mods::load(&paths, |_| true, |_| false);
    assert_eq!(mods.fps_cap(), None);
    assert!(mods.command("fpscap 30"));
    mods.drain();
    assert_eq!(mods.fps_cap(), Some(30));
    assert!(mods.command("fps_cap 60"));
    mods.drain();
    assert_eq!(mods.fps_cap(), Some(30));
    assert!(mods.command("fps_cap 20"));
    mods.drain();
    assert_eq!(mods.fps_cap(), Some(20));
    // 0 sanitizes to 1, the deepest tier — and wins the minimum.
    assert!(mods.command("fpscap 0"));
    mods.drain();
    assert_eq!(mods.fps_cap(), Some(1));
    assert!(mods.command("fpscap off"));
    mods.drain();
    assert_eq!(mods.fps_cap(), Some(20));
    assert!(mods.command("fpscap 15"));
    mods.drain();
    assert_eq!(mods.fps_cap(), Some(15));
    // The guest stops: its cap is dropped, the other's stands.
    assert!(mods.command("panic"));
    mods.drain();
    assert_eq!(mods.fps_cap(), Some(20));
    // 999 sanitizes to the top of the range.
    assert!(mods.command("fps_cap 999"));
    mods.drain();
    assert_eq!(mods.fps_cap(), Some(240));
    assert!(mods.command("fps_cap off"));
    mods.drain();
    assert_eq!(mods.fps_cap(), None);
}

/// The registered-key cap (API 19): 32 keys register; the 33rd is
/// refused with the cap's own error (16 before this API).
#[test]
fn the_registered_key_cap_is_32() {
    let mut mods = load("horizon_mods_test_ui_guest.kmod");
    assert_eq!(mods.entries()[0].status, Status::Running);
    assert!(mods.command("keys 32"));
    assert_eq!(
        mods.drain(),
        [Output::Chat {
            name: "Test UI Guest".into(),
            text: "keys 32 took; ".into(),
        }]
    );
    assert!(mods.command("keys 33"));
    assert_eq!(
        mods.drain(),
        [Output::Chat {
            name: "Test UI Guest".into(),
            text: "keys 32 took; a mod may register 32 keys".into(),
        }]
    );
}

#[test]
fn coords_hud_draws_and_answers() {
    let mut mods = load("coords_hud.kmod");
    assert_eq!(mods.entries()[0].status, Status::Running);
    assert_eq!(mods.commands().collect::<Vec<_>>(), ["where", "coords"]);
    mods.set_snapshot(player());
    mods.tick(1);
    let panel = mods.panels().next().expect("a panel after a tick");
    assert_eq!(panel.corner, Corner::TopLeft);
    let lines: Vec<&str> = panel.lines.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(
        lines,
        [
            "XYZ 12.5 / 64.0 / -3.2",
            "Facing North (-Z)",
            "Day 2, 12:00"
        ]
    );

    assert!(mods.command("where"));
    let chat = Output::Chat {
        name: "Coordinates HUD".into(),
        text: "12, 64, -4 in minecraft:overworld".into(),
    };
    assert_eq!(mods.drain(), [chat]);
    assert!(mods.command("coords"));
    mods.tick(2);
    assert_eq!(mods.panels().count(), 0);
    let hidden = Output::Feedback {
        text: "Coordinates panel hidden".into(),
        error: false,
    };
    assert_eq!(mods.drain(), [hidden]);
    assert!(!mods.command("gamemode creative"));
}

#[test]
fn off_mods_cost_nothing() {
    let mods = Mods::load(&[mods_dir().join("coords_hud.kmod")], |_| false, |_| false);
    assert_eq!(mods.entries()[0].status, Status::Off);
    assert!(!mods.is_running());
    assert_eq!(mods.commands().count(), 0);
}

#[test]
fn undeclared_capabilities_are_refused() {
    let dir = mods_dir().join("undeclared");
    std::fs::create_dir_all(&dir).unwrap();
    let wasm = Package::read(&mods_dir().join("horizon_mods_test_guest.kmod"))
        .unwrap()
        .wasm
        .expect("the guest has code");
    let manifest = GUEST_MANIFEST.replace("\"chat\", ", "");
    std::fs::write(dir.join("horizon-mod.toml"), manifest).unwrap();
    std::fs::write(dir.join("mod.wasm"), wasm).unwrap();
    let mods = Mods::load(&[dir], |_| true, |_| false);
    let refused = Status::Failed("uses capability chat, missing from horizon-mod.toml".into());
    assert_eq!(mods.entries()[0].status, refused);
}

/// A mod calling a function this Horizon doesn't have is refused with
/// "update Horizon", not a linking error; the test mods' levels are read
/// from the functions they import.
#[test]
fn a_mod_for_a_newer_horizon_is_refused() {
    use horizon_mods::api_level::{imports, needed};
    use wit_parser::{ManglingAndAbi, Resolve};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let wit = std::fs::read_to_string(root.join("crates/horizon-mod-sdk/wit/horizon.wit"))
        .unwrap()
        .replace(
            "    set-minimap: func(minimap: option<minimap>);",
            "    set-minimap: func(minimap: option<minimap>);\n    set-sky: func(on: bool);",
        );
    let mut resolve = Resolve::default();
    let package = resolve.push_str("horizon.wit", &wit).unwrap();
    let world = resolve
        .select_world(&[package], Some("horizon-ui-mod"))
        .unwrap();
    let mut module = wit_component::dummy_module(&resolve, world, ManglingAndAbi::Standard32);
    wit_component::embed_component_metadata(
        &mut module,
        &resolve,
        world,
        wit_component::StringEncoding::UTF8,
    )
    .unwrap();
    let wasm = wit_component::ComponentEncoder::default()
        .module(&module)
        .unwrap()
        .encode()
        .unwrap();
    let functions = imports(&wasm).unwrap();
    assert!(functions.iter().any(|f| f == "render.set-sky"));
    let dir = mods_dir().join("too_new");
    std::fs::create_dir_all(&dir).unwrap();
    let manifest = r#"
id = "too_new"
name = "Too New"
version = "0.1.0"
api = "0.1"
capabilities = ["game-read", "chat", "hud", "storage", "command", "ui", "render", "input", "recipes", "world", "inventory", "entity", "audio", "actions", "screen", "inventory-actions"]
experimental = ["ui", "render", "input", "recipes", "inventory", "entity", "audio", "actions", "screen", "inventory-actions"]
"#;
    std::fs::write(dir.join("horizon-mod.toml"), manifest).unwrap();
    std::fs::write(dir.join("mod.wasm"), wasm).unwrap();
    let mods = Mods::load(&[dir], |_| true, |_| false);
    let refused = "made for a newer Horizon (it uses render.set-sky): update Horizon to run it";
    assert_eq!(mods.entries()[0].status, Status::Failed(refused.into()));

    let level = |file: &str| {
        let wasm = Package::read(&mods_dir().join(file)).unwrap().wasm.unwrap();
        needed(imports(&wasm).unwrap().iter().map(String::as_str)).0
    };
    assert_eq!(level("coords_hud.kmod"), 0);
    // The UI guest sets the hand FOV (`render.set-hand-fov`, level 19).
    assert_eq!(level("horizon_mods_test_ui_guest.kmod"), 19);
    // The world probe reads blocks (`world.get-blocks`, level 8).
    assert_eq!(level("world_probe.kmod"), 8);
    // The inventory probe reads its inventory (`inventory.inventory`, level 9).
    assert_eq!(level("inventory_probe.kmod"), 9);
    // The entity probe reads the mobs (`entity.entities`, level 10).
    assert_eq!(level("entity_probe.kmod"), 10);
    // The draw probe draws world shapes (`render.set-world`, level 11).
    assert_eq!(level("draw_probe.kmod"), 11);
    // The light probe reads light (`world.get-lights`, level 12).
    assert_eq!(level("light_probe.kmod"), 12);
    // The gamma probe brightens (`render.set-gamma`, level 13).
    assert_eq!(level("gamma_probe.kmod"), 13);
    // The camera probe moves the camera (`render.set-camera`, level 14).
    assert_eq!(level("camera_probe.kmod"), 14);
    // The self probe reads the player's own effects (`game.get-effects`, level 18).
    assert_eq!(level("self_probe.kmod"), 18);
    // The zoom probe reads the zoom's truth (`render.can-zoom`, level 19).
    assert_eq!(level("zoom_probe.kmod"), 19);
    // The pose probe reads pose states (`entity-pose.get-pose-states`, level 20).
    assert_eq!(level("pose_probe.kmod"), 20);
    // The action probe reads the crosshair's target (`actions.target`, level 21).
    assert_eq!(level("action_probe.kmod"), 21);
    // The screen probe reads the open screen (`screen.state`, level 22).
    assert_eq!(level("screen_probe.kmod"), 22);
    // The item probe swaps an item's model (`item-model.set-model-rules`, level 23).
    assert_eq!(level("item_probe.kmod"), 23);
    // The inventory actions probe moves stacks (`inventory-actions.click`, level 24).
    assert_eq!(level("inventory_actions_probe.kmod"), 24);
}

#[test]
fn vanilla_commands_win() {
    let mods = Mods::load(
        &[mods_dir().join("coords_hud.kmod")],
        |_| true,
        |c| c == "where",
    );
    assert_eq!(mods.commands().collect::<Vec<_>>(), ["coords"]);
}

#[test]
fn a_misbehaving_mod_is_stopped_alone() {
    for (command, reason) in [
        ("spin", "it took too long (an endless loop?)"),
        ("panic", "it crashed (a panic)"),
        ("grow", "it crashed (a panic)"),
    ] {
        let paths = [
            mods_dir().join("coords_hud.kmod"),
            mods_dir().join("horizon_mods_test_guest.kmod"),
        ];
        let mut mods = Mods::load(&paths, |_| true, |_| false);
        let started = Instant::now();
        assert!(mods.command(command));
        assert!(started.elapsed().as_secs() < 5, "/{command} was cut short");
        let stopped = Output::Stopped {
            name: "Test Guest".into(),
            reason: reason.into(),
        };
        assert_eq!(mods.drain(), [stopped], "/{command}");
        assert_eq!(mods.entries()[1].status, Status::Failed(reason.into()));
        // The other mod runs on, and the stopped one's command is gone.
        assert!(mods.is_running());
        assert!(!mods.command("hello"));
        mods.set_snapshot(player());
        mods.tick(1);
        assert_eq!(mods.panels().count(), 1);
    }
}

#[test]
fn hot_reload_restarts_a_changed_mod() {
    let dir = mods_dir().join("reload");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("guest.kmod");
    std::fs::copy(mods_dir().join("horizon_mods_test_guest.kmod"), &path).unwrap();
    let mut mods = Mods::load(std::slice::from_ref(&path), |_| true, |_| false);
    mods.tick(1);
    assert!(!mods.reload_changed(|_| false));
    // A rebuild: same bytes, newer file.
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(later)
        .unwrap();
    assert!(mods.reload_changed(|_| false));
    assert!(mods.command("hello world"));
    let chat = Output::Chat {
        name: "Test Guest".into(),
        text: "hello world after 0 ticks".into(),
    };
    assert_eq!(mods.drain(), [chat]);
}

#[test]
fn storage_persists_across_reload() {
    let dir = mods_dir().join("storage_reload");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("guest.kmod");
    std::fs::copy(mods_dir().join("horizon_mods_test_guest.kmod"), &path).unwrap();
    let mut mods = Mods::load(std::slice::from_ref(&path), |_| true, |_| false);
    assert!(mods.command("save remembered"));
    let saved = Output::Feedback {
        text: "saved".into(),
        error: false,
    };
    assert_eq!(mods.drain(), [saved]);

    // A rebuild (hot_reload_restarts_a_changed_mod's pattern): a fresh Store,
    // but the same on-disk data directory.
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(later)
        .unwrap();
    assert!(mods.reload_changed(|_| false));

    assert!(mods.command("load"));
    let chat = Output::Chat {
        name: "Test Guest".into(),
        text: "remembered".into(),
    };
    assert_eq!(mods.drain(), [chat]);
}

#[test]
fn storage_write_budget_limits_one_call() {
    let dir = mods_dir().join("storage_budget");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("guest.kmod");
    std::fs::copy(mods_dir().join("horizon_mods_test_guest.kmod"), &path).unwrap();
    let mut mods = Mods::load(std::slice::from_ref(&path), |_| true, |_| false);
    // Under the 4 MiB on-disk quota, but over the 1 MiB a single call may
    // write: a loop replacing one key can never bypass the quota by
    // writing a lot in one call, only across many.
    let over_budget = "a".repeat((1 << 20) + 1024);
    assert!(mods.command(&format!("save {over_budget}")));
    let refused = Output::Feedback {
        text: "too much storage traffic in one call".into(),
        error: true,
    };
    assert_eq!(mods.drain(), [refused]);
}

#[test]
fn storage_quota_is_enforced() {
    let dir = mods_dir().join("storage_quota");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("guest.kmod");
    std::fs::copy(mods_dir().join("horizon_mods_test_guest.kmod"), &path).unwrap();
    let mut mods = Mods::load(std::slice::from_ref(&path), |_| true, |_| false);
    // Each call is under the per-call write budget; distinct keys so the
    // quota (not the fixed "note" key being overwritten) is what's tested.
    let chunk = "a".repeat(900 << 10);
    for i in 0..4 {
        assert!(mods.command(&format!("save_key k{i} {chunk}")));
        assert_eq!(
            mods.drain(),
            [Output::Feedback {
                text: "saved".into(),
                error: false,
            }]
        );
    }
    assert!(mods.command(&format!("save_key k4 {chunk}")));
    let refused = Output::Feedback {
        text: "would use over 4 MiB of storage".into(),
        error: true,
    };
    assert_eq!(mods.drain(), [refused]);
}

#[test]
fn storage_key_count_is_bounded() {
    let dir = mods_dir().join("storage_keys");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("guest.kmod");
    std::fs::copy(mods_dir().join("horizon_mods_test_guest.kmod"), &path).unwrap();
    let mut mods = Mods::load(std::slice::from_ref(&path), |_| true, |_| false);
    for i in 0..64 {
        assert!(mods.command(&format!("save_key k{i} x")));
        assert_eq!(
            mods.drain(),
            [Output::Feedback {
                text: "saved".into(),
                error: false,
            }],
            "key {i}"
        );
    }
    assert!(mods.command("save_key k64 x"));
    let refused = Output::Feedback {
        text: "would hold over 64 keys".into(),
        error: true,
    };
    assert_eq!(mods.drain(), [refused]);
}

#[test]
fn duplicate_mod_ids_are_refused() {
    let dir = mods_dir().join("duplicate_ids");
    std::fs::create_dir_all(&dir).unwrap();
    let first = dir.join("first.kmod");
    let second = dir.join("second.kmod");
    std::fs::copy(mods_dir().join("horizon_mods_test_guest.kmod"), &first).unwrap();
    std::fs::copy(mods_dir().join("horizon_mods_test_guest.kmod"), &second).unwrap();
    // Both declare id "test_guest" (the guest's own manifest): sharing a
    // `data_dir` would let the second read, overwrite or delete the
    // first's storage, so only the first-loaded one may start.
    let mods = Mods::load(&[first, second], |_| true, |_| false);
    assert_eq!(mods.entries()[0].status, Status::Running);
    let refused = Status::Failed("another running mod already uses id test_guest".into());
    assert_eq!(mods.entries()[1].status, refused);
}

/// `cargo test -p horizon-mods --release -- --ignored --nocapture`: the
/// cost of the per-tick and per-frame hooks (docs/mods.md, Performance).
#[test]
#[ignore]
fn hook_overhead() {
    let time = |mods: &mut Mods, frame: bool| {
        let n = 200_000;
        for i in 0..1000 {
            mods.tick(i);
        }
        let started = Instant::now();
        for i in 0..n {
            if frame {
                mods.frame(0.016);
            } else {
                mods.tick(i);
            }
        }
        started.elapsed().as_nanos() as f64 / n as f64
    };
    let mut guest = load("horizon_mods_test_guest.kmod");
    let mut hud = load("coords_hud.kmod");
    hud.set_snapshot(player());
    let mut none = Mods::default();
    println!("no mods, tick:        {:8.1} ns", time(&mut none, false));
    println!("empty tick hook:      {:8.1} ns", time(&mut guest, false));
    println!("empty frame hook:     {:8.1} ns", time(&mut guest, true));
    println!("coords-hud tick hook: {:8.1} ns", time(&mut hud, false));
    let started = Instant::now();
    let _ = load("coords_hud.kmod");
    println!(
        "load coords-hud:      {:8.1} ms",
        started.elapsed().as_secs_f64() * 1000.0
    );
}

#[test]
fn ui_screens_settings_and_events() {
    let mut mods = load("horizon_mods_test_ui_guest.kmod");
    assert_eq!(mods.entries()[0].status, Status::Running);
    // The guest declares the `ui` capability, so its row gains a Settings
    // button from load — before it pushes any widgets.
    assert!(mods.entries()[0].has_ui_settings());
    assert!(mods.settings_widgets(0).is_empty());
    // `/ui_screen` pushes the screen; the host clips nothing of this one.
    assert!(mods.command("ui_screen"));
    assert_eq!(mods.drain(), [Output::OpenScreen { owner: 0 }]);
    let screen = mods.screen(0).expect("a pushed screen");
    assert_eq!(screen.title, "Guest's screen");
    assert_eq!(screen.widgets.len(), 3);
    // What the player did comes back through `ui-events`.
    mods.ui_select(0, 7, 2);
    mods.ui_closed(0);
    assert_eq!(
        mods.drain(),
        [
            Output::Chat {
                name: "Test UI Guest".into(),
                text: "selected 7:2".into(),
            },
            Output::Chat {
                name: "Test UI Guest".into(),
                text: "closed".into(),
            },
        ]
    );
    // The widgets themselves: empty until the mod fills them, refreshed
    // on open.
    assert!(mods.command("ui_settings"));
    assert_eq!(mods.settings_widgets(0).len(), 3);
    assert!(mods.entries()[0].has_ui_settings());
    mods.ui_settings_opened(0);
    assert_eq!(mods.settings_widgets(0).len(), 2);
    // `/ui_close` closes the mod's own screen.
    assert!(mods.command("ui_close"));
    assert!(mods.drain().contains(&Output::CloseScreen { owner: 0 }));
    assert!(mods.screen(0).is_none());
}

#[test]
fn settings_button_follows_the_ui_capability() {
    // A running keyless mod that declares the `ui` capability gets its
    // Settings button before it pushes any widgets: it only fills the
    // page in `ui_settings_opened`, which fires when the page opens.
    let mods = load("horizon_mods_test_ui_guest.kmod");
    assert_eq!(mods.entries()[0].status, Status::Running);
    assert!(mods.settings_widgets(0).is_empty());
    assert!(mods.entries()[0].has_ui_settings());
    // A running mod without the capability, keys or widgets gets none:
    // Coordinates HUD is a pure HUD mod.
    let hud = load("coords_hud.kmod");
    assert_eq!(hud.entries()[0].status, Status::Running);
    assert!(!hud.entries()[0].has_ui_settings());
}

#[test]
fn render_shapes_and_shakes() {
    let mut mods = load("horizon_mods_test_ui_guest.kmod");
    mods.set_snapshot(Snapshot {
        gui_size: (427.0, 240.0),
        ..Snapshot::default()
    });
    assert_eq!(mods.hud_shapes().count(), 0);
    assert!(mods.command("render_hud"));
    let shapes: Vec<_> = mods.hud_shapes().cloned().collect();
    assert_eq!(shapes.len(), 5);
    // The guest read `gui-size`; text is clipped to 256 characters.
    let Shape::Text(t) = &shapes[4] else {
        panic!("{:?}", shapes[4])
    };
    assert!(t.text.starts_with("gui 427x240 "), "{}", t.text);
    assert_eq!(t.text.chars().count(), 256);
    assert!(mods.command("render_clear"));
    assert_eq!(mods.hud_shapes().count(), 0);
    // One jolt, clamped; zero and NaN shake nothing.
    mods.drain();
    assert!(mods.command("render_shake"));
    assert_eq!(
        mods.drain(),
        [Output::Shake {
            strength: 1.0,
            seconds: 10.0
        }]
    );
}

#[test]
fn input_claims_keys_and_events() {
    let mut mods = load("horizon_mods_test_ui_guest.kmod");
    let chat = |mods: &mut Mods| -> Vec<String> {
        mods.drain()
            .into_iter()
            .filter_map(|o| match o {
                Output::Chat { text, .. } => Some(text),
                _ => None,
            })
            .collect()
    };
    assert!(!mods.claims_any() && !mods.has_keys());
    assert!(mods.command("input_claim"));
    let said = chat(&mut mods);
    assert!(
        said[0].starts_with("claimed; \"R\" is not a key"),
        "{said:?}"
    );
    assert!(mods.claims("minecraft:stick") && !mods.claims("minecraft:stone"));
    let keys: Vec<_> = mods
        .mod_keys()
        .map(|(m, k)| (m.to_owned(), k.key.clone()))
        .collect();
    assert_eq!(
        keys,
        [("Test UI Guest".to_owned(), "key.keyboard.r".to_owned())]
    );
    // The game feeds the events; the mod reads them in order with `poll`.
    let stick = HeldItem {
        id: "minecraft:stick".into(),
        count: 3,
        slot: 2,
    };
    mods.use_start(&stick);
    mods.key("key.keyboard.r", true);
    mods.key("key.keyboard.t", true);
    mods.key("key.keyboard.r", false);
    mods.use_stop(&stick, 12);
    assert!(mods.command("input_poll"));
    assert_eq!(
        chat(&mut mods),
        [
            "use-start minecraft:stick 0",
            "key-down wheel",
            "key-up wheel",
            "use-stop minecraft:stick 12"
        ]
    );
    // `held` reads the snapshot.
    mods.set_snapshot(Snapshot {
        held: Some(stick),
        ..Snapshot::default()
    });
    assert!(mods.command("input_held"));
    assert_eq!(chat(&mut mods), ["holding minecraft:stick x3 in 2"]);
    // A screen opening lets held keys up, once; the release that follows
    // is not sent again.
    mods.key("key.keyboard.r", true);
    mods.release_keys();
    mods.release_keys();
    mods.key("key.keyboard.r", false);
    assert!(mods.command("input_poll"));
    assert_eq!(chat(&mut mods), ["key-down wheel", "key-up wheel"]);
}

#[test]
fn the_players_key_choice_wins() {
    let chat = |mods: &mut Mods| -> Vec<String> {
        mods.drain()
            .into_iter()
            .filter_map(|o| match o {
                Output::Chat { text, .. } => Some(text),
                _ => None,
            })
            .collect()
    };
    let mut mods = load("horizon_mods_test_ui_guest.kmod");
    assert!(mods.command("input_claim"));
    // Its keys alone give it a page in Options > Mods, where the game lists them.
    assert!(mods.settings_widgets(0).is_empty() && mods.entries()[0].has_ui_settings());
    // Rebinding while held lets the old key go.
    mods.key("key.keyboard.r", true);
    assert!(mods.rebind(0, "wheel", "key.mouse.4"));
    assert_eq!(mods.keys_of(0)[0].key, "key.mouse.4");
    mods.key("key.keyboard.r", false);
    mods.key("key.mouse.4", true);
    assert!(!mods.rebind(0, "wheel", "R") && !mods.rebind(0, "nope", "key.keyboard.g"));
    chat(&mut mods);
    assert!(mods.command("input_poll"));
    assert_eq!(
        chat(&mut mods),
        ["key-down wheel", "key-up wheel", "key-down wheel"]
    );
    // A choice saved earlier replaces the default, registered before or after.
    let mut mods = load("horizon_mods_test_ui_guest.kmod");
    mods.set_bindings(|_| vec![("wheel", "key.keyboard.g"), ("bad", "G")]);
    assert!(mods.command("input_claim"));
    assert_eq!(mods.keys_of(0)[0].key, "key.keyboard.g");
    mods.set_bindings(|_| vec![("wheel", "key.keyboard.h")]);
    assert_eq!(mods.keys_of(0)[0].key, "key.keyboard.h");
}

#[test]
fn input_wheel_is_the_mods_while_claimed() {
    let mut mods = load("horizon_mods_test_ui_guest.kmod");
    let chat = |mods: &mut Mods| -> Vec<String> {
        mods.drain()
            .into_iter()
            .filter_map(|o| match o {
                Output::Chat { text, .. } => Some(text),
                _ => None,
            })
            .collect()
    };
    // Unclaimed, the hotbar keeps the wheel and the mod hears nothing.
    assert!(!mods.wheel_claimed() && !mods.wheel(1.0));
    assert!(mods.command("input_wheel"));
    assert!(mods.wheel_claimed());
    assert!(mods.wheel(1.0) && mods.wheel(-2.0));
    assert!(mods.command("input_poll"));
    assert_eq!(chat(&mut mods), ["wheel 1", "wheel -2"]);
    assert!(mods.command("input_wheel off"));
    assert!(!mods.wheel(1.0));
}

#[test]
fn render_zoom_is_clamped_and_the_strongest_counts() {
    let mut mods = load("horizon_mods_test_ui_guest.kmod");
    let zoom = |mods: &mut Mods, args: &str| {
        assert!(mods.command(&format!("zoom {args}")));
        mods.zoom().map(|z| (z.factor, z.smooth, z.turn))
    };
    assert!(mods.zoom().is_none());
    assert_eq!(zoom(&mut mods, "4 1 0.25"), Some((4.0, true, 0.25)));
    // 50 passes since API 19 widened the cap; over 250 and a turn
    // under 0.05 are clamped.
    assert_eq!(zoom(&mut mods, "50 0 0"), Some((50.0, false, 0.05)));
    assert_eq!(zoom(&mut mods, "300 0 0"), Some((250.0, false, 0.05)));
    // A NaN turn keeps the player's sensitivity.
    assert_eq!(zoom(&mut mods, "2 0 x"), Some((2.0, false, 1.0)));
    // 1, under 1 and NaN end the zoom.
    assert_eq!(zoom(&mut mods, "1 0 1"), None);
    assert_eq!(zoom(&mut mods, "0.5 0 1"), None);
    assert_eq!(zoom(&mut mods, "x 0 1"), None);
}

#[test]
fn render_minimap_is_clamped_and_hidden() {
    let mut mods = load("horizon_mods_test_ui_guest.kmod");
    let map = |mods: &mut Mods, args: &str| {
        assert!(mods.command(&format!("minimap {args}")));
        mods.minimap()
            .map(|m| (m.corner, m.size, m.blocks_per_pixel, m.rotate, m.round))
    };
    assert!(mods.minimap().is_none());
    let corner = horizon_mods::RenderCorner::BottomLeft;
    assert_eq!(
        map(&mut mods, "96 2 1 0"),
        Some((corner, 96.0, 2.0, true, false))
    );
    // The side and the scale are clamped; NaN takes the smallest.
    assert_eq!(
        map(&mut mods, "999 0.1 0 1"),
        Some((corner, 256.0, 0.5, false, true))
    );
    assert_eq!(
        map(&mut mods, "x 9 0 0"),
        Some((corner, 16.0, 4.0, false, false))
    );
    assert_eq!(map(&mut mods, "off"), None);
}

#[test]
fn render_far_terrain_is_clamped_and_ended() {
    let mut mods = load("horizon_mods_test_ui_guest.kmod");
    let far = |mods: &mut Mods, args: &str| {
        assert!(mods.command(&format!("far_terrain {args}")));
        mods.far_terrain().map(|f| (f.distance, f.reduced))
    };
    assert!(mods.far_terrain().is_none());
    assert_eq!(far(&mut mods, "256 1"), Some((256, true)));
    // The distance is clamped to LOD Distance's 64 to 1024 chunks.
    assert_eq!(far(&mut mods, "8 0"), Some((64, false)));
    assert_eq!(far(&mut mods, "99999 0"), Some((1024, false)));
    assert_eq!(far(&mut mods, "off"), None);
}

/// `game.raycast`: the view's hit the game worked out, up to the distance
/// asked for; the game works it out only while a mod imports the call.
#[test]
fn game_raycast_sees_the_view_hit_within_its_distance() {
    let mut mods = load("horizon_mods_test_ui_guest.kmod");
    assert!(mods.wants_raycast());
    assert!(!load("coords_hud.kmod").wants_raycast());
    let said = |mods: &mut Mods, args: &str| -> Vec<String> {
        assert!(mods.command(&format!("raycast {args}")));
        mods.drain()
            .into_iter()
            .filter_map(|o| match o {
                Output::Chat { text, .. } => Some(text),
                _ => None,
            })
            .collect()
    };
    assert_eq!(said(&mut mods, "100"), ["no hit"]);
    mods.set_snapshot(Snapshot {
        view_hit: Some((
            40.5,
            Hit {
                block_x: 12,
                block_y: 63,
                block_z: -44,
                x: 12.5,
                y: 64.0,
                z: -43.75,
                block: "minecraft:grass_block".into(),
            },
        )),
        ..player()
    });
    let hit = "hit minecraft:grass_block at 12 63 -44 (12.5 64.0 -43.8)";
    assert_eq!(said(&mut mods, "100"), [hit]);
    assert_eq!(said(&mut mods, "40"), ["no hit"]);
    // NaN asks for nothing; past 1024 is 1024.
    assert_eq!(said(&mut mods, "x"), ["no hit"]);
    assert_eq!(said(&mut mods, "1e9"), [hit]);
}

#[test]
fn quiet_commands_come_out_as_one_batch_capped() {
    let mut mods = load("horizon_mods_test_ui_guest.kmod");
    let quiet = |mods: &mut Mods, command: &str| -> Vec<(String, Vec<String>)> {
        assert!(mods.command(command));
        mods.drain()
            .into_iter()
            .filter_map(|o| match o {
                Output::QuietCommands { mod_id, lines } => Some((mod_id, lines)),
                _ => None,
            })
            .collect()
    };
    assert_eq!(
        quiet(&mut mods, "quiet setblock 0 0 0 stone;say hi"),
        [(
            "test_ui_guest".to_string(),
            vec!["setblock 0 0 0 stone".to_string(), "say hi".to_string()]
        )]
    );
    let batches = quiet(&mut mods, "quiet_many 300");
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].1.len(), 256);
    assert_eq!(batches[0].1[255], "setblock 255 300 0 stone");
}

#[test]
fn corner_panels_text_width_and_readings() {
    let mut mods = load("horizon_mods_test_ui_guest.kmod");
    // A font of 5-pixel glyphs, 3-pixel spaces.
    let mut widths = vec![5.0f32; 0x250];
    widths[' ' as usize] = 3.0;
    mods.set_snapshot(Snapshot {
        frame: (60, 41, 52),
        mappings: vec![("key.forward", "W".into(), true)],
        cps: (7, 2),
        font_widths: widths.into(),
        ..Snapshot::default()
    });
    assert!(mods.command("corner"));
    let panels: Vec<_> = mods.corner_panels().cloned().collect();
    assert_eq!(panels.len(), 1);
    // "60 fps": five glyphs and a space, plus the box's 4.
    assert_eq!(
        (panels[0].width, panels[0].height),
        (5.0 * 5.0 + 3.0 + 4.0, 12.0)
    );
    assert_eq!(panels[0].shapes.len(), 2);
    assert!(matches!(
        panels[0].corner,
        horizon_mods::RenderCorner::TopRight
    ));
    assert!(matches!(
        &mods.settings_widgets(0)[0],
        horizon_mods::UiWidget::Button(b) if b.id == 3 && b.label == "Corner: Top Right"
    ));
    mods.drain();
    assert!(mods.command("readings"));
    assert_eq!(
        mods.drain(),
        [Output::Chat {
            name: "Test UI Guest".into(),
            text: "60 41 52; forward W true; cps 7 2; other true".into(),
        }]
    );
}

#[test]
fn content_only_mods_load_without_an_engine() {
    let manifest = r#"
id = "ruby"
name = "Ruby"
version = "0.1.0"
api = "0.2"
side = "both"
capabilities = ["registries"]
"#;
    let block: &[u8] = br#"{"template": "minecraft:iron_block"}"#;
    let dir = mods_dir().join("content");
    std::fs::create_dir_all(&dir).unwrap();
    let ok = dir.join("ruby.kmod");
    let files: &[(&str, &[u8])] = &[("data/ruby/horizon/block/ruby_block.json", block)];
    std::fs::write(&ok, Package::write_files(manifest, None, files).unwrap()).unwrap();
    let bad = dir.join("bad.kmod");
    let files: &[(&str, &[u8])] = &[("data/minecraft/recipe/stone.json", b"{}")];
    let bad_manifest = manifest.replace("\"ruby\"", "\"bad\"");
    std::fs::write(
        &bad,
        Package::write_files(&bad_manifest, None, files).unwrap(),
    )
    .unwrap();
    let mods = Mods::load(&[ok, bad], |_| true, |_| false);
    assert_eq!(mods.entries()[0].status, Status::Running);
    assert!(matches!(&mods.entries()[1].status, Status::Failed(e) if e.contains("own namespace")));
    // Content has no hooks: the game's per-tick mod work stays skipped.
    assert!(!mods.is_running());
}

/// `cargo horizon-mod test` on the examples: their own `tests/*.toml` pass.
#[test]
fn the_examples_test_scripts_pass() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../horizon-mod-sdk/examples");
    let mut ran = 0;
    let examples_and_files = [
        ("coords-hud", "coords_hud"),
        ("welcome", "welcome"),
        ("beacons", "beacons"),
        ("world-probe", "world_probe"),
        ("inventory-probe", "inventory_probe"),
        ("entity-probe", "entity_probe"),
        ("draw-probe", "draw_probe"),
        ("light-probe", "light_probe"),
        ("gamma-probe", "gamma_probe"),
    ];
    for (example, kmod) in examples_and_files {
        for entry in std::fs::read_dir(examples.join(example).join("tests")).unwrap() {
            let path = entry.unwrap().path();
            let script = Script::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
            let mut game = TestGame::load(&mods_dir().join(format!("{kmod}.kmod"))).unwrap();
            script
                .run(&mut game)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            ran += 1;
        }
    }
    assert_eq!(ran, 12);
}

/// Capability `world` without a world lent (the registry's trial run):
/// `get-block` finds nothing, `set-blocks` checks its count and queues.
#[test]
fn a_server_mod_without_a_world_reads_nothing() {
    let mut game = TestGame::load(&mods_dir().join("beacons.kmod")).unwrap();
    game.join("Alex");
    let alex = game.players[0].clone();
    assert!(game.player_command(&alex, "ground"));
    let out = game.drain();
    assert!(
        out.iter()
            .any(|o| matches!(o, Output::Reply { text, error: true, .. }
            if text == "That block isn't loaded")),
        "{out:?}"
    );
}

#[test]
fn a_test_script_says_which_step_failed_and_why() {
    let script = Script::parse(
        r#"
        [[step]]
        ticks = 3
        command = "hello there"
        expect.chat = ["hello there after 2 ticks"]
        "#,
    )
    .unwrap();
    let mut game = TestGame::load(&mods_dir().join("horizon_mods_test_guest.kmod")).unwrap();
    assert_eq!(
        script.run(&mut game).unwrap_err(),
        "step 1 (3 ticks, /hello there): chat\n    expected: [\"hello there after 2 ticks\"]\n    \
         got:      [\"hello there after 3 ticks\"]"
    );
    assert!(
        Script::parse("[[step]]\ntick = 1").is_err(),
        "a typo is an error"
    );

    let mut game = TestGame::load(&mods_dir().join("welcome.kmod")).unwrap();
    let script = "[[step]]\njoin = \"Alex\"\n\n[[step]]\nas = \"Steve\"\ncommand = \"mined\"";
    assert_eq!(
        Script::parse(script).unwrap().run(&mut game).unwrap_err(),
        "step 2 (/mined): Steve is not on the server"
    );
}

#[test]
fn a_test_script_reloads_and_expects_a_stop() {
    let script = Script::parse(
        r#"
        [[step]]
        command = "save kept"
        expect.feedback = "saved"

        [[step]]
        reload = true
        command = "load"
        expect.chat = ["kept"]

        [[step]]
        command = "panic"
        expect.stopped = "panic"

        [[step]]
        reload = true
        command = "nope"
        "#,
    )
    .unwrap();
    let mut game = TestGame::load(&mods_dir().join("horizon_mods_test_guest.kmod")).unwrap();
    let dir = game.kmod().parent().unwrap().to_path_buf();
    assert_eq!(
        script.run(&mut game).unwrap_err(),
        "step 4 (reload, /nope): the mod owns no command /nope"
    );
    drop(game);
    assert!(
        !dir.exists(),
        "the scratch copy and its storage are removed"
    );
}

fn alex() -> ServerPlayer {
    ServerPlayer {
        id: "00000000-0000-4000-8000-000000000001".into(),
        name: "Alex".into(),
        x: 0.5,
        y: 64.0,
        z: 0.5,
        dimension: "minecraft:the_nether".into(),
        game_mode: GameMode::Survival,
        health: 20.0,
    }
}

#[test]
fn a_server_mod_runs_only_where_the_world_does() {
    // A fresh folder: what the mod stored in an earlier run is beside it.
    let dir = mods_dir().join("server");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("welcome.kmod");
    std::fs::copy(mods_dir().join("welcome.kmod"), &path).unwrap();
    let paths = std::slice::from_ref(&path);
    let window = Mods::load_for(Runs::Client, paths, |_| true, |_| false);
    assert_eq!(window.entries()[0].status, Status::Elsewhere);
    assert!(!window.is_running());

    let mut mods = Mods::load_for(Runs::Server, paths, |_| true, |c| c == "give");
    assert!(mods.runs_server());
    let commands: Vec<_> = mods.server_commands().map(|c| c.name.as_str()).collect();
    assert_eq!(commands, ["online", "mined"]);
    // `/mined [player]`: its argument travels with it, for the command tree.
    let mined = mods.server_commands().nth(1).unwrap();
    assert_eq!(mined.arguments[0].kind, horizon_mods::ArgumentKind::Player);
    assert!(mined.arguments[0].optional);
    assert_eq!(mods.commands().count(), 0, "not the window's chat commands");
    let alex = alex();
    mods.set_snapshot(Snapshot {
        players: vec![alex.clone()],
        ..Snapshot::default()
    });
    mods.player_joined(&alex);
    let greeted = [
        Output::Tell {
            name: "Welcome".into(),
            player: alex.id.clone(),
            text: "Welcome, Alex! 0 other player(s) online.".into(),
        },
        Output::Broadcast {
            name: "Welcome".into(),
            text: "Alex is here for the first time".into(),
        },
    ];
    assert_eq!(mods.drain(), greeted);
    let stone = BlockPos { x: 1, y: 63, z: 0 };
    mods.block_broken(&alex, stone, "minecraft:stone");
    assert!(mods.server_command(&alex, "mined"));
    assert!(mods.server_command(&alex, "online"));
    let reply = |text: &str| Output::Reply {
        player: alex.id.clone(),
        text: text.into(),
        error: false,
    };
    assert_eq!(
        mods.drain(),
        [reply("You broke 1 block(s)"), reply("Alex (the_nether)")]
    );
    assert!(!mods.server_command(&alex, "where"));
    assert!(
        !mods.command("online"),
        "a server command isn't the window's"
    );
}

/// Capability `recipes`: the guest reads tables as big as vanilla's (1,500
/// items, 1,600 recipes of nine five-item slots) within one command's fuel.
#[test]
fn recipes_reads_the_tables() {
    use horizon_mods::{Recipe, RecipeItem, RecipeKind, RecipeStack, Recipes};
    let mut mods = load("horizon_mods_test_ui_guest.kmod");
    let stack = |item| RecipeStack {
        item,
        count: 1,
        components: String::new(),
    };
    let items = (0..1500)
        .map(|i| RecipeItem {
            id: format!("minecraft:item_{i}"),
            name: format!("Item {i}"),
            tags: vec!["planks".into(), "logs".into()],
        })
        .collect();
    let mut recipes: Vec<Recipe> = (0..1600)
        .map(|i| Recipe {
            id: format!("minecraft:recipe_{i}"),
            kind: RecipeKind::CraftingShaped,
            category: "crafting_misc".into(),
            group: String::new(),
            width: 3,
            height: 3,
            slots: (0..9)
                .map(|k| (0..5).map(|j| stack((i + k + j) % 1500)).collect())
                .collect(),
            results: vec![RecipeStack {
                item: 7,
                count: 4,
                components: String::new(),
            }],
            cooking_time: 0,
            experience: 0.0,
            book_id: i as i32,
        })
        .collect();
    recipes[1].kind = RecipeKind::Brewing;
    recipes[1].results[0].components =
        "[minecraft:potion_contents={potion:\"minecraft:swiftness\"}]".into();
    let mods_recipes = Some(std::sync::Arc::new(Recipes { items, recipes }));
    mods.set_snapshot(Snapshot {
        recipes: mods_recipes.clone(),
        known_recipes: vec![1, 5, 9].into(),
        ..Snapshot::default()
    });
    let said = |mods: &mut Mods, command: &str| {
        assert!(mods.command(command));
        mods.drain()
            .into_iter()
            .filter_map(|o| match o {
                Output::Chat { text, .. } => Some(text),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        said(&mut mods, "recipes"),
        ["1500 items, 1600 recipes, 3 known, 1 brewing"]
    );
    assert_eq!(
        said(&mut mods, "recipes minecraft:recipe_1"),
        [
            "minecraft:recipe_1 Kind::Brewing 3x3 0t slots 9, the first 1 minecraft:item_1 -> \
          4 minecraft:item_7[minecraft:potion_contents={potion:\"minecraft:swiftness\"}] known true"
        ]
    );
    assert!(
        mods.entries()
            .iter()
            .all(|e| e.status == horizon_mods::Status::Running),
        "it ran out of fuel"
    );
    // Actions: refused without a crafting screen, for an unknown recipe,
    // and outside creative mode.
    assert_eq!(
        said(&mut mods, "recipe_place minecraft:recipe_5"),
        ["place minecraft:recipe_5: no screen that takes recipes is open"]
    );
    let mut snap = Snapshot {
        recipes: mods_recipes.clone(),
        known_recipes: vec![1, 5, 9].into(),
        placing: true,
        ..Snapshot::default()
    };
    mods.set_snapshot(snap.clone());
    assert_eq!(
        said(&mut mods, "recipe_place minecraft:recipe_6"),
        ["place minecraft:recipe_6: recipe 6 is not in the player's recipe book"]
    );
    assert!(mods.command("recipe_place minecraft:recipe_5 all"));
    assert_eq!(
        mods.drain()[0],
        Output::PlaceRecipe {
            book_id: 5,
            all: true
        }
    );
    assert_eq!(
        said(&mut mods, "recipe_give minecraft:item_7 3"),
        ["give minecraft:item_7: only in creative mode"]
    );
    snap.creative = true;
    mods.set_snapshot(snap);
    assert!(mods.command("recipe_give minecraft:item_7 3"));
    assert_eq!(
        mods.drain()[0],
        Output::Give {
            item: "minecraft:item_7".into(),
            count: 3,
            components: String::new()
        }
    );
    // Without the game's tables (no mod read them yet): empty, not a trap.
    mods.set_snapshot(Snapshot::default());
    assert_eq!(
        said(&mut mods, "recipes"),
        ["0 items, 0 recipes, 0 known, 0 brewing"]
    );
}

#[test]
fn browser_lists_items_and_hears_the_player() {
    use horizon_mods::{BrowserClick, BrowserEvent, BrowserStack};
    let mut mods = load("horizon_mods_test_ui_guest.kmod");
    let said = |mods: &mut Mods, command: &str| {
        assert!(mods.command(command));
        mods.drain()
            .into_iter()
            .filter_map(|o| match o {
                Output::Chat { text, .. } => Some(text),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert!(mods.browser_items().is_none());
    let potion = "[minecraft:potion_contents={potion:\"minecraft:swiftness\"}]";
    assert_eq!(
        said(
            &mut mods,
            &format!("browser minecraft:stone minecraft:potion{potion}")
        ),
        ["panel of 2"]
    );
    let (owner, version, items) = mods.browser_items().unwrap();
    assert_eq!(
        (items[0].item.as_str(), items[1].components.as_str()),
        ("minecraft:stone", potion)
    );
    mods.browser_event(
        owner,
        BrowserEvent::Clicked(BrowserClick {
            index: 1,
            right: true,
        }),
    );
    mods.browser_event(owner, BrowserEvent::Search("oak".into()));
    let log = BrowserStack {
        item: "minecraft:oak_log".into(),
        count: 3,
        components: String::new(),
    };
    // R is the guest's `recipe`; X is none of its keys.
    mods.browser_key("key.keyboard.r", &log);
    mods.browser_key("key.keyboard.x", &log);
    assert_eq!(
        said(&mut mods, "browser_poll"),
        [
            "3 events",
            "clicked 1 right",
            "search \"oak\"",
            "key recipe on minecraft:oak_log x3"
        ]
    );
    // Each `set-items` is a new version; an empty list hides the panel.
    said(&mut mods, "browser minecraft:dirt");
    assert_ne!(mods.browser_items().unwrap().1, version);
    assert_eq!(said(&mut mods, "browser"), ["panel of 0"]);
    assert!(mods.browser_items().is_none());
    // Recipes in the panel's place: the guest shows those making planks.
    use horizon_mods::{BrowserLayout, Recipe, RecipeItem, RecipeKind, RecipeStack, Recipes};
    let item = |id: &str| RecipeItem {
        id: id.into(),
        name: id.into(),
        tags: Vec::new(),
    };
    let stack = |item| RecipeStack {
        item,
        count: 1,
        components: String::new(),
    };
    mods.set_snapshot(Snapshot {
        recipes: Some(std::sync::Arc::new(Recipes {
            items: vec![item("minecraft:oak_log"), item("minecraft:oak_planks")],
            recipes: vec![Recipe {
                id: "minecraft:oak_planks".into(),
                kind: RecipeKind::CraftingShapeless,
                category: "building".into(),
                group: "planks".into(),
                width: 0,
                height: 0,
                slots: vec![vec![stack(0)]],
                results: vec![RecipeStack {
                    count: 4,
                    ..stack(1)
                }],
                cooking_time: 0,
                experience: 0.0,
                book_id: 1,
            }],
        })),
        ..Snapshot::default()
    });
    said(&mut mods, "browser minecraft:dirt");
    assert_eq!(
        said(&mut mods, "browser_recipes minecraft:oak_planks"),
        ["recipes shown: 1"]
    );
    let page = mods.browser_recipes(owner).unwrap();
    assert_eq!(page.title, "Recipes for minecraft:oak_planks");
    assert_eq!(
        (page.tabs[0].item.as_str(), page.tab),
        ("minecraft:crafting_table", 0)
    );
    let r = &page.recipes[0];
    assert_eq!(
        (r.layout, r.width, r.height, r.action),
        (BrowserLayout::Grid, 1, 1, Some(false))
    );
    assert_eq!(
        (r.slots[0][0].item.as_str(), r.results[0].count),
        ("minecraft:oak_log", 4)
    );
    assert_eq!(said(&mut mods, "browser_recipes"), ["recipes shown: 0"]);
    assert!(mods.browser_recipes(owner).is_none());
    // At most 256 events wait for a poll: the oldest go.
    for _ in 0..300 {
        mods.browser_event(owner, BrowserEvent::Page(1));
    }
    assert_eq!(said(&mut mods, "browser_poll")[0], "256 events");
}
