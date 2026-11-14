//! Enumerated property values, such as a part's shape or material.
//!
//! Like classes and properties, these are internal names. The scripting
//! layer decides what scripts call them; the file format uses
//! [`EnumValue::file_name`], which must never change.

/// The geometry of a part. Size is the bounding box in every case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PartShape {
    /// A box that fills its size.
    #[default]
    Block,
    /// A sphere whose diameter is the smallest side of its size.
    Ball,
    /// A cylinder lying along the part's X axis.
    Cylinder,
    /// A ramp: full height at the back (+Z), sloping down to the front (-Z).
    Wedge,
}

/// What a part's surface looks like.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Material {
    #[default]
    Plastic,
    SmoothPlastic,
    /// Glows in its own colour.
    Neon,
    Metal,
    /// Shiny and see-through.
    Glass,
    Wood,
    Brick,
    Grass,
    Sand,
    Rock,
    Snow,
    /// See-through; terrain water can be swum in and things float in it.
    Water,
    /// Nothing: filling terrain with air empties it.
    Air,
    Slate,
    Concrete,
    CorrodedMetal,
    DiamondPlate,
    Foil,
    Granite,
    Marble,
    Pebble,
    Cobblestone,
    Fabric,
    Ice,
    ForceField,
    WoodPlanks,
    Mud,
    Ground,
    Basalt,
    Asphalt,
    LeafyGrass,
    Salt,
    Limestone,
    Pavement,
    Sandstone,
    Glacier,
    CrackedLava,
}

/// The shape of an animation curve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum EasingStyle {
    Linear,
    Sine,
    #[default]
    Quad,
    Cubic,
    Quart,
    Quint,
    Exponential,
    Circular,
    Back,
    Bounce,
    Elastic,
}

/// Which end of an animation curve the easing applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum EasingDirection {
    In,
    #[default]
    Out,
    InOut,
}

/// A key on the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum KeyCode {
    A,
    B,
    C,
    D,
    #[default]
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    Zero,
    One,
    Two,
    Three,
    Four,
    Five,
    Six,
    Seven,
    Eight,
    Nine,
    Space,
    Return,
    Tab,
    Backspace,
    Escape,
    LeftShift,
    RightShift,
    LeftControl,
    RightControl,
    LeftAlt,
    RightAlt,
    Up,
    Down,
    Left,
    Right,
    Slash,
    /// Not a key: the code an input event has when it came from the mouse.
    Unknown,
}

/// What kind of input an input event came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum UserInputType {
    #[default]
    Keyboard,
    MouseButton1,
    MouseButton2,
    /// A finger on a touch screen.
    Touch,
    /// The first game controller.
    Gamepad1,
}

/// Whether a path search found a way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PathStatus {
    Success,
    #[default]
    NoPath,
}

/// What to do on reaching a path's waypoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PathWaypointAction {
    #[default]
    Walk,
    Jump,
}

/// Whether a raycast's filter list is skipped or is the only thing hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RaycastFilterType {
    #[default]
    Exclude,
    Include,
}

/// Whether the camera follows the character or is left to scripts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CameraType {
    /// Orbits the character; the player turns it with the mouse.
    #[default]
    Custom,
    /// Stays wherever scripts put it.
    Scriptable,
}

/// Where text sits across its box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextXAlignment {
    Left,
    #[default]
    Center,
    Right,
}

/// Where text sits up and down its box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextYAlignment {
    Top,
    #[default]
    Center,
    Bottom,
}

/// A side of a part.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum NormalId {
    /// +X.
    Right,
    /// +Y.
    Top,
    /// +Z.
    Back,
    /// -X.
    Left,
    /// -Y.
    Bottom,
    /// -Z, the way a part faces.
    #[default]
    Front,
}

/// What drives a hinge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ActuatorType {
    /// Nothing: it swings freely.
    #[default]
    None,
    /// Turns at a steady speed.
    Motor,
    /// Turns to an angle and holds it.
    Servo,
}

/// How an image fills its box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ScaleType {
    /// Stretched to the box's shape.
    #[default]
    Stretch,
    /// As big as fits, keeping its shape.
    Fit,
    /// Covering the box, keeping its shape; the overhang is cut off.
    Crop,
}

