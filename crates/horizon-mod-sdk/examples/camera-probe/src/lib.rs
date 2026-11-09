//! Prototype probe for the mod camera (`docs/mods.md`, "Camera"): each tick
//! it shows the effective camera pose and the projected screen position of a
//! fixed world point in a corner panel, and draws a marker at that projected
//! position. `/cam-free` detaches the look (freelook) and the mouse turns the
//! camera while the player keeps walking straight; `/cam-back` detaches the
//! camera 8 blocks behind the player (freecam) and the mouse orbits it;
//! `/cam-orbit` is the other call pattern, a scripted camera the mod moves
//! every tick (the mouse does not move it); `/cam-roll` banks the view;
//! `/cam-far` proves a position 200 blocks out stops at the 64-block cap;
//! `/cam-nan` proves a non-finite position drops the call; `/cam-off`
//! releases the camera, and reloading the mod restores the player's own.
use horizon_mod_sdk::{
    Mod, Registration,
    game::{self, Player},
    hud::{self, Corner, Line},
    render::{self, CameraOverride, Point, Shape, TextShape, WorldPoint},
};

/// The player's eye above their feet, as the game puts the camera.
const EYE: f64 = 1.62;
/// A freecam's distance behind the player, in blocks.
const BACK: f64 = 8.0;
/// A scripted orbit's turn per tick, in degrees.
const ORBIT_STEP: f32 = 6.0;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Off,
    Freelook,
    Freecam,
    Orbit,
}

struct CameraProbe {
    mode: Mode,
    roll: f32,
    orbit_tick: u64,
    anchor: Option<WorldPoint>,
    last_panel: Vec<String>,
}

/// The effective camera pose, one decimal a field.
fn pose_line() -> String {
    let p = render::get_camera();
    format!(
        "[Camera Probe] pose x={:.1} y={:.1} z={:.1} yaw={:.1} pitch={:.1} roll={:.1}",
        p.x, p.y, p.z, p.yaw, p.pitch, p.roll
    )
}

/// The anchor's projected screen position, one decimal a field.
fn anchor_line(anchor: WorldPoint) -> String {
    let s = render::project(anchor);
    format!(
        "anchor x={:.1} y={:.1} visible={} depth={:.1}",
        s.x, s.y, s.visible, s.depth
    )
}

/// The anchor: the player's eye, 10 blocks along their look.
fn anchor_from(p: &Player) -> WorldPoint {
    let (yaw, pitch) = (
        f64::from(p.yaw).to_radians(),
        f64::from(p.pitch).to_radians(),
    );
    let forward = [
        -yaw.sin() * pitch.cos(),
        -pitch.sin(),
        yaw.cos() * pitch.cos(),
    ];
    WorldPoint {
        x: p.x + forward[0] * 10.0,
        y: p.y + EYE + forward[1] * 10.0,
        z: p.z + forward[2] * 10.0,
    }
}

fn parse_numbers(args: &str) -> Result<Vec<f32>, String> {
    args.split_whitespace()
        .map(|n| n.parse().map_err(|_| "give numbers".to_string()))
        .collect()
}

impl CameraProbe {
    /// Detach with this look: freelook with no position, freecam with one.
    fn set(&mut self, mode: Mode, position: Option<WorldPoint>, yaw: f32, pitch: f32) {
        self.mode = mode;
        render::set_camera(Some(CameraOverride {
            position,
            yaw,
            pitch,
            roll: self.roll,
        }));
    }

    /// The camera 8 blocks behind the player at eye height.
    fn behind(p: &Player) -> WorldPoint {
        let yaw = f64::from(p.yaw).to_radians();
        WorldPoint {
            x: p.x + yaw.sin() * BACK,
            y: p.y + EYE,
            z: p.z - yaw.cos() * BACK,
        }
    }
}

impl Mod for CameraProbe {
    fn init() -> Result<(Self, Registration), String> {
        let hooks = Registration::new()
            .command("cam")
            .command("cam-free")
            .command("cam-back")
            .command("cam-orbit")
            .command("cam-roll")
            .command("cam-far")
            .command("cam-nan")
            .command("cam-off")
            .command("cam-project")
            .on_tick();
        Ok((
            CameraProbe {
                mode: Mode::Off,
                roll: 0.0,
                orbit_tick: 0,
                anchor: None,
                last_panel: Vec::new(),
            },
            hooks,
        ))
    }

