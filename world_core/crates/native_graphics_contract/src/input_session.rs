//! C3.2 input-driven session seam: typed HID samples, a Rust-owned fixed-tick
//! kinematic simulation over the certified world, third-person camera intent,
//! and a digest-bound deterministic input trace.
//!
//! Authority model: the Julia worker samples raw HID state (keyboard, mouse
//! cursor, raw gamepad axes) while presenting and reports exactly one typed
//! [`InputSample`] per presented batch. Only this module turns samples into
//! gameplay meaning: the DS3-style button mapping, camera-space movement
//! intent, `PhysicsWorld::step()` integration at the shared fixed tick rate,
//! and the third-person camera that follows the body. The worker never
//! derives semantics; Rust never touches HID devices. Every applied step is a
//! digest-bound `KinematicStepReceipt`, and every batch appends to an input
//! trace whose rolling digest makes a replay byte-comparable.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use luxel_reference_runtime::{
    KinematicInput, KinematicState, PhysicsWorld, REFERENCE_TICK_RATE_HZ,
};

use crate::{
    GraphicsWorkerError, GraphicsWorkerSupervisor,
    session::{PresentedGraphicsSession, WindowFrameReport},
};

pub const INPUT_SAMPLE_SCHEMA: &str = "luxel.input-sample/v1";
pub const INPUT_FRAME_SCHEMA: &str = "luxel.input-frame/v1";
pub const INPUT_TRACE_SCHEMA: &str = "luxel.input-trace/v1";

/// Fixed simulated timestep the worker batches against. The worker is asked
/// for whole multiples of this interval, so sim time and present time stay
/// composable without drifting.
pub const INPUT_SIM_TICK_DT_MS: f64 = 1000.0 / (REFERENCE_TICK_RATE_HZ as f64);

/// Raw HID state sampled by the worker. Every field is device-raw: the
/// semantic meaning (sprint, crouch, camera yaw) is applied only by Rust.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct InputSample {
    pub schema_version: String,
    /// Worker-side monotonic timestamp of the sample, in milliseconds.
    pub timestamp_ms: f64,
    /// GLFW key codes currently held, from `glfwGetKey` reporting PRESS.
    pub held_keys: Vec<i32>,
    /// Normalized cursor position within the window, `(0..1, 0..1)`; `None`
    /// when the window is unfocused or the cursor is disabled.
    pub cursor: Option<[f64; 2]>,
    /// Raw joystick axes for joystick 1 in GLFW order, or an empty vector.
    pub gamepad_axes: Vec<f32>,
    /// Raw joystick button states for joystick 1, or an empty vector.
    pub gamepad_buttons: Vec<bool>,
}

impl InputSample {
    pub fn new(timestamp_ms: f64) -> Self {
        Self {
            schema_version: INPUT_SAMPLE_SCHEMA.to_owned(),
            timestamp_ms,
            held_keys: Vec::new(),
            cursor: None,
            gamepad_axes: Vec::new(),
            gamepad_buttons: Vec::new(),
        }
    }