/// Where a press is in its life: just down, held and moving, or let go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum UserInputState {
    #[default]
    Begin,
    Change,
    End,
    Cancel,
    None,
}

/// Which way a list layout lines things up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FillDirection {
    #[default]
    Vertical,
    Horizontal,
}

impl KeyCode {
    pub const ALL: &[KeyCode] = &[
        KeyCode::A,
        KeyCode::B,
        KeyCode::C,
        KeyCode::D,
        KeyCode::E,
        KeyCode::F,
        KeyCode::G,
        KeyCode::H,
        KeyCode::I,
        KeyCode::J,
        KeyCode::K,
        KeyCode::L,
        KeyCode::M,
        KeyCode::N,
        KeyCode::O,
        KeyCode::P,
        KeyCode::Q,
        KeyCode::R,
        KeyCode::S,
        KeyCode::T,
        KeyCode::U,
        KeyCode::V,
        KeyCode::W,
        KeyCode::X,
        KeyCode::Y,
        KeyCode::Z,
        KeyCode::Zero,
        KeyCode::One,
        KeyCode::Two,
        KeyCode::Three,
        KeyCode::Four,
        KeyCode::Five,
        KeyCode::Six,
        KeyCode::Seven,
        KeyCode::Eight,
        KeyCode::Nine,
        KeyCode::Space,
        KeyCode::Return,
        KeyCode::Tab,
        KeyCode::Backspace,
        KeyCode::Escape,
        KeyCode::LeftShift,
        KeyCode::RightShift,
        KeyCode::LeftControl,
        KeyCode::RightControl,
        KeyCode::LeftAlt,
        KeyCode::RightAlt,
        KeyCode::Up,
        KeyCode::Down,
        KeyCode::Left,
        KeyCode::Right,
        KeyCode::Slash,
        KeyCode::Unknown,
    ];
}

impl EasingStyle {
    /// The eased-in curve: 0 at 0, 1 at 1.
    fn ease_in(self, t: f32) -> f32 {
        use std::f32::consts::PI;
        match self {
            EasingStyle::Linear => t,
            EasingStyle::Sine => 1.0 - (t * PI / 2.0).cos(),
            EasingStyle::Quad => t * t,
            EasingStyle::Cubic => t * t * t,
            EasingStyle::Quart => t.powi(4),
            EasingStyle::Quint => t.powi(5),
            EasingStyle::Exponential => {
                if t <= 0.0 {
                    0.0
                } else {
                    2f32.powf(10.0 * t - 10.0)
                }
            }
            EasingStyle::Circular => 1.0 - (1.0 - t * t).max(0.0).sqrt(),
            EasingStyle::Back => {
                let c = 1.70158;
                (c + 1.0) * t * t * t - c * t * t
            }
            EasingStyle::Bounce => 1.0 - bounce_out(1.0 - t),
            EasingStyle::Elastic => {
                if t <= 0.0 || t >= 1.0 {
                    t
                } else {
                    -(2f32.powf(10.0 * t - 10.0)) * ((t * 10.0 - 10.75) * (2.0 * PI / 3.0)).sin()
                }
            }
        }
    }

    /// Progress (0 to 1) after a fraction `t` of the time, eased in `direction`.
    pub fn ease(self, direction: EasingDirection, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match direction {
            EasingDirection::In => self.ease_in(t),
            EasingDirection::Out => 1.0 - self.ease_in(1.0 - t),
            EasingDirection::InOut => {
                if t < 0.5 {
                    self.ease_in(t * 2.0) / 2.0
                } else {
                    1.0 - self.ease_in((1.0 - t) * 2.0) / 2.0
                }
            }
        }
    }
}

fn bounce_out(t: f32) -> f32 {
    let (n, d) = (7.5625, 2.75);
    if t < 1.0 / d {
        n * t * t
    } else if t < 2.0 / d {
        let t = t - 1.5 / d;
        n * t * t + 0.75
    } else if t < 2.5 / d {
        let t = t - 2.25 / d;
        n * t * t + 0.9375
    } else {
        let t = t - 2.625 / d;
        n * t * t + 0.984375
    }
}