    fn tick(&mut self, _tick: u64) {
        let Some(p) = game::get_player() else {
            if !self.last_panel.is_empty() {
                hud::clear();
                self.last_panel.clear();
            }
            render::set_hud(&[]);
            return;
        };
        if self.anchor.is_none() {
            self.anchor = Some(anchor_from(&p));
        }
        // The scripted pattern: a call every tick moves the camera, and
        // the mouse does not move it. The other modes call only on
        // change, and the mouse turns the camera instead.
        if self.mode == Mode::Orbit {
            self.orbit_tick += 1;
            let angle = self.orbit_tick as f32 * ORBIT_STEP;
            let a = f64::from(angle).to_radians();
            render::set_camera(Some(CameraOverride {
                position: Some(WorldPoint {
                    x: p.x + a.sin() * BACK,
                    y: p.y + EYE,
                    z: p.z - a.cos() * BACK,
                }),
                yaw: angle,
                pitch: 0.0,
                roll: self.roll,
            }));
        }
        let pose = render::get_camera();
        let panel = match self.anchor {
            Some(anchor) => vec![
                format!(
                    "cam x={:.1} y={:.1} z={:.1} yaw={:.1} pitch={:.1} roll={:.1}",
                    pose.x, pose.y, pose.z, pose.yaw, pose.pitch, pose.roll
                ),
                anchor_line(anchor),
            ],
            None => vec![],
        };
        if panel != self.last_panel {
            self.last_panel = panel.clone();
            if panel.is_empty() {
                hud::clear();
            } else {
                hud::set_panel(
                    Corner::TopLeft,
                    &panel
                        .iter()
                        .map(|text| Line {
                            text: text.clone(),
                            color: 0xFFFFFF,
                        })
                        .collect::<Vec<_>>(),
                );
            }
        }
        // The anchor's marker, at its projected position while visible.
        let marker = match self.anchor {
            Some(anchor) => {
                let s = render::project(anchor);
                if s.visible {
                    vec![Shape::Text(TextShape {
                        text: "anchor".into(),
                        at: Point { x: s.x, y: s.y },
                        color: 0xFFFF55,
                        centered: true,
                        shadow: true,
                    })]
                } else {
                    Vec::new()
                }
            }
            None => Vec::new(),
        };
        render::set_hud(&marker);
    }

    fn command(&mut self, name: &str, args: &str) -> Result<Option<String>, String> {
        match name {
            "cam" => Ok(Some(pose_line())),
            "cam-free" => {
                let numbers = parse_numbers(args)?;
                let (yaw, pitch, roll) = match numbers.as_slice() {
                    [] => {
                        let Some(p) = game::get_player() else {
                            return Ok(Some("cam: no player".into()));
                        };
                        (p.yaw, p.pitch, self.roll)
                    }
                    [yaw, pitch, roll] => (*yaw, *pitch, *roll),
                    _ => return Err("cam-free: give yaw pitch roll".into()),
                };
                self.roll = roll;
                self.set(Mode::Freelook, None, yaw, pitch);
                Ok(Some(pose_line()))
            }
            "cam-back" => {
                let Some(p) = game::get_player() else {
                    return Ok(Some("cam: no player".into()));
                };
                let behind = Self::behind(&p);
                let (yaw, pitch) = (p.yaw, p.pitch);
                self.set(Mode::Freecam, Some(behind), yaw, pitch);
                Ok(Some(pose_line()))
            }
            "cam-orbit" => {
                if self.mode == Mode::Orbit {
                    self.mode = Mode::Off;
                    render::set_camera(None);
                    Ok(Some("cam orbit off".into()))
                } else {
                    self.mode = Mode::Orbit;
                    self.orbit_tick = 0;
                    Ok(Some("cam orbit on".into()))
                }
            }
            "cam-roll" => {
                let numbers = parse_numbers(args)?;
                let [roll] = numbers.as_slice() else {
                    return Err("cam-roll: give degrees".into());
                };
                self.roll = *roll;
                // Re-apply the current mode with the new roll, from the
                // camera's current look: the roll must not snap the
                // look back to where the mode started.
                let pose = render::get_camera();
                match self.mode {
                    Mode::Off => self.set(Mode::Freelook, None, pose.yaw, pose.pitch),
                    Mode::Freelook => self.set(Mode::Freelook, None, pose.yaw, pose.pitch),
                    Mode::Freecam => self.set(
                        Mode::Freecam,
                        Some(WorldPoint {
                            x: pose.x,
                            y: pose.y,
                            z: pose.z,
                        }),
                        pose.yaw,
                        pose.pitch,
                    ),
                    Mode::Orbit => {}
                }
                Ok(Some(pose_line()))
            }
            "cam-far" => {
                let pose = render::get_camera();
                self.set(
                    Mode::Freecam,
                    Some(WorldPoint {
                        x: 200.5,
                        y: 64.0,
                        z: 0.5,
                    }),
                    pose.yaw,
                    pose.pitch,
                );
                Ok(Some(pose_line()))
            }
            "cam-nan" => {
                let pose = render::get_camera();
                render::set_camera(Some(CameraOverride {
                    position: Some(WorldPoint {
                        x: f64::NAN,
                        y: 64.0,
                        z: 0.5,
                    }),
                    yaw: pose.yaw,
                    pitch: pose.pitch,
                    roll: self.roll,
                }));
                Ok(Some(pose_line()))
            }
            "cam-off" => {
                self.mode = Mode::Off;
                render::set_camera(None);
                Ok(Some("cam off".into()))
            }
            "cam-project" => match self.anchor {
                Some(anchor) => Ok(Some(anchor_line(anchor))),
                None => Ok(Some("anchor: none".into())),
            },
            _ => Err("This command does nothing".into()),
        }
    }
}

horizon_mod_sdk::export_mod!(CameraProbe);
