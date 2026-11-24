//! Touchscreen controls.
//!
//! Enabled on Android, or on desktop with `VOXYGEN_TOUCH_EMULATION=1` (the
//! left mouse button then acts as a single finger, for testing).
//!
//! Two modes:
//! * **UI mode** (menus, and in-game whenever the cursor is free, e.g. the
//!   inventory is open): one finger acts as a mouse. A tap is a left click, a
//!   drag is a left-button drag, a long press is a right click and a second
//!   finger scrolls.
//! * **Game mode** (in-game with the cursor grabbed): a floating virtual
//!   joystick on the left, camera drag and pinch-zoom on the right, plus
//!   on-screen action buttons. Buttons send the same `GameInput`s as the
//!   keyboard, so all existing gameplay logic applies unchanged.
//!
//! All positions here are logical pixels with the origin at the top-left of
//! the window.

use crate::{
    game_input::GameInput,
    ui,
    window::{AnalogGameInput, Event},
};
use conrod_core::input;
use hashbrown::HashMap;
use std::time::{Duration, Instant};
use vek::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TouchPhase {
    Start,
    Move,
    End,
    Cancel,
}

impl From<winit::event::TouchPhase> for TouchPhase {
    fn from(phase: winit::event::TouchPhase) -> Self {
        match phase {
            winit::event::TouchPhase::Started => Self::Start,
            winit::event::TouchPhase::Moved => Self::Move,
            winit::event::TouchPhase::Ended => Self::End,
            winit::event::TouchPhase::Cancelled => Self::Cancel,
        }
    }
}

/// A single finger update in logical pixels.
#[derive(Clone, Copy, Debug)]
pub struct TouchInput {
    pub id: u64,
    pub phase: TouchPhase,
    pub pos: Vec2<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ButtonId {
    Primary,
    Secondary,
    Block,
    Roll,
    Jump,
    Interact,
    Glide,
    Sneak,
    Slot(u8),
    Menu,
    Bag,
    Map,
    Wield,
    Swap,
    Sit,
    Mount,
    Lantern,
    Respawn,
    /// Shown in-game while the cursor is free (a window is open).
    Back,
}

impl ButtonId {
    pub fn label(self) -> &'static str {
        match self {
            Self::Primary => "ATK",
            Self::Secondary => "ALT",
            Self::Block => "BLOCK",
            Self::Roll => "ROLL",
            Self::Jump => "JUMP",
            Self::Interact => "USE",
            Self::Glide => "GLIDE",
            Self::Sneak => "SNEAK",
            Self::Slot(1) => "1",
            Self::Slot(2) => "2",
            Self::Slot(3) => "3",
            Self::Slot(4) => "4",
            Self::Slot(5) => "5",
            Self::Slot(_) => "?",
            Self::Menu => "MENU",
            Self::Bag => "BAG",
            Self::Map => "MAP",
            Self::Wield => "WIELD",
            Self::Swap => "SWAP",
            Self::Sit => "SIT",
            Self::Mount => "MOUNT",
            Self::Lantern => "LIGHT",
            Self::Respawn => "RESPAWN",
            Self::Back => "BACK",
        }
    }

    fn inputs(self) -> &'static [GameInput] {
        match self {
            Self::Primary => &[GameInput::Primary],
            Self::Secondary => &[GameInput::Secondary],
            Self::Block => &[GameInput::Block],
            Self::Roll => &[GameInput::Roll],
            // Same set the space bar triggers by default.
            Self::Jump => &[GameInput::Jump, GameInput::WallJump, GameInput::SwimUp],
            Self::Interact => &[GameInput::Interact],
            Self::Glide => &[GameInput::Glide],
            Self::Sneak => &[GameInput::Sneak],
            Self::Slot(1) => &[GameInput::Slot1],
            Self::Slot(2) => &[GameInput::Slot2],
            Self::Slot(3) => &[GameInput::Slot3],
            Self::Slot(4) => &[GameInput::Slot4],
            Self::Slot(5) => &[GameInput::Slot5],
            Self::Slot(_) => &[],
            Self::Menu | Self::Back => &[GameInput::Escape],
            Self::Bag => &[GameInput::Inventory],
            Self::Map => &[GameInput::Map],
            Self::Wield => &[GameInput::ToggleWield],
            Self::Swap => &[GameInput::SwapLoadout],
            Self::Sit => &[GameInput::Sit],
            Self::Mount => &[GameInput::Mount],
            Self::Lantern => &[GameInput::ToggleLantern],
            Self::Respawn => &[GameInput::Respawn],
        }
    }

    /// Dragging while holding these also turns the camera, so you can aim
    /// while attacking.
    fn allows_look(self) -> bool { matches!(self, Self::Primary | Self::Secondary | Self::Block) }
}