/// Which enumeration a value belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnumKind {
    Shape,
    Material,
    EasingStyle,
    EasingDirection,
    KeyCode,
    UserInputType,
    RaycastFilterType,
    CameraType,
    FillDirection,
    TextXAlignment,
    TextYAlignment,
    ScaleType,
    ActuatorType,
    NormalId,
    PathStatus,
    PathWaypointAction,
    UserInputState,
}

/// A value from one of the enumerations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnumValue {
    Shape(PartShape),
    Material(Material),
    EasingStyle(EasingStyle),
    EasingDirection(EasingDirection),
    KeyCode(KeyCode),
    UserInputType(UserInputType),
    RaycastFilterType(RaycastFilterType),
    CameraType(CameraType),
    FillDirection(FillDirection),
    TextXAlignment(TextXAlignment),
    TextYAlignment(TextYAlignment),
    ScaleType(ScaleType),
    ActuatorType(ActuatorType),
    NormalId(NormalId),
    PathStatus(PathStatus),
    PathWaypointAction(PathWaypointAction),
    UserInputState(UserInputState),
}

const KEY_CODES: &[EnumValue] = &[
    EnumValue::KeyCode(KeyCode::A),
    EnumValue::KeyCode(KeyCode::B),
    EnumValue::KeyCode(KeyCode::C),
    EnumValue::KeyCode(KeyCode::D),
    EnumValue::KeyCode(KeyCode::E),
    EnumValue::KeyCode(KeyCode::F),
    EnumValue::KeyCode(KeyCode::G),
    EnumValue::KeyCode(KeyCode::H),
    EnumValue::KeyCode(KeyCode::I),
    EnumValue::KeyCode(KeyCode::J),
    EnumValue::KeyCode(KeyCode::K),
    EnumValue::KeyCode(KeyCode::L),
    EnumValue::KeyCode(KeyCode::M),
    EnumValue::KeyCode(KeyCode::N),
    EnumValue::KeyCode(KeyCode::O),
    EnumValue::KeyCode(KeyCode::P),
    EnumValue::KeyCode(KeyCode::Q),
    EnumValue::KeyCode(KeyCode::R),
    EnumValue::KeyCode(KeyCode::S),
    EnumValue::KeyCode(KeyCode::T),
    EnumValue::KeyCode(KeyCode::U),
    EnumValue::KeyCode(KeyCode::V),
    EnumValue::KeyCode(KeyCode::W),
    EnumValue::KeyCode(KeyCode::X),
    EnumValue::KeyCode(KeyCode::Y),
    EnumValue::KeyCode(KeyCode::Z),
    EnumValue::KeyCode(KeyCode::Zero),
    EnumValue::KeyCode(KeyCode::One),
    EnumValue::KeyCode(KeyCode::Two),
    EnumValue::KeyCode(KeyCode::Three),
    EnumValue::KeyCode(KeyCode::Four),
    EnumValue::KeyCode(KeyCode::Five),
    EnumValue::KeyCode(KeyCode::Six),
    EnumValue::KeyCode(KeyCode::Seven),
    EnumValue::KeyCode(KeyCode::Eight),
    EnumValue::KeyCode(KeyCode::Nine),
    EnumValue::KeyCode(KeyCode::Space),
    EnumValue::KeyCode(KeyCode::Return),
    EnumValue::KeyCode(KeyCode::Tab),
    EnumValue::KeyCode(KeyCode::Backspace),
    EnumValue::KeyCode(KeyCode::Escape),
    EnumValue::KeyCode(KeyCode::LeftShift),
    EnumValue::KeyCode(KeyCode::RightShift),
    EnumValue::KeyCode(KeyCode::LeftControl),
    EnumValue::KeyCode(KeyCode::RightControl),
    EnumValue::KeyCode(KeyCode::LeftAlt),
    EnumValue::KeyCode(KeyCode::RightAlt),
    EnumValue::KeyCode(KeyCode::Up),
    EnumValue::KeyCode(KeyCode::Down),
    EnumValue::KeyCode(KeyCode::Left),
    EnumValue::KeyCode(KeyCode::Right),
    EnumValue::KeyCode(KeyCode::Slash),
    EnumValue::KeyCode(KeyCode::Unknown),
];