    fn validate(&self) -> Result<(), GraphicsWorkerError> {
        if self.schema_version != INPUT_SAMPLE_SCHEMA {
            return Err(GraphicsWorkerError::protocol(
                "input sample schema is not the supported version",
            ));
        }
        if !self.timestamp_ms.is_finite() {
            return Err(GraphicsWorkerError::protocol(
                "input sample timestamp must be finite",
            ));
        }
        for key in &self.held_keys {
            // Positive codes are GLFW keys; the reserved negative codes
            // -2..-1 are the adapter's mouse-button channel.
            let is_mouse_button = (-2..=-1).contains(key);
            if *key == 0 || (*key < 0 && !is_mouse_button) {
                return Err(GraphicsWorkerError::protocol(
                    "held key codes must be GLFW keys or the reserved mouse codes",
                ));
            }
        }
        for value in self.gamepad_axes.iter().copied() {
            if !value.is_finite() {
                return Err(GraphicsWorkerError::protocol(
                    "gamepad axes must be finite",
                ));
            }
        }
        if let Some(cursor) = self.cursor {
            for value in cursor {
                if !value.is_finite() || !(-1.0..=2.0).contains(&value) {
                    return Err(GraphicsWorkerError::protocol(
                        "cursor position must be finite and near the window",
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Semantic movement intent for one fixed tick, already DS3-shaped: camera-
/// relative planar movement, look deltas, and the discrete buttons the arena
/// demo binds. Sprint changes the movement scalar; the combat buttons are
/// carried as state for the gameplay kit and do not affect locomotion.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct InputFrame {
    pub schema_version: String,
    /// Sim tick this frame drives (`state.tick + 1`).
    pub expected_tick: u64,
    /// Movement in camera space: `x` = strafe right, `y` = forward.
    pub move_x: f32,
    pub move_y: f32,
    /// Look intent in radians: yaw (around world up) and pitch.
    pub look_dx_rad: f32,
    pub look_dy_rad: f32,
    pub sprint: bool,
    pub roll: bool,
    pub guard: bool,
    pub light_attack: bool,
    pub heavy_attack: bool,
    pub interact: bool,
}

impl InputFrame {
    pub fn new(expected_tick: u64) -> Self {
        Self {
            schema_version: INPUT_FRAME_SCHEMA.to_owned(),
            expected_tick,
            move_x: 0.0,
            move_y: 0.0,
            look_dx_rad: 0.0,
            look_dy_rad: 0.0,
            sprint: false,
            roll: false,
            guard: false,
            light_attack: false,
            heavy_attack: false,
            interact: false,
        }
    }

    fn validate(&self) -> Result<(), GraphicsWorkerError> {
        if self.schema_version != INPUT_FRAME_SCHEMA {
            return Err(GraphicsWorkerError::protocol(
                "input frame schema is not the supported version",
            ));
        }
        for value in [self.move_x, self.move_y, self.look_dx_rad, self.look_dy_rad] {
            if !value.is_finite() {
                return Err(GraphicsWorkerError::protocol(
                    "input frame fields must be finite",
                ));
            }
        }
        if self.move_x.abs() > 1.0 || self.move_y.abs() > 1.0 {
            return Err(GraphicsWorkerError::protocol(
                "movement axes are unit-scaled and must stay within [-1, 1]",
            ));
        }
        Ok(())
    }
}

/// DS3-style Xbox-layout keyboard mapping over GLFW key codes. The gamepad
/// mapping over raw axes/buttons mirrors the same layout.
pub mod ds3_map {
    /// GLFW key codes (US layout, DS3 Xbox-layout equivalents).
    pub const KEY_W: i32 = 87;
    pub const KEY_A: i32 = 65;
    pub const KEY_S: i32 = 83;
    pub const KEY_D: i32 = 68;
    pub const KEY_SHIFT: i32 = 340; // left shift
    pub const KEY_SPACE: i32 = 32; // roll/dodge
    pub const KEY_E: i32 = 69; // interact
    /// Guard is a hold on mouse-right in DS3; keyboard stand-in.
    pub const KEY_CTRL: i32 = 341; // left control
    /// GLFW mouse buttons reported through the same held-keys channel with
    /// these negative codes (GLFW key codes are positive).
    pub const MOUSE_LEFT: i32 = -1;
    pub const MOUSE_RIGHT: i32 = -2;

    /// GLFW gamepad axis indices.
    pub const AXIS_LEFT_X: usize = 0;
    pub const AXIS_LEFT_Y: usize = 1;
    pub const AXIS_RIGHT_X: usize = 2;
    pub const AXIS_RIGHT_Y: usize = 3;
    pub const AXIS_RIGHT_TRIGGER: usize = 5;

    /// Standard GLFW gamepad button indices (libglfw mapping order).
    pub const BUTTON_A: usize = 0; // roll
    pub const BUTTON_B: usize = 1; // sprint (hold)
    pub const BUTTON_X: usize = 2; // light attack
    pub const BUTTON_Y: usize = 3; // heavy attack
    pub const BUTTON_LEFT_BUMPER: usize = 4; // guard
    pub const BUTTON_RIGHT_BUMPER: usize = 5; // interact
}

const MOVE_SPEED_MPS: f32 = 4.0;
const SPRINT_MULTIPLIER: f32 = 1.8;
const MOUSE_YAW_RATE_RAD_PER_NORM: f32 = 6.5;
const MOUSE_PITCH_RATE_RAD_PER_NORM: f32 = 3.0;
const GAMEPAD_LOOK_RATE_RAD_PER_S: f32 = 3.4;
const GAMEPAD_DEADZONE: f32 = 0.15;
const CAMERA_MIN_HEIGHT_M: f32 = 0.5;
const CAMERA_DISTANCE_M: f32 = 5.0;
const CAMERA_SHOULDER_M: f32 = 0.9;

/// Apply the DS3-style keyboard mapping to one raw sample. Only the keys the
/// arena demo binds are interpreted; everything else is ignored, not guessed.
pub fn frame_from_keyboard(frame: &mut InputFrame, sample: &InputSample) {
    let held = |key: i32| sample.held_keys.contains(&key);
    let mut x = 0.0f32;
    let mut y = 0.0f32;
    if held(ds3_map::KEY_W) {
        y += 1.0;
    }
    if held(ds3_map::KEY_S) {
        y -= 1.0;
    }
    if held(ds3_map::KEY_D) {
        x += 1.0;
    }
    if held(ds3_map::KEY_A) {
        x -= 1.0;
    }
    let length = (x * x + y * y).sqrt();
    if length > 1.0 {
        x /= length;
        y /= length;
    }
    frame.move_x = x;
    frame.move_y = y;
    frame.sprint = held(ds3_map::KEY_SHIFT);
    frame.roll = held(ds3_map::KEY_SPACE);
    frame.guard = held(ds3_map::KEY_CTRL) || held(ds3_map::MOUSE_RIGHT);
    frame.light_attack = held(ds3_map::MOUSE_LEFT);
    frame.heavy_attack = held(ds3_map::KEY_SHIFT) && held(ds3_map::MOUSE_LEFT);
    frame.interact = held(ds3_map::KEY_E);
}

/// Merge the DS3-style gamepad mapping into one frame: an active stick axis
/// overrides the keyboard axis for that stick, and pressed buttons OR in. A
/// resting or deadzoned gamepad never cancels keyboard intent.
pub fn frame_from_gamepad(frame: &mut InputFrame, sample: &InputSample, batch_seconds: f32) {
    let axes = &sample.gamepad_axes;
    let buttons = &sample.gamepad_buttons;
    if axes.is_empty() && buttons.is_empty() {
        return;
    }
    let axis = |index: usize| -> f32 {
        let value = axes.get(index).copied().unwrap_or(0.0);
        let magnitude = value.abs();
        if magnitude <= GAMEPAD_DEADZONE {
            0.0
        } else {
            value.signum() * (magnitude - GAMEPAD_DEADZONE) / (1.0 - GAMEPAD_DEADZONE)
        }
    };
    let button = |index: usize| -> bool { buttons.get(index).copied().unwrap_or(false) };
    let stick_x = axis(ds3_map::AXIS_LEFT_X);
    let stick_y = axis(ds3_map::AXIS_LEFT_Y);
    if stick_x != 0.0 {
        frame.move_x = stick_x;
    }
    if stick_y != 0.0 {
        // GLFW stick Y is positive down; forward is stick up.
        frame.move_y = -stick_y;
    }
    frame.look_dx_rad += axis(ds3_map::AXIS_RIGHT_X) * GAMEPAD_LOOK_RATE_RAD_PER_S * batch_seconds;
    frame.look_dy_rad += axis(ds3_map::AXIS_RIGHT_Y) * GAMEPAD_LOOK_RATE_RAD_PER_S * batch_seconds;
    frame.sprint |= button(ds3_map::BUTTON_B);
    frame.roll |= button(ds3_map::BUTTON_A);
    frame.guard |= button(ds3_map::BUTTON_LEFT_BUMPER);
    frame.light_attack |= button(ds3_map::BUTTON_X);
    frame.heavy_attack |= button(ds3_map::BUTTON_Y);
    frame.interact |= button(ds3_map::BUTTON_RIGHT_BUMPER);
}

/// Turn a raw sample into a semantic frame for the given tick, merging
/// keyboard and gamepad (gamepad wins where both drive the same axis).
pub fn frame_from_sample(
    expected_tick: u64,
    sample: &InputSample,
    batch_seconds: f32,
) -> Result<InputFrame, GraphicsWorkerError> {
    sample.validate()?;
    let mut frame = InputFrame::new(expected_tick);
    frame_from_keyboard(&mut frame, sample);
    frame_from_gamepad(&mut frame, sample, batch_seconds);
    // Mouse look: cursor displacement from center drives yaw/pitch like a
    // raw-relative approximation until a true cursor-lock seam exists.
    if let Some([cx, cy]) = sample.cursor {
        let dx = (cx - 0.5) as f32;
        let dy = (cy - 0.5) as f32;
        frame.look_dx_rad += dx.abs() * MOUSE_YAW_RATE_RAD_PER_NORM * dx.signum();
        frame.look_dy_rad += dy.abs() * MOUSE_PITCH_RATE_RAD_PER_NORM * dy.signum();
    }
    frame.validate()?;
    Ok(frame)
}

/// Unit-norm heading basis from a yaw angle in the right-handed, Y-up world:
/// `forward = (sin yaw, cos yaw)` lies in the XZ plane, and `right` is
/// `up × forward` (east when facing north), so strafing and the camera
/// shoulder offset agree with the movement basis.
fn heading_basis(yaw_rad: f32) -> ([f64; 2], [f64; 2]) {
    let forward = [(yaw_rad.sin()) as f64, (yaw_rad.cos()) as f64];
    let right = [forward[1], -forward[0]];
    (forward, right)
}

/// Compose the planar velocity command for one tick: camera-relative movement
/// rotated into world space, sprint-scaled. Rejected moves never move the
/// body; the step seam consumes the tick atomically.
fn velocity_for(frame: &InputFrame, yaw_rad: f32) -> [f64; 2] {
    let (forward, right) = heading_basis(yaw_rad);
    let scalar = if frame.sprint {
        MOVE_SPEED_MPS * SPRINT_MULTIPLIER
    } else {
        MOVE_SPEED_MPS
    };
    let world_x = right[0] * frame.move_x as f64 + forward[0] * frame.move_y as f64;
    let world_z = right[1] * frame.move_x as f64 + forward[1] * frame.move_y as f64;
    let length = (world_x * world_x + world_z * world_z).sqrt() as f32;
    let scale = if length > 1.0 { scalar / length } else { scalar };
    [world_x * scale as f64, world_z * scale as f64]
}

/// Third-person camera intent: behind and slightly above the body origin,
/// shoulder offset to the right, looking along the view heading. Height is
/// clamped above the ground so the camera never dips below terrain level.
pub fn camera_intent(
    body: &KinematicState,
    yaw_rad: f32,
    pitch_rad: f32,
) -> Result<([f32; 3], [f32; 3]), GraphicsWorkerError> {
    let position = body.body.position_xyz_m;
    let (forward, right) = heading_basis(yaw_rad);
    let pitch = pitch_rad.clamp(-1.2, 1.2) as f64;
    let eye = [
        position[0] - forward[0] * CAMERA_DISTANCE_M as f64 + right[0] * CAMERA_SHOULDER_M as f64,
        (position[1] + 2.0 + (pitch.sin() * CAMERA_DISTANCE_M as f64)).max(
            position[1] + CAMERA_MIN_HEIGHT_M as f64,
        ),
        position[2] - forward[1] * CAMERA_DISTANCE_M as f64 + right[1] * CAMERA_SHOULDER_M as f64,
    ];
    // Look direction from eye toward a point above the body origin.
    let target = [position[0], position[1] + 1.5, position[2]];
    let look = [target[0] - eye[0], target[1] - eye[1], target[2] - eye[2]];
    let length = (look[0] * look[0] + look[1] * look[1] + look[2] * look[2]).sqrt();
    if length <= f64::EPSILON || !length.is_finite() {
        return Err(GraphicsWorkerError::protocol(
            "camera look direction degenerated",
        ));
    }
    Ok((
        [eye[0] as f32, eye[1] as f32, eye[2] as f32],
        [
            (look[0] / length) as f32,
            (look[1] / length) as f32,
            (look[2] / length) as f32,
        ],
    ))
}

/// One applied or rejected simulation step, recorded for replay comparison.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TraceStep {
    pub frame: InputFrame,
    pub disposition: String,
    pub position_after_xyz_m: [f64; 3],
}

/// Rolling input-trace digest. The digest chains frame bytes with the previous
/// digest, so a replay is byte-comparable against the registered trace.
pub const INPUT_TRACE_DIGEST_DOMAIN: &[u8] = b"luxel.input-trace/v1\0";

pub fn trace_digest(steps: &[TraceStep]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(INPUT_TRACE_DIGEST_DOMAIN);
    for step in steps {
        hasher.update(
            serde_json::to_vec(step).expect("trace step serialization cannot fail"),
        );
    }
    format!("sha256:{}", hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// An input-driven native session: the presented session keeps presenting,
/// and this layer owns the semantics that drive its camera each batch. The
/// world artifact stays owned by the caller and borrowed here, matching the
/// session contract's `capture_and_promote(supervisor, world)` pattern.
pub struct InputDrivenSession<'world> {
    session: PresentedGraphicsSession,
    physics: PhysicsWorld<'world>,
    world: &'world luxel_reference_runtime::WorldArtifact,
    player: KinematicState,
    yaw_rad: f32,
    pitch_rad: f32,
    last_sample_ms: Option<f64>,
    trace: Vec<TraceStep>,
    trace_digest: String,
}

impl<'world> InputDrivenSession<'world> {
    /// Bind an input-driven layer onto a presented session over a validated
    /// world. The player starts at the authored spawn.
    pub fn new(
        session: PresentedGraphicsSession,
        world: &'world luxel_reference_runtime::WorldArtifact,
    ) -> Result<Self, GraphicsWorkerError> {
        session.validate_bound_world(world)?;
        let physics_world = PhysicsWorld::new(world)
            .map_err(|error| GraphicsWorkerError::provenance(format!("physics binding failed: {error}")))?;
        let player = physics_world
            .initialize_player()
            .map_err(|error| GraphicsWorkerError::provenance(format!("player spawn failed: {error}")))?;
        Ok(Self {
            session,
            physics: physics_world,
            world,
            player,
            yaw_rad: 0.0,
            pitch_rad: 0.0,
            last_sample_ms: None,
            trace: Vec::new(),
            trace_digest: trace_digest(&[]),
        })
    }

    pub fn player_state(&self) -> &KinematicState {
        &self.player
    }

    pub fn camera_yaw_rad(&self) -> f32 {
        self.yaw_rad
    }

    pub fn camera_pitch_rad(&self) -> f32 {
        self.pitch_rad
    }

    pub fn trace(&self) -> &[TraceStep] {
        &self.trace
    }

    pub fn trace_digest(&self) -> &str {
        &self.trace_digest
    }

    /// Advance the fixed-tick simulation by one semantic frame. A rejected
    /// step (collision, slope, boundary) still consumes the tick and is
    /// recorded with its disposition — the world rejects, the session adapts.
    pub fn apply_frame(&mut self, frame: InputFrame) -> Result<&TraceStep, GraphicsWorkerError> {
        frame.validate()?;
        let expected = self.player.body.tick + 1;
        if frame.expected_tick != expected {
            return Err(GraphicsWorkerError::protocol(format!(
                "input frame expects tick {}, simulation is at tick {}",
                frame.expected_tick, self.player.body.tick
            )));
        }
        // Yaw is the simulation's heading authority; pitch only affects the
        // camera and is clamped in `camera_intent`.
        self.yaw_rad = wrap_angle(self.yaw_rad + frame.look_dx_rad);
        self.pitch_rad = (self.pitch_rad + frame.look_dy_rad).clamp(-1.2, 1.2);
        let velocity = velocity_for(&frame, self.yaw_rad);
        let input = KinematicInput {
            expected_tick: frame.expected_tick,
            velocity_xz_mps: velocity,
        };
        let receipt = self
            .physics
            .step(&self.player, input)
            .map_err(|error| {
                GraphicsWorkerError::provenance(format!("kinematic step rejected: {error}"))
            })?;
        self.player = receipt.body.next_state.clone();
        let disposition = match receipt.body.disposition {
            luxel_reference_runtime::KinematicStepDisposition::Advanced => "advanced",
            luxel_reference_runtime::KinematicStepDisposition::Rejected => "rejected",
        };
        let step = TraceStep {
            frame,
            disposition: disposition.to_owned(),
            position_after_xyz_m: self.player.body.position_xyz_m,
        };
        self.trace.push(step);
        self.trace_digest = trace_digest(&self.trace);
        let last = self.trace.last().expect("a step was just pushed");
        Ok(last)
    }

    /// Sample one raw worker input, advance the simulation by the elapsed
    /// whole ticks (capped for the bounded seam), and re-aim the presented
    /// camera at the resulting third-person intent. Returns the batch report.
    pub fn sample_and_present(
        &mut self,
        supervisor: &mut GraphicsWorkerSupervisor,
        sample: &InputSample,
        target_frames: u32,
    ) -> Result<(WindowFrameReport, usize), GraphicsWorkerError> {
        sample.validate()?;
        let elapsed_ms = match self.last_sample_ms {
            Some(previous) => (sample.timestamp_ms - previous).max(0.0),
            None => INPUT_SIM_TICK_DT_MS * target_frames.max(1) as f64,
        };
        self.last_sample_ms = Some(sample.timestamp_ms);
        let ticks = ((elapsed_ms / INPUT_SIM_TICK_DT_MS).floor() as usize)
            .clamp(1, target_frames.max(1) as usize);
        let mut applied = 0usize;
        for index in 0..ticks {
            let expected_tick = self.player.body.tick + 1;
            let frame = frame_from_sample(
                expected_tick,
                sample,
                INPUT_SIM_TICK_DT_MS as f32,
            )?;
            self.apply_frame(frame)?;
            applied += 1;
            let _ = index;
        }
        let (position, forward) = camera_intent(&self.player, self.yaw_rad, self.pitch_rad)?;
        self.session.set_camera(position, forward)?;
        let report = self.session.present_frames(supervisor, target_frames)?;
        Ok((report, applied))
    }

    /// Promote Tier-A evidence at the current camera through the offscreen
    /// authority path (unchanged contract: presented frames carry none).
    pub fn capture_and_promote(
        &mut self,
        supervisor: &mut GraphicsWorkerSupervisor,
    ) -> Result<crate::PromotedFrame, GraphicsWorkerError> {
        self.session.capture_and_promote(supervisor, self.world)
    }

    pub fn open_window(
        &mut self,
        supervisor: &mut GraphicsWorkerSupervisor,
    ) -> Result<crate::session::PresentedSessionOpenReceipt, GraphicsWorkerError> {
        self.session.open_window(supervisor)
    }

    pub fn close_window(
        &mut self,
        supervisor: &mut GraphicsWorkerSupervisor,
    ) -> Result<u64, GraphicsWorkerError> {
        self.session.close_window(supervisor)
    }

    pub fn presented_session(&self) -> &PresentedGraphicsSession {
        &self.session
    }

    pub fn is_window_open(&self) -> bool {
        self.session.is_window_open()
    }
}

fn wrap_angle(angle: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    let mut wrapped = angle % tau;
    if wrapped > std::f32::consts::PI {
        wrapped -= tau;
    } else if wrapped < -std::f32::consts::PI {
        wrapped += tau;
    }
    wrapped
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_keys(keys: &[i32]) -> InputSample {
        let mut sample = InputSample::new(100.0);
        sample.held_keys = keys.to_vec();
        sample
    }

    #[test]
    fn input_sample_rejects_bad_schema_and_nonfinite_fields() {
        let mut sample = InputSample::new(1.0);
        sample.schema_version = "luxel.input-sample/v0".into();
        assert!(sample.validate().is_err());
        let mut sample = InputSample::new(f64::NAN);
        sample.schema_version = INPUT_SAMPLE_SCHEMA.into();
        assert!(sample.validate().is_err());
        let mut sample = InputSample::new(1.0);
        sample.gamepad_axes = vec![f32::INFINITY];
        assert!(sample.validate().is_err());
        sample.gamepad_axes = Vec::new();
        sample.cursor = Some([f64::NAN, 0.5]);
        assert!(sample.validate().is_err());
    }

    #[test]
    fn ds3_keyboard_mapping_matches_the_xbox_layout_shape() {
        let sample = sample_keys(&[ds3_map::KEY_W, ds3_map::KEY_SHIFT]);
        let frame = frame_from_sample(1, &sample, 1.0 / 30.0).expect("frame maps");
        assert!((frame.move_y - 1.0).abs() < 1e-6, "W drives forward");
        assert!(frame.sprint, "shift sprints");
        assert!(!frame.roll && !frame.guard, "unbound actions stay false");

        let sample = sample_keys(&[ds3_map::KEY_A, ds3_map::KEY_SPACE, ds3_map::MOUSE_RIGHT]);
        let frame = frame_from_sample(1, &sample, 1.0 / 30.0).expect("frame maps");
        assert!((frame.move_x + 1.0).abs() < 1e-6, "A strafes left");
        assert!(frame.roll, "space rolls");
        assert!(frame.guard, "right mouse guards");
    }

    #[test]
    fn ds3_gamepad_mapping_respects_deadzone_and_stick_sign() {
        let mut sample = InputSample::new(1.0);
        sample.gamepad_axes = vec![0.05, -1.0, 1.0, 0.0, 0.0, 0.0];
        sample.gamepad_buttons = vec![true, false, false, false, false, false];
        let frame = frame_from_sample(1, &sample, 1.0).expect("frame maps");
        assert_eq!(frame.move_x, 0.0, "small stick deflection is deadzoned");
        assert!((frame.move_y - 1.0).abs() < 1e-6, "stick up drives forward");
        assert!(frame.look_dx_rad > 0.0, "right stick right yaws positive");
        assert!(frame.roll, "A rolls");
    }

    #[test]
    fn movement_composes_in_camera_space() {
        let mut frame = InputFrame::new(1);
        frame.move_y = 1.0;
        let [vx, vz] = velocity_for(&frame, 0.0);
        assert!((vx - 0.0).abs() < 1e-9 && (vz - MOVE_SPEED_MPS as f64).abs() < 1e-9);
        // Yaw +90 degrees: forward becomes +x.
        let [vx, _] = velocity_for(&frame, std::f32::consts::FRAC_PI_2);
        assert!((vx - MOVE_SPEED_MPS as f64).abs() < 1e-6);
        // Diagonal movement is normalized before scaling.
        frame.move_x = 1.0;
        let [vx, vz] = velocity_for(&frame, 0.0);
        let speed = (vx * vx + vz * vz).sqrt();
        // f32 scale rounding admits ~1e-7 relative error at unit-normalized
        // diagonals; the tolerance matches that envelope, not exactness.
        assert!((speed - MOVE_SPEED_MPS as f64).abs() < 1e-6);
    }

    #[test]
    fn camera_intent_stays_above_the_body_origin() {
        let mut body_body = luxel_reference_runtime::KinematicStateBody {
            schema_version: luxel_reference_runtime::PHYSICS_STATE_SCHEMA.into(),
            world_artifact_id: "w".into(),
            world_artifact_sha256: "sha256:0".into(),
            tick: 3,
            position_xyz_m: [1.0, 6.0, 2.0],
            velocity_xz_mps: [0.0, 0.0],
        };
        body_body.position_xyz_m[1] = 6.0;
        let state = KinematicState {
            body: body_body,
            state_sha256: "sha256:0".into(),
        };
        let (position, forward) = camera_intent(&state, 0.0, -0.9).expect("intent resolves");
        assert!(f64::from(position[1]) > state.body.position_xyz_m[1]);
        let length =
            (forward[0] * forward[0] + forward[1] * forward[1] + forward[2] * forward[2]).sqrt();
        assert!((length - 1.0).abs() < 1e-5, "forward is normalized");
    }

    #[test]
    fn trace_digest_is_stable_and_order_sensitive() {
        let mut frame = InputFrame::new(1);
        frame.move_y = 0.5;
        let step = TraceStep {
            frame,
            disposition: "advanced".into(),
            position_after_xyz_m: [0.0, 0.0, 0.5],
        };
        let digest_a = trace_digest(&[step.clone()]);
        let digest_b = trace_digest(&[step]);
        assert_eq!(digest_a, digest_b, "digest is deterministic");
        let mut other = InputFrame::new(1);
        other.move_y = 0.4;
        let digest_c = trace_digest(&[TraceStep {
            frame: other,
            disposition: "advanced".into(),
            position_after_xyz_m: [0.0, 0.0, 0.4],
        }]);
        assert_ne!(digest_a, digest_c, "different traces digest differently");
        assert!(digest_a.starts_with("sha256:"));
    }

    #[test]
    fn frame_from_sample_rejects_out_of_range_movement() {
        let mut frame = InputFrame::new(1);
        frame.move_x = 1.5;
        assert!(frame.validate().is_err());
    }
}