#[derive(Clone, Copy, Debug)]
pub struct Button {
    pub id: ButtonId,
    pub center: Vec2<f32>,
    pub radius: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisualStyle {
    Button,
    StickBase,
    StickKnob,
}

/// Something for the HUD to draw (see `hud::touch_overlay`).
#[derive(Clone, Copy, Debug)]
pub struct Visual {
    pub center: Vec2<f32>,
    pub radius: f32,
    pub label: &'static str,
    pub pressed: bool,
    pub style: VisualStyle,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum UiState {
    /// Down, not moved yet: could become a tap, drag or long press.
    Pending,
    /// Left button held, following the finger.
    Dragging,
    /// Already fired a right click; ignore the rest of this touch.
    LongPressed,
    /// Turned into a two-finger scroll; no click on release.
    Scrolling,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Role {
    Ui(UiState),
    /// Second finger in UI mode.
    UiScroll,
    Joystick { origin: Vec2<f32> },
    Button(ButtonId),
    Camera,
    /// Belonged to a mode we have since left; wait for it to lift.
    Ignored,
}

#[derive(Clone, Copy, Debug)]
struct Finger {
    role: Role,
    start: Vec2<f32>,
    last: Vec2<f32>,
    started_at: Instant,
}

const TAP_SLOP: f32 = 10.0;
const LONG_PRESS: Duration = Duration::from_millis(450);
/// Camera turn per logical pixel dragged (in `CursorPan` units).
const LOOK_SPEED: f32 = 1.1;
const PINCH_ZOOM_SPEED: f32 = 0.12;
const SCROLL_PX_PER_LINE: f32 = 40.0;

pub struct TouchControls {
    enabled: bool,
    size: Vec2<f32>,
    /// Set every frame by the session; cleared when the play state changes.
    pub in_game: bool,
    /// Set by the HUD so a respawn button can be shown.
    pub player_dead: bool,
    grabbed: bool,
    fingers: HashMap<u64, Finger>,
    stick: Vec2<f32>,
    frame: u64,
    hide_cursor_at_frame: Option<u64>,
    pub look_sensitivity: f32,
}

impl TouchControls {
    pub fn new() -> Self {
        let enabled = cfg!(target_os = "android")
            || std::env::var("VOXYGEN_TOUCH_EMULATION").is_ok_and(|v| v != "0" && !v.is_empty());
        if enabled {
            tracing::info!("Touch controls enabled");
        }
        Self {
            enabled,
            size: Vec2::new(1280.0, 720.0),
            in_game: false,
            player_dead: false,
            grabbed: false,
            fingers: HashMap::new(),
            stick: Vec2::zero(),
            frame: 0,
            hide_cursor_at_frame: None,
            look_sensitivity: 1.0,
        }
    }

    pub fn enabled(&self) -> bool { self.enabled }

    pub fn set_size(&mut self, logical: Vec2<f32>) {
        if logical.x >= 1.0 && logical.y >= 1.0 {
            self.size = logical;
        }
    }

    fn game_mode(&self) -> bool { self.in_game && self.grabbed }

    /// Called whenever the cursor grab changes (i.e. we switch between game
    /// and UI mode). Releases anything still held from the old mode.
    pub fn set_grabbed(&mut self, grabbed: bool, out: &mut Vec<Event>) {
        if grabbed != self.grabbed {
            self.release_all(out);
            self.grabbed = grabbed;
        }
    }

    /// Called when the play state changes (menu <-> game).
    pub fn reset(&mut self, out: &mut Vec<Event>) {
        self.release_all(out);
        self.in_game = false;
        self.player_dead = false;
    }

    fn release_all(&mut self, out: &mut Vec<Event>) {
        for finger in self.fingers.values_mut() {
            match finger.role {
                Role::Button(id) => {
                    for input in id.inputs() {
                        out.push(Event::InputUpdate(*input, false));
                    }
                },
                Role::Ui(UiState::Dragging) => {
                    push_ui_button(out, Mouse::Left, false);
                },
                _ => {},
            }
            finger.role = Role::Ignored;
        }
        if self.stick != Vec2::zero() {
            self.stick = Vec2::zero();
            push_stick(out, self.stick);
        }
    }

    /// Size unit for the on-screen controls, in logical pixels.
    fn unit(&self) -> f32 { (self.size.x.min(self.size.y) / 7.5).clamp(40.0, 84.0) }

    fn joystick_home(&self) -> Vec2<f32> {
        let u = self.unit();
        Vec2::new(0.3 * u + 1.5 * u, self.size.y - 0.3 * u - 1.5 * u)
    }

    fn joystick_radius(&self) -> f32 { 1.15 * self.unit() }

    fn in_joystick_zone(&self, pos: Vec2<f32>) -> bool {
        pos.x < self.size.x * 0.42 && pos.y > self.size.y * 0.28
    }

    /// Buttons for the current mode.
    pub fn buttons(&self) -> Vec<Button> {
        let mut buttons = Vec::new();
        if !self.in_game {
            return buttons;
        }
        let u = self.unit();
        let (w, h) = (self.size.x, self.size.y);
        let m = 0.3 * u;

        if !self.grabbed {
            buttons.push(Button {
                id: ButtonId::Back,
                center: Vec2::new(m + 0.5 * u, m + 0.5 * u),
                radius: 0.5 * u,
            });
            return buttons;
        }

        // Bottom-right cluster, in units measured left/up from the corner.
        let br = |left: f32, up: f32, r: f32, id| Button {
            id,
            center: Vec2::new(w - m - left * u, h - m - up * u),
            radius: r * u,
        };
        let primary = Vec2::new(1.1, 1.1);
        // Ring around the main attack button: angle 0 = left, 90 = up.
        let ring = |deg: f32, dist: f32, r: f32, id| {
            let a = deg.to_radians();
            br(primary.x + dist * a.cos(), primary.y + dist * a.sin(), r, id)
        };
        buttons.push(br(primary.x, primary.y, 0.82, ButtonId::Primary));
        buttons.push(ring(-6.0, 1.62, 0.55, ButtonId::Secondary));
        buttons.push(ring(31.0, 1.62, 0.46, ButtonId::Block));
        buttons.push(ring(63.0, 1.62, 0.5, ButtonId::Roll));
        buttons.push(ring(96.0, 1.65, 0.56, ButtonId::Jump));
        // Outer ring: less frequent actions.
        buttons.push(ring(-2.0, 2.85, 0.44, ButtonId::Interact));
        buttons.push(ring(24.0, 2.85, 0.42, ButtonId::Glide));
        buttons.push(ring(48.0, 2.85, 0.4, ButtonId::Sneak));
        // Ability hotbar slots in an arc above the cluster.
        for (i, deg) in [74.0f32, 88.0, 102.0].into_iter().enumerate() {
            buttons.push(ring(deg, 3.1, 0.36, ButtonId::Slot(i as u8 + 1)));
        }
        for (i, deg) in [70.0f32, 83.0].into_iter().enumerate() {
            buttons.push(ring(deg, 3.95, 0.34, ButtonId::Slot(i as u8 + 4)));
        }

        // Top-left row of small utility buttons.
        let top = [
            ButtonId::Menu,
            ButtonId::Bag,
            ButtonId::Map,
            ButtonId::Wield,
            ButtonId::Swap,
            ButtonId::Sit,
            ButtonId::Mount,
            ButtonId::Lantern,
        ];
        let r = 0.38 * u;
        for (i, id) in top.into_iter().enumerate() {
            buttons.push(Button {
                id,
                center: Vec2::new(m + r + i as f32 * 0.86 * u, m + r),
                radius: r,
            });
        }

        if self.player_dead {
            buttons.push(Button {
                id: ButtonId::Respawn,
                center: Vec2::new(w * 0.5, h * 0.62),
                radius: 0.75 * u,
            });
        }
        buttons
    }

    fn hit_button(&self, pos: Vec2<f32>) -> Option<ButtonId> {
        self.buttons()
            .into_iter()
            .filter_map(|b| {
                let d = b.center.distance(pos);
                // A little forgiving: fingers are imprecise.
                (d <= b.radius * 1.2 + 4.0).then_some((d / b.radius, b.id))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, id)| id)
    }

    /// What the HUD should draw this frame.
    pub fn visuals(&self) -> Vec<Visual> {
        let mut visuals = Vec::new();
        if !self.enabled || !self.in_game {
            return visuals;
        }
        let held: Vec<ButtonId> = self
            .fingers
            .values()
            .filter_map(|f| match f.role {
                Role::Button(id) => Some(id),
                _ => None,
            })
            .collect();

        if self.game_mode() {
            let (base, knob) = match self
                .fingers
                .values()
                .find_map(|f| match f.role {
                    Role::Joystick { origin } => Some(origin),
                    _ => None,
                }) {
                Some(origin) => (origin, origin + self.stick_offset()),
                None => (self.joystick_home(), self.joystick_home()),
            };
            visuals.push(Visual {
                center: base,
                radius: self.joystick_radius(),
                label: "",
                pressed: false,
                style: VisualStyle::StickBase,
            });
            visuals.push(Visual {
                center: knob,
                radius: 0.45 * self.unit(),
                label: "",
                pressed: self.stick != Vec2::zero(),
                style: VisualStyle::StickKnob,
            });
        }

        for button in self.buttons() {
            visuals.push(Visual {
                center: button.center,
                radius: button.radius,
                label: button.id.label(),
                pressed: held.contains(&button.id),
                style: VisualStyle::Button,
            });
        }
        visuals
    }

    fn stick_offset(&self) -> Vec2<f32> {
        Vec2::new(self.stick.x, -self.stick.y) * self.joystick_radius()
    }

    /// Per-frame work: deferred cursor hiding and long presses.
    pub fn on_frame(&mut self) -> Vec<Event> {
        let mut out = Vec::new();
        if !self.enabled {
            return out;
        }
        self.frame += 1;

        // Move the virtual mouse away after a tap so hover highlights and
        // tooltips don't stick. Deferred: iced uses one cursor position per
        // frame, and the click must be processed at the tap position first.
        if let Some(at) = self.hide_cursor_at_frame
            && self.frame >= at
            && !self.fingers.values().any(|f| matches!(f.role, Role::Ui(_)))
        {
            self.hide_cursor_at_frame = None;
            push_ui_cursor(&mut out, self.size, Vec2::new(-10_000.0, -10_000.0));
        }

        let now = Instant::now();
        let mut long_press = None;
        for finger in self.fingers.values_mut() {
            if finger.role == Role::Ui(UiState::Pending)
                && now.duration_since(finger.started_at) >= LONG_PRESS
            {
                finger.role = Role::Ui(UiState::LongPressed);
                long_press = Some(finger.start);
            }
        }
        if let Some(pos) = long_press {
            push_ui_cursor(&mut out, self.size, pos);
            push_ui_button(&mut out, Mouse::Right, true);
            push_ui_button(&mut out, Mouse::Right, false);
            self.hide_cursor_at_frame = Some(self.frame + 2);
        }
        out
    }

    pub fn handle(&mut self, touch: TouchInput, out: &mut Vec<Event>) {
        if !self.enabled {
            return;
        }
        match touch.phase {
            TouchPhase::Start => self.start(touch, out),
            TouchPhase::Move => self.moved(touch, out),
            TouchPhase::End | TouchPhase::Cancel => {
                self.end(touch, touch.phase == TouchPhase::Cancel, out)
            },
        }
    }

    fn start(&mut self, touch: TouchInput, out: &mut Vec<Event>) {
        let pos = touch.pos;
        let role = if self.game_mode() {
            if let Some(id) = self.hit_button(pos) {
                for input in id.inputs() {
                    out.push(Event::InputUpdate(*input, true));
                }
                Role::Button(id)
            } else if self.in_joystick_zone(pos)
                && !self
                    .fingers
                    .values()
                    .any(|f| matches!(f.role, Role::Joystick { .. }))
            {
                Role::Joystick { origin: pos }
            } else {
                Role::Camera
            }
        } else if self.in_game
            && let Some(id @ ButtonId::Back) = self.hit_button(pos)
        {
            for input in id.inputs() {
                out.push(Event::InputUpdate(*input, true));
            }
            Role::Button(id)
        } else if let Some(first) = self
            .fingers
            .values_mut()
            .find(|f| matches!(f.role, Role::Ui(_)))
        {
            // Second finger in UI mode: scroll instead of clicking.
            match first.role {
                Role::Ui(UiState::Pending) => first.role = Role::Ui(UiState::Scrolling),
                Role::Ui(UiState::Dragging) => {
                    push_ui_button(out, Mouse::Left, false);
                    first.role = Role::Ui(UiState::Scrolling);
                },
                _ => {},
            }
            let anchor = first.last;
            push_ui_cursor(out, self.size, anchor);
            Role::UiScroll
        } else {
            push_ui_cursor(out, self.size, pos);
            self.hide_cursor_at_frame = None;
            Role::Ui(UiState::Pending)
        };

        self.fingers.insert(touch.id, Finger {
            role,
            start: pos,
            last: pos,
            started_at: Instant::now(),
        });
    }

    fn moved(&mut self, touch: TouchInput, out: &mut Vec<Event>) {
        let Some(mut finger) = self.fingers.get(&touch.id).copied() else {
            return;
        };
        let pos = touch.pos;
        let delta = pos - finger.last;
        if delta == Vec2::zero() {
            return;
        }
        finger.last = pos;

        match finger.role {
            Role::Ui(UiState::Pending) => {
                if pos.distance(finger.start) > TAP_SLOP {
                    // Start a drag from where the finger went down.
                    push_ui_cursor(out, self.size, finger.start);
                    push_ui_button(out, Mouse::Left, true);
                    push_ui_cursor(out, self.size, pos);
                    finger.role = Role::Ui(UiState::Dragging);
                }
            },
            Role::Ui(UiState::Dragging) => push_ui_cursor(out, self.size, pos),
            Role::Ui(UiState::Scrolling) | Role::UiScroll => push_ui_scroll(out, delta.y * 0.5),
            Role::Ui(UiState::LongPressed) | Role::Ignored => {},
            Role::Joystick { origin } => {
                let r = self.joystick_radius();
                let mut v = (pos - origin) / r;
                if v.magnitude() > 1.0 {
                    v = v.normalized();
                }
                // Small dead zone so a resting thumb doesn't drift.
                let v = if v.magnitude() < 0.12 { Vec2::zero() } else { v };
                self.stick = Vec2::new(v.x, -v.y);
                push_stick(out, self.stick);
            },
            Role::Camera => {
                let pinch = self
                    .fingers
                    .iter()
                    .find(|(id, f)| **id != touch.id && f.role == Role::Camera)
                    .map(|(_, f)| f.last);
                if let Some(other) = pinch {
                    let before = (pos - delta).distance(other);
                    let after = pos.distance(other);
                    // Fingers apart = zoom in (negative), together = zoom out.
                    out.push(Event::Zoom((before - after) * PINCH_ZOOM_SPEED));
                } else {
                    out.push(Event::CursorPan(delta * LOOK_SPEED * self.look_sensitivity));
                }
            },
            Role::Button(id) => {
                if id.allows_look() && pos.distance(finger.start) > TAP_SLOP {
                    out.push(Event::CursorPan(delta * LOOK_SPEED * self.look_sensitivity));
                }
            },
        }
        self.fingers.insert(touch.id, finger);
    }

    fn end(&mut self, touch: TouchInput, cancelled: bool, out: &mut Vec<Event>) {
        let Some(finger) = self.fingers.remove(&touch.id) else {
            return;
        };
        let pos = touch.pos;
        match finger.role {
            Role::Ui(UiState::Pending) if !cancelled => {
                push_ui_cursor(out, self.size, finger.start);
                push_ui_button(out, Mouse::Left, true);
                push_ui_button(out, Mouse::Left, false);
                self.hide_cursor_at_frame = Some(self.frame + 2);
            },
            Role::Ui(UiState::Dragging) => {
                push_ui_cursor(out, self.size, pos);
                push_ui_button(out, Mouse::Left, false);
                self.hide_cursor_at_frame = Some(self.frame + 2);
            },
            Role::Ui(_) | Role::UiScroll | Role::Ignored | Role::Camera => {},
            Role::Joystick { .. } => {
                self.stick = Vec2::zero();
                push_stick(out, self.stick);
            },
            Role::Button(id) => {
                for input in id.inputs() {
                    out.push(Event::InputUpdate(*input, false));
                }
            },
        }
    }
}

impl Default for TouchControls {
    fn default() -> Self { Self::new() }
}

#[derive(Clone, Copy)]
enum Mouse {
    Left,
    Right,
}

fn push_stick(out: &mut Vec<Event>, stick: Vec2<f32>) {
    out.push(Event::AnalogGameInput(AnalogGameInput::MovementX(stick.x)));
    out.push(Event::AnalogGameInput(AnalogGameInput::MovementY(stick.y)));
}

/// Move the virtual mouse for both UI toolkits (conrod: origin at the window
/// centre, y up; iced: origin top-left, y down; both logical pixels).
fn push_ui_cursor(out: &mut Vec<Event>, size: Vec2<f32>, pos: Vec2<f32>) {
    let (w, h) = (size.x as f64, size.y as f64);
    out.push(Event::Ui(ui::Event(conrod_core::event::Input::Motion(
        input::Motion::MouseCursor {
            x: pos.x as f64 - w / 2.0,
            y: -(pos.y as f64 - h / 2.0),
        },
    ))));
    out.push(Event::IcedUi(iced::Event::Mouse(
        iced::mouse::Event::CursorMoved {
            position: iced::Point::new(pos.x, pos.y),
        },
    )));
}

fn push_ui_button(out: &mut Vec<Event>, button: Mouse, pressed: bool) {
    let (conrod_button, iced_button) = match button {
        Mouse::Left => (input::MouseButton::Left, iced::mouse::Button::Left),
        Mouse::Right => (input::MouseButton::Right, iced::mouse::Button::Right),
    };
    let conrod_button = input::Button::Mouse(conrod_button);
    out.push(Event::Ui(ui::Event(if pressed {
        conrod_core::event::Input::Press(conrod_button)
    } else {
        conrod_core::event::Input::Release(conrod_button)
    })));
    out.push(Event::IcedUi(iced::Event::Mouse(if pressed {
        iced::mouse::Event::ButtonPressed(iced_button)
    } else {
        iced::mouse::Event::ButtonReleased(iced_button)
    })));
}

/// Natural scrolling: `dy` is finger movement in logical pixels (down = +).
fn push_ui_scroll(out: &mut Vec<Event>, dy: f32) {
    out.push(Event::Ui(ui::Event(conrod_core::event::Input::Motion(
        input::Motion::Scroll {
            x: 0.0,
            y: -dy as f64,
        },
    ))));
    out.push(Event::IcedUi(iced::Event::Mouse(
        iced::mouse::Event::WheelScrolled {
            delta: iced::mouse::ScrollDelta::Lines {
                x: 0.0,
                y: dy / SCROLL_PX_PER_LINE,
            },
        },
    )));
}