impl EnumKind {
    pub const ALL: &[EnumKind] = &[
        EnumKind::Shape,
        EnumKind::Material,
        EnumKind::EasingStyle,
        EnumKind::EasingDirection,
        EnumKind::KeyCode,
        EnumKind::UserInputType,
        EnumKind::RaycastFilterType,
        EnumKind::CameraType,
        EnumKind::FillDirection,
        EnumKind::TextXAlignment,
        EnumKind::TextYAlignment,
        EnumKind::ScaleType,
        EnumKind::ActuatorType,
        EnumKind::NormalId,
        EnumKind::PathStatus,
        EnumKind::PathWaypointAction,
        EnumKind::UserInputState,
    ];

    /// Every value of this enumeration, in a stable order.
    pub fn items(self) -> &'static [EnumValue] {
        use EnumValue as E;
        match self {
            EnumKind::Shape => &[
                E::Shape(PartShape::Block),
                E::Shape(PartShape::Ball),
                E::Shape(PartShape::Cylinder),
                E::Shape(PartShape::Wedge),
            ],
            EnumKind::Material => &[
                E::Material(Material::Plastic),
                E::Material(Material::SmoothPlastic),
                E::Material(Material::Neon),
                E::Material(Material::Metal),
                E::Material(Material::Glass),
                E::Material(Material::Wood),
                E::Material(Material::Brick),
                E::Material(Material::Grass),
                E::Material(Material::Sand),
                E::Material(Material::Rock),
                E::Material(Material::Snow),
                E::Material(Material::Water),
                E::Material(Material::Air),
                E::Material(Material::Slate),
                E::Material(Material::Concrete),
                E::Material(Material::CorrodedMetal),
                E::Material(Material::DiamondPlate),
                E::Material(Material::Foil),
                E::Material(Material::Granite),
                E::Material(Material::Marble),
                E::Material(Material::Pebble),
                E::Material(Material::Cobblestone),
                E::Material(Material::Fabric),
                E::Material(Material::Ice),
                E::Material(Material::ForceField),
                E::Material(Material::WoodPlanks),
                E::Material(Material::Mud),
                E::Material(Material::Ground),
                E::Material(Material::Basalt),
                E::Material(Material::Asphalt),
                E::Material(Material::LeafyGrass),
                E::Material(Material::Salt),
                E::Material(Material::Limestone),
                E::Material(Material::Pavement),
                E::Material(Material::Sandstone),
                E::Material(Material::Glacier),
                E::Material(Material::CrackedLava),
            ],
            EnumKind::EasingStyle => &[
                E::EasingStyle(EasingStyle::Linear),
                E::EasingStyle(EasingStyle::Sine),
                E::EasingStyle(EasingStyle::Quad),
                E::EasingStyle(EasingStyle::Cubic),
                E::EasingStyle(EasingStyle::Quart),
                E::EasingStyle(EasingStyle::Quint),
                E::EasingStyle(EasingStyle::Exponential),
                E::EasingStyle(EasingStyle::Circular),
                E::EasingStyle(EasingStyle::Back),
                E::EasingStyle(EasingStyle::Bounce),
                E::EasingStyle(EasingStyle::Elastic),
            ],
            EnumKind::EasingDirection => &[
                E::EasingDirection(EasingDirection::In),
                E::EasingDirection(EasingDirection::Out),
                E::EasingDirection(EasingDirection::InOut),
            ],
            EnumKind::KeyCode => KEY_CODES,
            EnumKind::UserInputType => &[
                E::UserInputType(UserInputType::Keyboard),
                E::UserInputType(UserInputType::MouseButton1),
                E::UserInputType(UserInputType::MouseButton2),
                E::UserInputType(UserInputType::Touch),
                E::UserInputType(UserInputType::Gamepad1),
            ],
            EnumKind::PathStatus => &[E::PathStatus(PathStatus::Success), E::PathStatus(PathStatus::NoPath)],
            EnumKind::PathWaypointAction => &[
                E::PathWaypointAction(PathWaypointAction::Walk),
                E::PathWaypointAction(PathWaypointAction::Jump),
            ],
            EnumKind::RaycastFilterType => &[
                E::RaycastFilterType(RaycastFilterType::Exclude),
                E::RaycastFilterType(RaycastFilterType::Include),
            ],
            EnumKind::CameraType => {
                &[E::CameraType(CameraType::Custom), E::CameraType(CameraType::Scriptable)]
            }
            EnumKind::FillDirection => {
                &[E::FillDirection(FillDirection::Vertical), E::FillDirection(FillDirection::Horizontal)]
            }
            EnumKind::UserInputState => &[
                E::UserInputState(UserInputState::Begin),
                E::UserInputState(UserInputState::Change),
                E::UserInputState(UserInputState::End),
                E::UserInputState(UserInputState::Cancel),
                E::UserInputState(UserInputState::None),
            ],
            EnumKind::TextXAlignment => &[
                E::TextXAlignment(TextXAlignment::Left),
                E::TextXAlignment(TextXAlignment::Center),
                E::TextXAlignment(TextXAlignment::Right),
            ],
            EnumKind::TextYAlignment => &[
                E::TextYAlignment(TextYAlignment::Top),
                E::TextYAlignment(TextYAlignment::Center),
                E::TextYAlignment(TextYAlignment::Bottom),
            ],
            EnumKind::ScaleType => &[
                E::ScaleType(ScaleType::Stretch),
                E::ScaleType(ScaleType::Fit),
                E::ScaleType(ScaleType::Crop),
            ],
            EnumKind::ActuatorType => &[
                E::ActuatorType(ActuatorType::None),
                E::ActuatorType(ActuatorType::Motor),
                E::ActuatorType(ActuatorType::Servo),
            ],
            EnumKind::NormalId => &[
                E::NormalId(NormalId::Right),
                E::NormalId(NormalId::Top),
                E::NormalId(NormalId::Back),
                E::NormalId(NormalId::Left),
                E::NormalId(NormalId::Bottom),
                E::NormalId(NormalId::Front),
            ],
        }
    }

    /// The value kind name used in type checks (see [`crate::PropertyValue::kind_name`]).
    pub fn kind_name(self) -> &'static str {
        match self {
            EnumKind::Shape => "enum:shape",
            EnumKind::Material => "enum:material",
            EnumKind::EasingStyle => "enum:easing_style",
            EnumKind::EasingDirection => "enum:easing_direction",
            EnumKind::KeyCode => "enum:key_code",
            EnumKind::UserInputType => "enum:user_input_type",
            EnumKind::RaycastFilterType => "enum:raycast_filter_type",
            EnumKind::CameraType => "enum:camera_type",
            EnumKind::FillDirection => "enum:fill_direction",
            EnumKind::TextXAlignment => "enum:text_x_alignment",
            EnumKind::TextYAlignment => "enum:text_y_alignment",
            EnumKind::ScaleType => "enum:scale_type",
            EnumKind::ActuatorType => "enum:actuator_type",
            EnumKind::NormalId => "enum:normal_id",
            EnumKind::PathStatus => "enum:path_status",
            EnumKind::PathWaypointAction => "enum:path_waypoint_action",
            EnumKind::UserInputState => "enum:user_input_state",
        }
    }

    /// Looks a value up by its file name.
    pub fn from_file_name(self, name: &str) -> Option<EnumValue> {
        self.items().iter().copied().find(|v| v.file_name() == name)
    }
}

