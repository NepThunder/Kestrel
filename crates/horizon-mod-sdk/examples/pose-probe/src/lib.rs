//! Prototype probe for entity poses (API 20): each tick it reads
//! the pose states (`entity-pose.get-pose-states`), shows the
//! local player's swing in a HUD panel, and mirrors that swing
//! onto the model's own arm with a pose offset
//! (`entity-pose.set-pose-offset`): the arm's pitch and roll
//! follow the swing's progress, so the body the world draws
//! swings as the state says. While no swing runs it clears its
//! offset, handing the arm back to vanilla.
use horizon_mod_sdk::{
    Mod, PartOffset, PoseOffset, PoseTarget, Registration,
    entity_pose::{self, SwingAnim},
    hud::{self, Corner, Line},
};

struct PoseProbe {
    last_panel: Vec<String>,
}

fn anim_name(anim: SwingAnim) -> &'static str {
    match anim {
        SwingAnim::None => "none",
        SwingAnim::Whack => "whack",
        SwingAnim::Stab => "stab",
    }
}

impl Mod for PoseProbe {
    fn init() -> Result<(Self, Registration), String> {
        Ok((
            PoseProbe {
                last_panel: Vec::new(),
            },
            Registration::new().on_tick(),
        ))
    }

    fn tick(&mut self, _tick: u64) {
        let states = entity_pose::get_pose_states();
        let Some(me) = states.iter().find(|s| s.self_) else {
            if !self.last_panel.is_empty() {
                hud::set_panel(Corner::TopLeft, &[]);
                self.last_panel.clear();
            }
            return;
        };
        let panel = vec![
            format!(
                "swing {:.2} {}",
                me.swing_progress,
                anim_name(me.swing_anim)
            ),
            format!("players {}", states.len()),
        ];
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
        // The mirror: the swinging arm's offset is the swing,
        // scaled by its progress — pitch down, a little roll.
        if me.swing_progress > 0.0 {
            let arm = if me.swing_off_hand {
                "left_arm"
            } else {
                "right_arm"
            };
            entity_pose::set_pose_offset(
                PoseTarget::SelfPlayer,
                &PoseOffset {
                    parts: vec![PartOffset {
                        part: arm.into(),
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                        pitch: -90.0 * me.swing_progress,
                        yaw: 0.0,
                        roll: 20.0 * me.swing_progress,
                    }],
                    hidden: Vec::new(),
                },
            );
        } else {
            entity_pose::clear_pose_offset(PoseTarget::SelfPlayer);
        }
    }
}

horizon_mod_sdk::export_mod!(PoseProbe);