impl EnumValue {
    pub fn kind(self) -> EnumKind {
        match self {
            EnumValue::Shape(_) => EnumKind::Shape,
            EnumValue::Material(_) => EnumKind::Material,
            EnumValue::EasingStyle(_) => EnumKind::EasingStyle,
            EnumValue::EasingDirection(_) => EnumKind::EasingDirection,
            EnumValue::KeyCode(_) => EnumKind::KeyCode,
            EnumValue::UserInputType(_) => EnumKind::UserInputType,
            EnumValue::RaycastFilterType(_) => EnumKind::RaycastFilterType,
            EnumValue::CameraType(_) => EnumKind::CameraType,
            EnumValue::FillDirection(_) => EnumKind::FillDirection,
            EnumValue::UserInputState(_) => EnumKind::UserInputState,
            EnumValue::TextXAlignment(_) => EnumKind::TextXAlignment,
            EnumValue::TextYAlignment(_) => EnumKind::TextYAlignment,
            EnumValue::ScaleType(_) => EnumKind::ScaleType,
            EnumValue::ActuatorType(_) => EnumKind::ActuatorType,
            EnumValue::NormalId(_) => EnumKind::NormalId,
            EnumValue::PathStatus(_) => EnumKind::PathStatus,
            EnumValue::PathWaypointAction(_) => EnumKind::PathWaypointAction,
        }
    }

    /// The position of this value in [`EnumKind::items`].
    pub fn index(self) -> usize {
        self.kind().items().iter().position(|v| *v == self).expect("every value is listed")
    }

    /// A stable name for files. Never shown to scripts; never change these.
    pub fn file_name(self) -> &'static str {
        match self {
            EnumValue::Shape(s) => match s {
                PartShape::Block => "block",
                PartShape::Ball => "ball",
                PartShape::Cylinder => "cylinder",
                PartShape::Wedge => "wedge",
            },
            EnumValue::Material(m) => match m {
                Material::Plastic => "plastic",
                Material::SmoothPlastic => "smooth_plastic",
                Material::Neon => "neon",
                Material::Metal => "metal",
                Material::Glass => "glass",
                Material::Wood => "wood",
                Material::Brick => "brick",
                Material::Grass => "grass",
                Material::Sand => "sand",
                Material::Rock => "rock",
                Material::Snow => "snow",
                Material::Water => "water",
                Material::Air => "air",
                Material::Slate => "slate",
                Material::Concrete => "concrete",
                Material::CorrodedMetal => "corroded_metal",
                Material::DiamondPlate => "diamond_plate",
                Material::Foil => "foil",
                Material::Granite => "granite",
                Material::Marble => "marble",
                Material::Pebble => "pebble",
                Material::Cobblestone => "cobblestone",
                Material::Fabric => "fabric",
                Material::Ice => "ice",
                Material::ForceField => "force_field",
                Material::WoodPlanks => "wood_planks",
                Material::Mud => "mud",
                Material::Ground => "ground",
                Material::Basalt => "basalt",
                Material::Asphalt => "asphalt",
                Material::LeafyGrass => "leafy_grass",
                Material::Salt => "salt",
                Material::Limestone => "limestone",
                Material::Pavement => "pavement",
                Material::Sandstone => "sandstone",
                Material::Glacier => "glacier",
                Material::CrackedLava => "cracked_lava",
            },
            EnumValue::EasingStyle(e) => match e {
                EasingStyle::Linear => "linear",
                EasingStyle::Sine => "sine",
                EasingStyle::Quad => "quad",
                EasingStyle::Cubic => "cubic",
                EasingStyle::Quart => "quart",
                EasingStyle::Quint => "quint",
                EasingStyle::Exponential => "exponential",
                EasingStyle::Circular => "circular",
                EasingStyle::Back => "back",
                EasingStyle::Bounce => "bounce",
                EasingStyle::Elastic => "elastic",
            },
            EnumValue::EasingDirection(d) => match d {
                EasingDirection::In => "in",
                EasingDirection::Out => "out",
                EasingDirection::InOut => "in_out",
            },
            EnumValue::KeyCode(k) => match k {
                KeyCode::A => "a",
                KeyCode::B => "b",
                KeyCode::C => "c",
                KeyCode::D => "d",
                KeyCode::E => "e",
                KeyCode::F => "f",
                KeyCode::G => "g",
                KeyCode::H => "h",
                KeyCode::I => "i",
                KeyCode::J => "j",
                KeyCode::K => "k",
                KeyCode::L => "l",
                KeyCode::M => "m",
                KeyCode::N => "n",
                KeyCode::O => "o",
                KeyCode::P => "p",
                KeyCode::Q => "q",
                KeyCode::R => "r",
                KeyCode::S => "s",
                KeyCode::T => "t",
                KeyCode::U => "u",
                KeyCode::V => "v",
                KeyCode::W => "w",
                KeyCode::X => "x",
                KeyCode::Y => "y",
                KeyCode::Z => "z",
                KeyCode::Zero => "zero",
                KeyCode::One => "one",
                KeyCode::Two => "two",
                KeyCode::Three => "three",
                KeyCode::Four => "four",
                KeyCode::Five => "five",
                KeyCode::Six => "six",
                KeyCode::Seven => "seven",
                KeyCode::Eight => "eight",
                KeyCode::Nine => "nine",
                KeyCode::Space => "space",
                KeyCode::Return => "return",
                KeyCode::Tab => "tab",
                KeyCode::Backspace => "backspace",
                KeyCode::Escape => "escape",
                KeyCode::LeftShift => "left_shift",
                KeyCode::RightShift => "right_shift",
                KeyCode::LeftControl => "left_control",
                KeyCode::RightControl => "right_control",
                KeyCode::LeftAlt => "left_alt",
                KeyCode::RightAlt => "right_alt",
                KeyCode::Up => "up",
                KeyCode::Down => "down",
                KeyCode::Left => "left",
                KeyCode::Right => "right",
                KeyCode::Slash => "slash",
                KeyCode::Unknown => "unknown",
            },
            EnumValue::UserInputType(t) => match t {
                UserInputType::Keyboard => "keyboard",
                UserInputType::MouseButton1 => "mouse_button1",
                UserInputType::MouseButton2 => "mouse_button2",
                UserInputType::Touch => "touch",
                UserInputType::Gamepad1 => "gamepad1",
            },
            EnumValue::PathStatus(s) => match s {
                PathStatus::Success => "success",
                PathStatus::NoPath => "no_path",
            },
            EnumValue::PathWaypointAction(a) => match a {
                PathWaypointAction::Walk => "walk",
                PathWaypointAction::Jump => "jump",
            },
            EnumValue::RaycastFilterType(t) => match t {
                RaycastFilterType::Exclude => "exclude",
                RaycastFilterType::Include => "include",
            },
            EnumValue::CameraType(t) => match t {
                CameraType::Custom => "custom",
                CameraType::Scriptable => "scriptable",
            },
            EnumValue::FillDirection(d) => match d {
                FillDirection::Vertical => "vertical",
                FillDirection::Horizontal => "horizontal",
            },
            EnumValue::UserInputState(s) => match s {
                UserInputState::Begin => "begin",
                UserInputState::Change => "change",
                UserInputState::End => "end",
                UserInputState::Cancel => "cancel",
                UserInputState::None => "none",
            },
            EnumValue::TextXAlignment(a) => match a {
                TextXAlignment::Left => "left",
                TextXAlignment::Center => "center",
                TextXAlignment::Right => "right",
            },
            EnumValue::TextYAlignment(a) => match a {
                TextYAlignment::Top => "top",
                TextYAlignment::Center => "center",
                TextYAlignment::Bottom => "bottom",
            },
            EnumValue::ScaleType(s) => match s {
                ScaleType::Stretch => "stretch",
                ScaleType::Fit => "fit",
                ScaleType::Crop => "crop",
            },
            EnumValue::ActuatorType(a) => match a {
                ActuatorType::None => "none",
                ActuatorType::Motor => "motor",
                ActuatorType::Servo => "servo",
            },
            EnumValue::NormalId(n) => match n {
                NormalId::Right => "right",
                NormalId::Top => "top",
                NormalId::Back => "back",
                NormalId::Left => "left",
                NormalId::Bottom => "bottom",
                NormalId::Front => "front",
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easing_curves_start_at_zero_and_end_at_one() {
        for &item in EnumKind::EasingStyle.items() {
            let EnumValue::EasingStyle(style) = item else { unreachable!() };
            for &dir in &[EasingDirection::In, EasingDirection::Out, EasingDirection::InOut] {
                assert!(style.ease(dir, 0.0).abs() < 1e-3, "{style:?} {dir:?}");
                assert!((style.ease(dir, 1.0) - 1.0).abs() < 1e-3, "{style:?} {dir:?}");
            }
        }
        assert!((EasingStyle::Linear.ease(EasingDirection::In, 0.25) - 0.25).abs() < 1e-6);
    }

    #[test]
    fn file_names_round_trip() {
        for &kind in EnumKind::ALL {
            for (i, &item) in kind.items().iter().enumerate() {
                assert_eq!(item.kind(), kind);
                assert_eq!(item.index(), i);
                assert_eq!(kind.from_file_name(item.file_name()), Some(item));
            }
        }
    }
}
