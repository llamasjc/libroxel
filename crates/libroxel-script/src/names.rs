//! The naming layer: every identifier a script can see, in one place.
//!
//! The engine has its own internal vocabulary ([`ClassKind`], [`Property`]).
//! Scripts see the names below instead. Keeping every script-facing string
//! here means the public API can be renamed, aliased, or versioned by editing
//! this one file, without touching the instance tree or the bindings.
//!
//! The names follow a common, widely-known style for scripting a scene
//! graph (an `Instance` tree, `Vector3`, services, signals with `Connect`).
//! They are written from the public descriptions of that style only.
//! See `CONTRIBUTING.md` for the clean-room rules.

use libroxel_instance::{
    ActuatorType, CameraType, ClassKind, EasingDirection, EasingStyle, EnumKind, EnumValue, FillDirection,
    KeyCode, Material, NormalId, PartShape, PathStatus, PathWaypointAction, Property, RaycastFilterType,
    ScaleType, TextXAlignment, TextYAlignment, UserInputState, UserInputType,
};

/// A weld's or motor's two offsets, as CFrames.
pub mod joint {
    pub const C0: &str = "C0";
    pub const C1: &str = "C1";
}

/// Names `IsA` answers for that are families of classes rather than
/// classes of their own (`None` for any other name).
pub fn is_a_family(class: ClassKind, name: &str) -> Option<bool> {
    use ClassKind as C;
    Some(match name {
        "Instance" | "Object" => true,
        "BasePart" | "PVInstance" if class.is_physical() => true,
        "PVInstance" => matches!(class, C::Model | C::Workspace),
        "BasePart" | "FormFactorPart" => class.is_physical() && class != C::Terrain,
        "GuiBase" | "GuiBase2d" => class.is_gui_object() || is_layer(class),
        "GuiButton" => class.is_button(),
        "LayerCollector" => is_layer(class),
        "ValueBase" => matches!(
            class,
            C::IntValue
                | C::NumberValue
                | C::StringValue
                | C::BoolValue
                | C::ObjectValue
                | C::Vector3Value
                | C::Color3Value
                | C::CFrameValue
                | C::BrickColorValue
        ),
        "LuaSourceContainer" => matches!(class, C::Script | C::LocalScript | C::ModuleScript),
        "BaseScript" => matches!(class, C::Script | C::LocalScript),
        "JointInstance" => matches!(class, C::Weld | C::Motor6D),
        "Light" => matches!(class, C::PointLight | C::SpotLight | C::SurfaceLight),
        "PostEffect" => matches!(
            class,
            C::BloomEffect
                | C::BlurEffect
                | C::ColorCorrectionEffect
                | C::SunRaysEffect
                | C::DepthOfFieldEffect
        ),
        "Constraint" => matches!(
            class,
            C::RopeConstraint
                | C::SpringConstraint
                | C::HingeConstraint
                | C::PrismaticConstraint
                | C::BallSocketConstraint
                | C::AlignPosition
                | C::AlignOrientation
                | C::LinearVelocity
                | C::AngularVelocity
                | C::VectorForce
                | C::Torque
                | C::RodConstraint
                | C::CylindricalConstraint
                | C::NoCollisionConstraint
        ),
        "BodyMover" => matches!(
            class,
            C::BodyVelocity | C::BodyPosition | C::BodyForce | C::BodyAngularVelocity | C::BodyGyro
        ),
        "UIComponent" | "UIBase" => matches!(
            class,
            C::UICorner
                | C::UIListLayout
                | C::UIGridLayout
                | C::UIStroke
                | C::UIPadding
                | C::UIAspectRatioConstraint
                | C::UIScale
                | C::UIGradient
                | C::UISizeConstraint
                | C::UITextSizeConstraint
        ),
        "GuiLabel" => matches!(class, C::TextLabel | C::ImageLabel),
        "Clothing" => matches!(class, C::Shirt | C::Pants),
        "DataModelMesh" => class == C::SpecialMesh,
        "FaceInstance" => matches!(class, C::Decal | C::Texture),
        "BackpackItem" => class == C::Tool,
        "Controller" | "ServiceProvider" => class == C::DataModel,
        _ => return None,
    })
}

fn is_layer(class: ClassKind) -> bool {
    matches!(class, ClassKind::ScreenGui | ClassKind::BillboardGui | ClassKind::SurfaceGui)
}

/// Class names `Instance.new` makes as another class: parts with special
/// meshes become plain parts (wedges keep their shape), and a manual weld
/// is a weld.
pub fn stand_in(name: &str) -> Option<(ClassKind, Option<PartShape>)> {
    Some(match name {
        "WedgePart" | "CornerWedgePart" => (ClassKind::Part, Some(PartShape::Wedge)),
        "MeshPart" | "TrussPart" | "UnionOperation" | "NegateOperation" | "PartOperation" => {
            (ClassKind::Part, None)
        }
        "ManualWeld" | "Snap" | "Glue" => (ClassKind::Weld, None),
        "SoundGroup" => (ClassKind::Folder, None),
        _ => return None,
    })
}

/// Services that are another service under a second name.
pub fn service_alias(name: &str) -> &str {
    match name {
        // Things that load first: libroxel loads everything up front.
        "ReplicatedFirst" => class_name(ClassKind::ReplicatedStorage),
        other => other,
    }
}

/// Global variables installed into every script.
pub mod globals {
    pub const GAME: &str = "game";
    pub const WORKSPACE: &str = "workspace";
    pub const INSTANCE: &str = "Instance";
    pub const VECTOR3: &str = "Vector3";
    pub const COLOR3: &str = "Color3";
    pub const PRINT: &str = "print";
    pub const WARN: &str = "warn";
    /// The scheduling library.
    pub const TASK: &str = "task";
    /// Shorthand for `task.wait`, kept because many existing scripts use it.
    pub const WAIT: &str = "wait";
    /// Seconds since the game started.
    pub const TIME: &str = "time";
    /// Inside a script: the script instance itself.
    pub const SCRIPT: &str = "script";
    /// Loads a module script and returns what it returned.
    pub const REQUIRE: &str = "require";
    /// The table of enumerations, such as `Enum.Material`.
    pub const ENUM: &str = "Enum";
    pub const CFRAME: &str = "CFrame";
    pub const VECTOR2: &str = "Vector2";
    pub const UDIM2: &str = "UDim2";
    pub const UDIM: &str = "UDim";
    /// Random number generators: `Random.new(seed?)`.
    pub const RANDOM: &str = "Random";
    pub const TWEEN_INFO: &str = "TweenInfo";
    pub const RAYCAST_PARAMS: &str = "RaycastParams";
    pub const OVERLAP_PARAMS: &str = "OverlapParams";
    /// Seconds since 1970, as a fractional number.
    pub const TICK: &str = "tick";
    /// Seconds since the game started (the same as `time()`).
    pub const ELAPSED_TIME: &str = "elapsedTime";
    /// Older spellings of `task.spawn` and `task.delay`.
    pub const SPAWN: &str = "spawn";
    pub const DELAY: &str = "delay";
}

/// Members of the scheduling library.
pub mod task {
    /// Pause the current thread for some seconds (one frame by default).
    pub const WAIT: &str = "wait";
    /// Run a function now in a new thread.
    pub const SPAWN: &str = "spawn";
    /// Run a function in a new thread after some seconds.
    pub const DELAY: &str = "delay";
    /// Run a function in a new thread at the next step.
    pub const DEFER: &str = "defer";
    /// Stop a thread from running again (one made by spawn, delay or defer).
    pub const CANCEL: &str = "cancel";
}

/// Events that instances fire.
pub mod events {
    /// On parts: fires with the other part when something starts touching it.
    pub const TOUCHED: &str = "Touched";
    /// On the players service: fires with the player when someone joins.
    pub const PLAYER_ADDED: &str = "PlayerAdded";
    /// On the players service: fires with the player just before they leave.
    pub const PLAYER_REMOVING: &str = "PlayerRemoving";
    /// On a player: fires with the new character model each time one spawns.
    pub const CHARACTER_ADDED: &str = "CharacterAdded";
    /// On a humanoid: fires once when its health reaches zero.
    pub const DIED: &str = "Died";
    /// On any instance: fires with a property's name after it changes.
    pub const CHANGED: &str = "Changed";
    /// On any instance: fires with the new child when one is added.
    pub const CHILD_ADDED: &str = "ChildAdded";
    /// On a text button: fires when it is clicked.
    pub const MOUSE_BUTTON1_CLICK: &str = "MouseButton1Click";
    /// On a text button: fires when it is clicked (same moment as the click event).
    pub const ACTIVATED: &str = "Activated";
    /// On a click detector: fires with the player who clicked its part.
    pub const MOUSE_CLICK: &str = "MouseClick";
    /// On a tween: fires when it finishes playing.
    pub const COMPLETED: &str = "Completed";
    /// On a proximity prompt: fires with the player when they press (or finish holding) its key.
    pub const TRIGGERED: &str = "Triggered";
    /// On a proximity prompt: fires with the player when they let go of its key.
    pub const TRIGGER_ENDED: &str = "TriggerEnded";
    /// On a remote event, for game scripts: fires with the player, then what they sent.
    pub const ON_SERVER_EVENT: &str = "OnServerEvent";
    /// On a remote event, for local scripts: fires with what the game sent.
    pub const ON_CLIENT_EVENT: &str = "OnClientEvent";
    /// On a bindable event: fires with whatever was passed to `Fire`.
    pub const EVENT: &str = "Event";
    /// On the input service: fires with an input object when a key or button goes down.
    pub const INPUT_BEGAN: &str = "InputBegan";
    /// On the input service: fires with an input object when a key or button comes up.
    pub const INPUT_ENDED: &str = "InputEnded";
    /// On any instance: fires with an attribute's name after it changes.
    pub const ATTRIBUTE_CHANGED: &str = "AttributeChanged";
    /// On a tool: fires with the player's mouse when the character starts holding it.
    pub const EQUIPPED: &str = "Equipped";
    /// On a tool: fires when the character stops holding it.
    pub const UNEQUIPPED: &str = "Unequipped";
    /// On a tool: fires when the player releases the mouse button while holding it.
    pub const DEACTIVATED: &str = "Deactivated";
    /// On the mouse: the left button went down or came up.
    pub const BUTTON1_DOWN: &str = "Button1Down";
    pub const BUTTON1_UP: &str = "Button1Up";
    /// On a text box: the player clicked into it.
    pub const FOCUSED: &str = "Focused";
    /// On a text box: the player clicked away or pressed Enter (passed as true).
    pub const FOCUS_LOST: &str = "FocusLost";
    /// On an explosion: fires with each part it reaches and how far away it was.
    pub const HIT: &str = "Hit";
    /// On a humanoid: fires with whether it got there after `MoveTo`.
    pub const MOVE_TO_FINISHED: &str = "MoveToFinished";
    /// On parts: fires with the other part when they stop touching.
    pub const TOUCH_ENDED: &str = "TouchEnded";
    /// On any instance: fires with a child that was taken out of it.
    pub const CHILD_REMOVED: &str = "ChildRemoved";
    /// On any instance: fires with anything put somewhere inside it.
    pub const DESCENDANT_ADDED: &str = "DescendantAdded";
    /// On any instance: fires with anything taken out from inside it.
    pub const DESCENDANT_REMOVING: &str = "DescendantRemoving";
    /// On any instance: fires when it is destroyed.
    pub const DESTROYING: &str = "Destroying";
    /// On any instance: fires with (itself, its new parent) when it or an
    /// ancestor moves to another parent.
    pub const ANCESTRY_CHANGED: &str = "AncestryChanged";
    /// On a humanoid: fires with the new health whenever it changes.
    pub const HEALTH_CHANGED: &str = "HealthChanged";
    /// On a player: fires with their character just before it is removed.
    pub const CHARACTER_REMOVING: &str = "CharacterRemoving";
    /// On a humanoid: fires with true and the seat when it sits, false and nil when it gets up.
    pub const SEATED: &str = "Seated";
    /// On a GUI object: the pointer moved onto it, or off it.
    pub const MOUSE_ENTER: &str = "MouseEnter";
    pub const MOUSE_LEAVE: &str = "MouseLeave";
}

/// Methods that only some classes have.
pub mod methods {
    /// Players service: a list of everyone in the game.
    pub const GET_PLAYERS: &str = "GetPlayers";
    /// Players service: the player whose character is the given model, or nil.
    pub const GET_PLAYER_FROM_CHARACTER: &str = "GetPlayerFromCharacter";
    /// Humanoid: lowers health by an amount, never below zero.
    pub const TAKE_DAMAGE: &str = "TakeDamage";
    /// Any instance: a signal that fires when one named property changes.
    pub const GET_PROPERTY_CHANGED_SIGNAL: &str = "GetPropertyChangedSignal";
    /// Any instance: destroys every child.
    pub const CLEAR_ALL_CHILDREN: &str = "ClearAllChildren";
    /// Any instance: the nearest ancestor with a name.
    pub const FIND_FIRST_ANCESTOR: &str = "FindFirstAncestor";
    /// Any instance: the first descendant with a name.
    pub const FIND_FIRST_DESCENDANT: &str = "FindFirstDescendant";
    /// Players service: the player on this computer.
    pub const LOCAL_PLAYER: &str = "LocalPlayer";
    /// Tween service: makes a tween.
    pub const CREATE: &str = "Create";
    /// Model: moves the whole model so its first part is at a CFrame.
    pub const PIVOT_TO: &str = "PivotTo";
    /// Model: the CFrame of its first part.
    pub const GET_PIVOT: &str = "GetPivot";
    /// Remote event, from a local script: sends to game scripts.
    pub const FIRE_SERVER: &str = "FireServer";
    /// Remote event, from a game script: sends to one player's local scripts.
    pub const FIRE_CLIENT: &str = "FireClient";
    /// Remote event, from a game script: sends to every player's local scripts.
    pub const FIRE_ALL_CLIENTS: &str = "FireAllClients";
    /// Bindable event: sends to everything listening.
    pub const FIRE: &str = "Fire";
    /// Workspace: the first part a ray hits.
    pub const RAYCAST: &str = "Raycast";
    /// Workspace: the parts in a box, a ball, or another part's space.
    pub const GET_PART_BOUNDS_IN_BOX: &str = "GetPartBoundsInBox";
    pub const GET_PART_BOUNDS_IN_RADIUS: &str = "GetPartBoundsInRadius";
    pub const GET_PARTS_IN_PART: &str = "GetPartsInPart";
    /// Part: the parts touching it.
    pub const GET_TOUCHING_PARTS: &str = "GetTouchingParts";
    /// Any instance: attach, read and list named values.
    pub const SET_ATTRIBUTE: &str = "SetAttribute";
    pub const GET_ATTRIBUTE: &str = "GetAttribute";
    pub const GET_ATTRIBUTES: &str = "GetAttributes";
    /// Any instance: a signal for one attribute changing.
    pub const GET_ATTRIBUTE_CHANGED_SIGNAL: &str = "GetAttributeChangedSignal";
    /// Any instance, and the tag service (with the instance first): labels.
    pub const ADD_TAG: &str = "AddTag";
    pub const REMOVE_TAG: &str = "RemoveTag";
    pub const HAS_TAG: &str = "HasTag";
    pub const GET_TAGS: &str = "GetTags";
    /// Tag service: every instance with a tag, and signals for tags coming and going.
    pub const GET_TAGGED: &str = "GetTagged";
    pub const GET_INSTANCE_ADDED_SIGNAL: &str = "GetInstanceAddedSignal";
    pub const GET_INSTANCE_REMOVED_SIGNAL: &str = "GetInstanceRemovedSignal";
    /// Humanoid: hold a tool, or put every held tool back in the backpack.
    pub const EQUIP_TOOL: &str = "EquipTool";
    pub const UNEQUIP_TOOLS: &str = "UnequipTools";
    /// Player: the player's mouse.
    pub const GET_MOUSE: &str = "GetMouse";
    /// Player: removes their character (if any) and spawns a fresh one now.
    pub const LOAD_CHARACTER: &str = "LoadCharacter";
    /// Player: removes them from the game, telling them why.
    pub const KICK: &str = "Kick";
    /// Seat: sits a humanoid in it.
    pub const SIT: &str = "Sit";
    /// Text box: start or stop typing into it, and whether the player is.
    pub const CAPTURE_FOCUS: &str = "CaptureFocus";
    pub const RELEASE_FOCUS: &str = "ReleaseFocus";
    pub const IS_FOCUSED: &str = "IsFocused";
    /// Part: a push, as a change in momentum (mass times speed).
    pub const APPLY_IMPULSE: &str = "ApplyImpulse";
    /// Part: how heavy it is (its volume times its density).
    pub const GET_MASS: &str = "GetMass";
    /// Camera: where a point in the world appears on screen.
    pub const WORLD_TO_VIEWPORT_POINT: &str = "WorldToViewportPoint";
    pub const WORLD_TO_SCREEN_POINT: &str = "WorldToScreenPoint";
    /// Humanoid: walk to a point (or a part, if one is given).
    pub const MOVE_TO: &str = "MoveTo";
    /// Teams service: every team.
    pub const GET_TEAMS: &str = "GetTeams";
    /// On the players service: the player with this user id.
    pub const GET_PLAYER_BY_USER_ID: &str = "GetPlayerByUserId";
}

/// Members of random number generators.
pub mod random {
    pub const NEW: &str = "new";
    /// A number from 0 to 1, or between two numbers.
    pub const NEXT_NUMBER: &str = "NextNumber";
    /// A whole number between two whole numbers, both included.
    pub const NEXT_INTEGER: &str = "NextInteger";
    pub const TYPE_NAME: &str = "Random";
}

/// Sound methods and events.
pub mod sound {
    /// Starts it from the beginning.
    pub const PLAY: &str = "Play";
    /// Stops it and goes back to the beginning.
    pub const STOP: &str = "Stop";
    /// Stops it where it is.
    pub const PAUSE: &str = "Pause";
    /// Carries on from where it was paused.
    pub const RESUME: &str = "Resume";
    /// Fires when it plays to its end (not when looped or stopped).
    pub const ENDED: &str = "Ended";
}

/// Terrain methods.
pub mod terrain {
    /// Fills a box (a CFrame and a size) with a material.
    pub const FILL_BLOCK: &str = "FillBlock";
    /// Fills a ball (a centre and a radius) with a material.
    pub const FILL_BALL: &str = "FillBall";
    /// Fills an upright cylinder (a CFrame, a height and a radius) with a material.
    pub const FILL_CYLINDER: &str = "FillCylinder";
    /// Empties all of it.
    pub const CLEAR: &str = "Clear";
}

/// Members that some classes have beyond their saved properties.
pub mod extras {
    /// Workspace: the camera the player sees through.
    pub const CURRENT_CAMERA: &str = "CurrentCamera";
    /// Workspace: its terrain (made the first time it's asked for).
    pub const TERRAIN: &str = "Terrain";
    /// Part: how fast it is moving, in studs per second.
    pub const ASSEMBLY_LINEAR_VELOCITY: &str = "AssemblyLinearVelocity";
    /// Part: how fast it is spinning, in radians per second about each axis.
    pub const ASSEMBLY_ANGULAR_VELOCITY: &str = "AssemblyAngularVelocity";
    /// Camera: the size of the view in pixels.
    pub const VIEWPORT_SIZE: &str = "ViewportSize";
    /// The name of the camera the engine puts in the workspace.
    pub const CAMERA_NAME: &str = "Camera";
    /// How heavy a part is per cubic stud.
    pub const DENSITY: f32 = 0.7;
}

/// Members of the player's mouse.
pub mod mouse {
    /// Where the mouse points in the world, as a CFrame facing along the ray.
    pub const HIT: &str = "Hit";
    /// The part under the mouse, or nil.
    pub const TARGET: &str = "Target";
    /// Pixel position on screen.
    pub const X: &str = "X";
    pub const Y: &str = "Y";
    pub const TYPE_NAME: &str = "Mouse";
    /// How far the mouse ray reaches, in studs.
    pub const REACH: f32 = 1000.0;
}

/// Limits on attribute names.
pub mod attributes {
    pub const MAX_NAME_LEN: usize = 100;
}

/// The name of the part a character holds a tool by.
pub const TOOL_HANDLE: &str = "Handle";

/// Members of the input service and its input objects.
pub mod user_input {
    pub const IS_KEY_DOWN: &str = "IsKeyDown";
    pub const IS_MOUSE_BUTTON_PRESSED: &str = "IsMouseButtonPressed";
    pub const INPUT_OBJECT_TYPE_NAME: &str = "InputObject";
    pub const KEY_CODE: &str = "KeyCode";
    pub const USER_INPUT_TYPE: &str = "UserInputType";
    /// Whether the player has a touch screen they've used.
    pub const TOUCH_ENABLED: &str = "TouchEnabled";
    /// Whether a game controller is plugged in.
    pub const GAMEPAD_ENABLED: &str = "GamepadEnabled";
    /// The kind of input the player used last.
    pub const GET_LAST_INPUT_TYPE: &str = "GetLastInputType";
}

/// Members of the saved-data service and its stores.
pub mod data_store {
    pub const GET_DATA_STORE: &str = "GetDataStore";
    pub const GET_ASYNC: &str = "GetAsync";
    pub const SET_ASYNC: &str = "SetAsync";
    pub const UPDATE_ASYNC: &str = "UpdateAsync";
    pub const INCREMENT_ASYNC: &str = "IncrementAsync";
    pub const REMOVE_ASYNC: &str = "RemoveAsync";
    pub const TYPE_NAME: &str = "DataStore";
    /// Keys and store names may be at most this long.
    pub const MAX_NAME_LEN: usize = 50;
}

/// Members of the clean-up service.
/// Remote and bindable functions: a call that waits for an answer.
pub mod functions {
    pub const INVOKE_SERVER: &str = "InvokeServer";
    pub const INVOKE_CLIENT: &str = "InvokeClient";
    pub const INVOKE: &str = "Invoke";
    /// The callbacks a script sets to answer.
    pub const ON_SERVER_INVOKE: &str = "OnServerInvoke";
    pub const ON_CLIENT_INVOKE: &str = "OnClientInvoke";
    pub const ON_INVOKE: &str = "OnInvoke";
}

/// The web helper service. It turns tables into JSON text and back.
pub mod http {
    pub const JSON_ENCODE: &str = "JSONEncode";
    pub const JSON_DECODE: &str = "JSONDecode";
    pub const GENERATE_GUID: &str = "GenerateGUID";
    pub const URL_ENCODE: &str = "UrlEncode";
}

/// Particle effects and the value types they use.
pub mod effects {
    pub const NUMBER_RANGE: &str = "NumberRange";
    pub const NUMBER_SEQUENCE: &str = "NumberSequence";
    pub const COLOR_SEQUENCE: &str = "ColorSequence";
    pub const NUMBER_KEYPOINT: &str = "NumberSequenceKeypoint";
    pub const COLOR_KEYPOINT: &str = "ColorSequenceKeypoint";
    pub const MIN: &str = "Min";
    pub const MAX: &str = "Max";
    pub const KEYPOINTS: &str = "Keypoints";
    pub const TIME: &str = "Time";
    pub const VALUE: &str = "Value";
    /// On a particle emitter: sends out this many particles at once.
    pub const EMIT: &str = "Emit";
}

/// Attachments: points on parts.
pub mod attachment {
    /// Where the attachment is in the world (read-only).
    pub const WORLD_POSITION: &str = "WorldPosition";
}

/// Read-only facts about the running game.
pub mod game_info {
    pub const JOB_ID: &str = "JobId";
    pub const PLACE_ID: &str = "PlaceId";
    pub const GAME_ID: &str = "GameId";
    pub const CREATOR_ID: &str = "CreatorId";
}

pub mod debris {
    /// Destroys an instance after some seconds (10 by default).
    pub const ADD_ITEM: &str = "AddItem";
}

/// Members of raycast parameters and results.
pub mod raycast {
    pub const PARAMS_TYPE_NAME: &str = "RaycastParams";
    pub const FILTER_DESCENDANTS_INSTANCES: &str = "FilterDescendantsInstances";
    pub const FILTER_TYPE: &str = "FilterType";
    pub const RESULT_TYPE_NAME: &str = "RaycastResult";
    pub const INSTANCE: &str = "Instance";
    pub const POSITION: &str = "Position";
    pub const NORMAL: &str = "Normal";
    pub const DISTANCE: &str = "Distance";
    pub const MATERIAL: &str = "Material";
}

/// Members of the pathfinding service, its paths and their waypoints.
pub mod pathfinding {
    pub const CREATE_PATH: &str = "CreatePath";
    pub const AGENT_RADIUS: &str = "AgentRadius";
    pub const AGENT_HEIGHT: &str = "AgentHeight";
    pub const AGENT_CAN_JUMP: &str = "AgentCanJump";
    pub const PATH_TYPE_NAME: &str = "Path";
    pub const STATUS: &str = "Status";
    pub const COMPUTE_ASYNC: &str = "ComputeAsync";
    pub const GET_WAYPOINTS: &str = "GetWaypoints";
    pub const WAYPOINT_TYPE_NAME: &str = "PathWaypoint";
    pub const POSITION: &str = "Position";
    pub const ACTION: &str = "Action";
    pub const LABEL: &str = "Label";
}

/// Members of animators and animation tracks.
pub mod animation {
    pub const LOAD_ANIMATION: &str = "LoadAnimation";
    pub const GET_PLAYING_ANIMATION_TRACKS: &str = "GetPlayingAnimationTracks";
    pub const TRACK_TYPE_NAME: &str = "AnimationTrack";
    pub const PLAY: &str = "Play";
    pub const STOP: &str = "Stop";
    pub const ADJUST_SPEED: &str = "AdjustSpeed";
    pub const ADJUST_WEIGHT: &str = "AdjustWeight";
    pub const IS_PLAYING: &str = "IsPlaying";
    pub const LENGTH: &str = "Length";
    pub const LOOPED: &str = "Looped";
    pub const SPEED: &str = "Speed";
    pub const WEIGHT: &str = "WeightCurrent";
    pub const TIME_POSITION: &str = "TimePosition";
    pub const NAME: &str = "Name";
    pub const ANIMATION: &str = "Animation";
    pub const STOPPED: &str = "Stopped";
    /// The animator every character's humanoid has.
    pub const ANIMATOR: &str = "Animator";
}

/// Members of overlap parameters (the filter members are as on raycast ones).
pub mod overlap {
    pub const PARAMS_TYPE_NAME: &str = "OverlapParams";
    pub const MAX_PARTS: &str = "MaxParts";
}

/// Members of tweens.
pub mod tween {
    pub const PLAY: &str = "Play";
    pub const PAUSE: &str = "Pause";
    pub const CANCEL: &str = "Cancel";
    pub const TYPE_NAME: &str = "Tween";
    pub const INFO_TYPE_NAME: &str = "TweenInfo";
    pub const TIME: &str = "Time";
    pub const EASING_STYLE: &str = "EasingStyle";
    pub const EASING_DIRECTION: &str = "EasingDirection";
    pub const REPEAT_COUNT: &str = "RepeatCount";
    pub const REVERSES: &str = "Reverses";
    pub const DELAY_TIME: &str = "DelayTime";
}

/// Members of the position-and-rotation type.
pub mod cframe {
    pub const TYPE_NAME: &str = "CFrame";
    pub const POSITION: &str = "Position";
    pub const X: &str = "X";
    pub const Y: &str = "Y";
    pub const Z: &str = "Z";
    pub const LOOK_VECTOR: &str = "LookVector";
    pub const RIGHT_VECTOR: &str = "RightVector";
    pub const UP_VECTOR: &str = "UpVector";
    pub const ROTATION: &str = "Rotation";
    pub const INVERSE: &str = "Inverse";
    pub const LERP: &str = "Lerp";
    pub const TO_WORLD_SPACE: &str = "ToWorldSpace";
    pub const TO_OBJECT_SPACE: &str = "ToObjectSpace";
    pub const POINT_TO_WORLD_SPACE: &str = "PointToWorldSpace";
    pub const POINT_TO_OBJECT_SPACE: &str = "PointToObjectSpace";
    pub const TO_ORIENTATION: &str = "ToOrientation";
    pub const TO_EULER_ANGLES_XYZ: &str = "ToEulerAnglesXYZ";
    pub const TO_EULER_ANGLES_YXZ: &str = "ToEulerAnglesYXZ";
    pub const TO_AXIS_ANGLE: &str = "ToAxisAngle";
    pub const GET_COMPONENTS: &str = "GetComponents";
    pub const X_VECTOR: &str = "XVector";
    pub const Y_VECTOR: &str = "YVector";
    pub const Z_VECTOR: &str = "ZVector";
    pub const FROM_EULER_ANGLES_XYZ: &str = "fromEulerAnglesXYZ";
    pub const FROM_EULER_ANGLES_YXZ: &str = "fromEulerAnglesYXZ";
    pub const FROM_MATRIX: &str = "fromMatrix";
    /// Constructors.
    pub const LOOK_AT: &str = "lookAt";
    pub const ANGLES: &str = "Angles";
    pub const FROM_ORIENTATION: &str = "fromOrientation";
    pub const FROM_AXIS_ANGLE: &str = "fromAxisAngle";
    pub const IDENTITY: &str = "identity";
}

/// Members of the 2D vector and screen-dimension types.
pub mod gui_values {
    pub const VECTOR2_TYPE_NAME: &str = "Vector2";
    pub const UDIM2_TYPE_NAME: &str = "UDim2";
    pub const UDIM_TYPE_NAME: &str = "UDim";
    pub const FROM_SCALE: &str = "fromScale";
    pub const FROM_OFFSET: &str = "fromOffset";
    pub const X: &str = "X";
    pub const Y: &str = "Y";
    /// On a UDim2 axis.
    pub const SCALE: &str = "Scale";
    pub const OFFSET: &str = "Offset";
    pub const WIDTH: &str = "Width";
    pub const HEIGHT: &str = "Height";
}

/// The extra member every part has: its position and rotation together.
pub const PART_CFRAME: &str = "CFrame";

/// Members of enum items.
pub mod enum_item {
    pub const NAME: &str = "Name";
    pub const VALUE: &str = "Value";
    pub const ENUM_TYPE: &str = "EnumType";
    pub const TYPE_NAME: &str = "EnumItem";
}

/// The name scripts use for each enumeration, as in `Enum.<name>`.
pub fn enum_name(kind: EnumKind) -> &'static str {
    match kind {
        EnumKind::Shape => "PartType",
        EnumKind::Material => "Material",
        EnumKind::EasingStyle => "EasingStyle",
        EnumKind::EasingDirection => "EasingDirection",
        EnumKind::KeyCode => "KeyCode",
        EnumKind::UserInputType => "UserInputType",
        EnumKind::RaycastFilterType => "RaycastFilterType",
        EnumKind::CameraType => "CameraType",
        EnumKind::FillDirection => "FillDirection",
        EnumKind::UserInputState => "UserInputState",
        EnumKind::TextXAlignment => "TextXAlignment",
        EnumKind::TextYAlignment => "TextYAlignment",
        EnumKind::ScaleType => "ScaleType",
        EnumKind::ActuatorType => "ActuatorType",
        EnumKind::NormalId => "NormalId",
        EnumKind::PathStatus => "PathStatus",
        EnumKind::PathWaypointAction => "PathWaypointAction",
    }
}

/// The enumeration a script-facing name refers to.
pub fn enum_from_name(name: &str) -> Option<EnumKind> {
    EnumKind::ALL.iter().copied().find(|k| enum_name(*k) == name)
}

/// The name scripts use for each enum value, as in `Enum.Material.<name>`.
pub fn enum_item_name(value: EnumValue) -> &'static str {
    match value {
        EnumValue::Shape(s) => match s {
            PartShape::Block => "Block",
            PartShape::Ball => "Ball",
            PartShape::Cylinder => "Cylinder",
            PartShape::Wedge => "Wedge",
        },
        EnumValue::Material(m) => match m {
            Material::Plastic => "Plastic",
            Material::SmoothPlastic => "SmoothPlastic",
            Material::Neon => "Neon",
            Material::Metal => "Metal",
            Material::Glass => "Glass",
            Material::Wood => "Wood",
            Material::Brick => "Brick",
            Material::Grass => "Grass",
            Material::Sand => "Sand",
            Material::Rock => "Rock",
            Material::Snow => "Snow",
            Material::Water => "Water",
            Material::Air => "Air",
        },
        EnumValue::EasingStyle(e) => match e {
            EasingStyle::Linear => "Linear",
            EasingStyle::Sine => "Sine",
            EasingStyle::Quad => "Quad",
            EasingStyle::Cubic => "Cubic",
            EasingStyle::Quart => "Quart",
            EasingStyle::Quint => "Quint",
            EasingStyle::Exponential => "Exponential",
            EasingStyle::Circular => "Circular",
            EasingStyle::Back => "Back",
            EasingStyle::Bounce => "Bounce",
            EasingStyle::Elastic => "Elastic",
        },
        EnumValue::EasingDirection(d) => match d {
            EasingDirection::In => "In",
            EasingDirection::Out => "Out",
            EasingDirection::InOut => "InOut",
        },
        EnumValue::KeyCode(k) => match k {
            KeyCode::A => "A",
            KeyCode::B => "B",
            KeyCode::C => "C",
            KeyCode::D => "D",
            KeyCode::E => "E",
            KeyCode::F => "F",
            KeyCode::G => "G",
            KeyCode::H => "H",
            KeyCode::I => "I",
            KeyCode::J => "J",
            KeyCode::K => "K",
            KeyCode::L => "L",
            KeyCode::M => "M",
            KeyCode::N => "N",
            KeyCode::O => "O",
            KeyCode::P => "P",
            KeyCode::Q => "Q",
            KeyCode::R => "R",
            KeyCode::S => "S",
            KeyCode::T => "T",
            KeyCode::U => "U",
            KeyCode::V => "V",
            KeyCode::W => "W",
            KeyCode::X => "X",
            KeyCode::Y => "Y",
            KeyCode::Z => "Z",
            KeyCode::Zero => "Zero",
            KeyCode::One => "One",
            KeyCode::Two => "Two",
            KeyCode::Three => "Three",
            KeyCode::Four => "Four",
            KeyCode::Five => "Five",
            KeyCode::Six => "Six",
            KeyCode::Seven => "Seven",
            KeyCode::Eight => "Eight",
            KeyCode::Nine => "Nine",
            KeyCode::Space => "Space",
            KeyCode::Return => "Return",
            KeyCode::Tab => "Tab",
            KeyCode::Backspace => "Backspace",
            KeyCode::Escape => "Escape",
            KeyCode::LeftShift => "LeftShift",
            KeyCode::RightShift => "RightShift",
            KeyCode::LeftControl => "LeftControl",
            KeyCode::RightControl => "RightControl",
            KeyCode::LeftAlt => "LeftAlt",
            KeyCode::RightAlt => "RightAlt",
            KeyCode::Up => "Up",
            KeyCode::Down => "Down",
            KeyCode::Left => "Left",
            KeyCode::Right => "Right",
            KeyCode::Slash => "Slash",
            KeyCode::Unknown => "Unknown",
        },
        EnumValue::UserInputType(t) => match t {
            UserInputType::Keyboard => "Keyboard",
            UserInputType::MouseButton1 => "MouseButton1",
            UserInputType::MouseButton2 => "MouseButton2",
            UserInputType::Touch => "Touch",
            UserInputType::Gamepad1 => "Gamepad1",
        },
        EnumValue::PathStatus(s) => match s {
            PathStatus::Success => "Success",
            PathStatus::NoPath => "NoPath",
        },
        EnumValue::PathWaypointAction(a) => match a {
            PathWaypointAction::Walk => "Walk",
            PathWaypointAction::Jump => "Jump",
        },
        EnumValue::RaycastFilterType(t) => match t {
            RaycastFilterType::Exclude => "Exclude",
            RaycastFilterType::Include => "Include",
        },
        EnumValue::CameraType(t) => match t {
            CameraType::Custom => "Custom",
            CameraType::Scriptable => "Scriptable",
        },
        EnumValue::TextXAlignment(a) => match a {
            TextXAlignment::Left => "Left",
            TextXAlignment::Center => "Center",
            TextXAlignment::Right => "Right",
        },
        EnumValue::TextYAlignment(a) => match a {
            TextYAlignment::Top => "Top",
            TextYAlignment::Center => "Center",
            TextYAlignment::Bottom => "Bottom",
        },
        EnumValue::NormalId(n) => match n {
            NormalId::Right => "Right",
            NormalId::Top => "Top",
            NormalId::Back => "Back",
            NormalId::Left => "Left",
            NormalId::Bottom => "Bottom",
            NormalId::Front => "Front",
        },
        EnumValue::ActuatorType(a) => match a {
            ActuatorType::None => "None",
            ActuatorType::Motor => "Motor",
            ActuatorType::Servo => "Servo",
        },
        EnumValue::ScaleType(s) => match s {
            ScaleType::Stretch => "Stretch",
            ScaleType::Fit => "Fit",
            ScaleType::Crop => "Crop",
        },
        EnumValue::FillDirection(d) => match d {
            FillDirection::Vertical => "Vertical",
            FillDirection::Horizontal => "Horizontal",
        },
        EnumValue::UserInputState(s) => match s {
            UserInputState::Begin => "Begin",
            UserInputState::Change => "Change",
            UserInputState::End => "End",
            UserInputState::Cancel => "Cancel",
            UserInputState::None => "None",
        },
    }
}

/// The value of `kind` that a script-facing item name refers to.
pub fn enum_item_from_name(kind: EnumKind, name: &str) -> Option<EnumValue> {
    kind.items().iter().copied().find(|v| enum_item_name(*v) == name)
}

/// Names of the instances the engine builds for each player.
pub mod character {
    pub const MODEL: &str = "Player";
    pub const ROOT_PART: &str = "HumanoidRootPart";
    pub const HUMANOID: &str = "Humanoid";
    /// The body colours object every character has.
    pub const BODY_COLORS: &str = "BodyColors";
    /// The attribute on an accessory's part saying where it sits on the head.
    pub const HEAD_OFFSET: &str = "HeadOffset";
    /// The attribute on an accessory's part saying how it's turned on the head.
    pub const HEAD_ORIENTATION: &str = "HeadOrientation";
}

/// Constructors and constants on the global library tables.
pub mod constructors {
    pub const NEW: &str = "new";
    pub const FROM_RGB: &str = "fromRGB";
    pub const ZERO: &str = "zero";
    pub const ONE: &str = "one";
}

/// Members that every instance has, beyond its class properties.
pub mod instance {
    pub const CLASS_NAME: &str = "ClassName";
    pub const PARENT: &str = "Parent";
    pub const FIND_FIRST_CHILD: &str = "FindFirstChild";
    pub const FIND_FIRST_CHILD_OF_CLASS: &str = "FindFirstChildOfClass";
    pub const GET_CHILDREN: &str = "GetChildren";
    pub const GET_DESCENDANTS: &str = "GetDescendants";
    pub const IS_A: &str = "IsA";
    pub const IS_DESCENDANT_OF: &str = "IsDescendantOf";
    pub const GET_FULL_NAME: &str = "GetFullName";
    pub const DESTROY: &str = "Destroy";
    pub const CLONE: &str = "Clone";
    /// Yields until a child with the given name exists.
    pub const WAIT_FOR_CHILD: &str = "WaitForChild";
    /// Only on the root instance.
    pub const GET_SERVICE: &str = "GetService";
    pub const FIND_FIRST_CHILD_WHICH_IS_A: &str = "FindFirstChildWhichIsA";
    pub const FIND_FIRST_ANCESTOR_OF_CLASS: &str = "FindFirstAncestorOfClass";
    pub const FIND_FIRST_ANCESTOR_WHICH_IS_A: &str = "FindFirstAncestorWhichIsA";
    pub const IS_ANCESTOR_OF: &str = "IsAncestorOf";
}

/// Members of the 3D vector type.
pub mod vector3 {
    pub const X: &str = "X";
    pub const Y: &str = "Y";
    pub const Z: &str = "Z";
    pub const MAGNITUDE: &str = "Magnitude";
    pub const UNIT: &str = "Unit";
    pub const DOT: &str = "Dot";
    pub const CROSS: &str = "Cross";
    pub const LERP: &str = "Lerp";
    pub const MIN: &str = "Min";
    pub const MAX: &str = "Max";
    pub const ABS: &str = "Abs";
    pub const FLOOR: &str = "Floor";
    pub const CEIL: &str = "Ceil";
    pub const SIGN: &str = "Sign";
    pub const FUZZY_EQ: &str = "FuzzyEq";
    pub const ANGLE: &str = "Angle";
    pub const X_AXIS: &str = "xAxis";
    pub const Y_AXIS: &str = "yAxis";
    pub const Z_AXIS: &str = "zAxis";
    pub const TYPE_NAME: &str = "Vector3";
}

/// Members of the colour type.
pub mod color3 {
    pub const R: &str = "R";
    pub const G: &str = "G";
    pub const B: &str = "B";
    pub const LERP: &str = "Lerp";
    pub const TO_HSV: &str = "ToHSV";
    pub const TO_HEX: &str = "ToHex";
    pub const FROM_HSV: &str = "fromHSV";
    pub const FROM_HEX: &str = "fromHex";
    pub const TYPE_NAME: &str = "Color3";
}

/// Services reachable through the root's `GetService`.
pub mod services {
    pub const RUN_SERVICE: &str = "RunService";
    pub const TWEEN_SERVICE: &str = "TweenService";
    pub const USER_INPUT_SERVICE: &str = "UserInputService";
    pub const DATA_STORE_SERVICE: &str = "DataStoreService";
    pub const DEBRIS: &str = "Debris";
    pub const COLLECTION_SERVICE: &str = "CollectionService";
    pub const HTTP_SERVICE: &str = "HttpService";
    pub const PATHFINDING_SERVICE: &str = "PathfindingService";
    /// Fires once per frame with the frame's delta time in seconds.
    pub const HEARTBEAT: &str = "Heartbeat";
    /// Fires each frame before physics, with the game time and the frame time.
    pub const STEPPED: &str = "Stepped";
    /// Fires each frame after everything else, with the frame time.
    pub const RENDER_STEPPED: &str = "RenderStepped";
    /// There is one computer, so both are always true.
    pub const IS_CLIENT: &str = "IsClient";
    pub const IS_SERVER: &str = "IsServer";
    /// True: games only run inside the editor for now.
    pub const IS_STUDIO: &str = "IsStudio";
}

/// Members of signals and connections.
pub mod signal {
    pub const CONNECT: &str = "Connect";
    pub const DISCONNECT: &str = "Disconnect";
    pub const CONNECTED: &str = "Connected";
    /// Like Connect, but only for the next time the signal fires.
    pub const ONCE: &str = "Once";
    /// Yields until the signal fires, then returns what it fired with.
    pub const WAIT: &str = "Wait";
}

/// What `typeof` reports for an instance.
pub const INSTANCE_TYPE_NAME: &str = "Instance";

/// The script-facing name of each class.
pub fn class_name(class: ClassKind) -> &'static str {
    match class {
        ClassKind::DataModel => "DataModel",
        ClassKind::Workspace => "Workspace",
        ClassKind::Folder => "Folder",
        ClassKind::Model => "Model",
        ClassKind::Part => "Part",
        ClassKind::SpawnLocation => "SpawnLocation",
        ClassKind::Script => "Script",
        ClassKind::Humanoid => "Humanoid",
        ClassKind::ModuleScript => "ModuleScript",
        ClassKind::Players => "Players",
        ClassKind::Player => "Player",
        ClassKind::ReplicatedStorage => "ReplicatedStorage",
        ClassKind::IntValue => "IntValue",
        ClassKind::NumberValue => "NumberValue",
        ClassKind::StringValue => "StringValue",
        ClassKind::BoolValue => "BoolValue",
        ClassKind::StarterGui => "StarterGui",
        ClassKind::Lighting => "Lighting",
        ClassKind::PlayerGui => "PlayerGui",
        ClassKind::ScreenGui => "ScreenGui",
        ClassKind::GuiObject => "GuiObject",
        ClassKind::Frame => "Frame",
        ClassKind::TextLabel => "TextLabel",
        ClassKind::TextButton => "TextButton",
        ClassKind::ClickDetector => "ClickDetector",
        ClassKind::ServerScriptService => "ServerScriptService",
        ClassKind::LocalScript => "LocalScript",
        ClassKind::RemoteEvent => "RemoteEvent",
        ClassKind::BindableEvent => "BindableEvent",
        ClassKind::ProximityPrompt => "ProximityPrompt",
        ClassKind::Tool => "Tool",
        ClassKind::StarterPack => "StarterPack",
        ClassKind::Backpack => "Backpack",
        ClassKind::BillboardGui => "BillboardGui",
        ClassKind::Camera => "Camera",
        ClassKind::PointLight => "PointLight",
        ClassKind::SpotLight => "SpotLight",
        ClassKind::TextBox => "TextBox",
        ClassKind::UICorner => "UICorner",
        ClassKind::UIListLayout => "UIListLayout",
        ClassKind::Explosion => "Explosion",
        ClassKind::Teams => "Teams",
        ClassKind::Team => "Team",
        ClassKind::WeldConstraint => "WeldConstraint",
        ClassKind::Seat => "Seat",
        ClassKind::VehicleSeat => "VehicleSeat",
        ClassKind::RemoteFunction => "RemoteFunction",
        ClassKind::BindableFunction => "BindableFunction",
        ClassKind::ObjectValue => "ObjectValue",
        ClassKind::Vector3Value => "Vector3Value",
        ClassKind::Color3Value => "Color3Value",
        ClassKind::ServerStorage => "ServerStorage",
        ClassKind::StarterPlayer => "StarterPlayer",
        ClassKind::Attachment => "Attachment",
        ClassKind::LinearVelocity => "LinearVelocity",
        ClassKind::VectorForce => "VectorForce",
        ClassKind::AlignPosition => "AlignPosition",
        ClassKind::RopeConstraint => "RopeConstraint",
        ClassKind::SpringConstraint => "SpringConstraint",
        ClassKind::BodyVelocity => "BodyVelocity",
        ClassKind::HingeConstraint => "HingeConstraint",
        ClassKind::PrismaticConstraint => "PrismaticConstraint",
        ClassKind::BallSocketConstraint => "BallSocketConstraint",
        ClassKind::AngularVelocity => "AngularVelocity",
        ClassKind::BodyAngularVelocity => "BodyAngularVelocity",
        ClassKind::Terrain => "Terrain",
        ClassKind::Sound => "Sound",
        ClassKind::Decal => "Decal",
        ClassKind::Texture => "Texture",
        ClassKind::BodyPosition => "BodyPosition",
        ClassKind::BodyForce => "BodyForce",
        ClassKind::ParticleEmitter => "ParticleEmitter",
        ClassKind::Fire => "Fire",
        ClassKind::Smoke => "Smoke",
        ClassKind::Sparkles => "Sparkles",
        ClassKind::Highlight => "Highlight",
        ClassKind::ScrollingFrame => "ScrollingFrame",
        ClassKind::ImageLabel => "ImageLabel",
        ClassKind::ImageButton => "ImageButton",
        ClassKind::UIStroke => "UIStroke",
        ClassKind::UIPadding => "UIPadding",
        ClassKind::UIGridLayout => "UIGridLayout",
        ClassKind::BodyColors => "BodyColors",
        ClassKind::Accessory => "Accessory",
        ClassKind::Animation => "Animation",
        ClassKind::Animator => "Animator",
        ClassKind::UIAspectRatioConstraint => "UIAspectRatioConstraint",
        ClassKind::UIScale => "UIScale",
        ClassKind::Weld => "Weld",
        ClassKind::Motor6D => "Motor6D",
        ClassKind::Configuration => "Configuration",
        ClassKind::CFrameValue => "CFrameValue",
        ClassKind::BrickColorValue => "BrickColorValue",
        ClassKind::Sky => "Sky",
        ClassKind::Atmosphere => "Atmosphere",
        ClassKind::BloomEffect => "BloomEffect",
        ClassKind::BlurEffect => "BlurEffect",
        ClassKind::ColorCorrectionEffect => "ColorCorrectionEffect",
        ClassKind::SunRaysEffect => "SunRaysEffect",
        ClassKind::DepthOfFieldEffect => "DepthOfFieldEffect",
        ClassKind::Beam => "Beam",
        ClassKind::Trail => "Trail",
        ClassKind::SelectionBox => "SelectionBox",
        ClassKind::SurfaceGui => "SurfaceGui",
        ClassKind::SurfaceLight => "SurfaceLight",
        ClassKind::UIGradient => "UIGradient",
        ClassKind::UISizeConstraint => "UISizeConstraint",
        ClassKind::UITextSizeConstraint => "UITextSizeConstraint",
        ClassKind::BodyGyro => "BodyGyro",
        ClassKind::AlignOrientation => "AlignOrientation",
        ClassKind::Torque => "Torque",
        ClassKind::NoCollisionConstraint => "NoCollisionConstraint",
        ClassKind::RodConstraint => "RodConstraint",
        ClassKind::CylindricalConstraint => "CylindricalConstraint",
        ClassKind::SpecialMesh => "SpecialMesh",
        ClassKind::Shirt => "Shirt",
        ClassKind::Pants => "Pants",
        ClassKind::StarterPlayerScripts => "StarterPlayerScripts",
        ClassKind::StarterCharacterScripts => "StarterCharacterScripts",
        ClassKind::PlayerScripts => "PlayerScripts",
        ClassKind::ForceField => "ForceField",
    }
}

/// The class a script-facing name refers to.
pub fn class_from_name(name: &str) -> Option<ClassKind> {
    ClassKind::ALL.iter().copied().find(|c| class_name(*c) == name)
}

/// The script-facing name of each property.
pub fn property_name(property: Property) -> &'static str {
    match property {
        Property::Name => "Name",
        Property::Position => "Position",
        Property::Size => "Size",
        Property::Color => "Color",
        Property::Anchored => "Anchored",
        Property::Transparency => "Transparency",
        Property::Source => "Source",
        Property::CanCollide => "CanCollide",
        Property::Reflectance => "Reflectance",
        Property::CastShadow => "CastShadow",
        Property::CanTouch => "CanTouch",
        Property::CanQuery => "CanQuery",
        Property::Massless => "Massless",
        Property::Locked => "Locked",
        Property::CollisionGroup => "CollisionGroup",
        Property::Disabled => "Disabled",
        Property::Health => "Health",
        Property::MaxHealth => "MaxHealth",
        Property::WalkSpeed => "WalkSpeed",
        Property::JumpPower => "JumpPower",
        Property::Orientation => "Orientation",
        Property::Shape => "Shape",
        Property::Material => "Material",
        // Every value object calls its value `Value`, whatever its type.
        Property::NumberValue
        | Property::StringValue
        | Property::BoolValue
        | Property::ObjectValue
        | Property::Vector3Value
        | Property::Color3Value => "Value",
        Property::Character => "Character",
        Property::UserId => "UserId",
        Property::GuiPosition => "Position",
        Property::GuiSize => "Size",
        Property::AnchorPoint => "AnchorPoint",
        Property::BackgroundColor => "BackgroundColor3",
        Property::BackgroundTransparency => "BackgroundTransparency",
        Property::Visible => "Visible",
        Property::Text => "Text",
        Property::TextColor => "TextColor3",
        Property::TextSize => "TextSize",
        Property::Enabled => "Enabled",
        Property::ResetOnSpawn => "ResetOnSpawn",
        Property::MaxActivationDistance => "MaxActivationDistance",
        Property::ClockTime => "ClockTime",
        Property::Brightness => "Brightness",
        Property::ActionText => "ActionText",
        Property::ObjectText => "ObjectText",
        Property::KeyboardKeyCode => "KeyboardKeyCode",
        Property::HoldDuration => "HoldDuration",
        Property::StudsOffset => "StudsOffset",
        Property::AlwaysOnTop => "AlwaysOnTop",
        Property::MaxDistance => "MaxDistance",
        Property::LayoutOrder => "LayoutOrder",
        Property::PlaceholderText => "PlaceholderText",
        Property::ClearTextOnFocus => "ClearTextOnFocus",
        Property::FieldOfView => "FieldOfView",
        Property::CameraType => "CameraType",
        Property::Range => "Range",
        Property::Angle => "Angle",
        Property::CornerRadius => "CornerRadius",
        Property::FillDirection => "FillDirection",
        Property::Padding => "Padding",
        Property::BlastRadius => "BlastRadius",
        Property::BlastPressure => "BlastPressure",
        Property::Jump => "Jump",
        Property::Team => "Team",
        Property::Neutral => "Neutral",
        Property::TeamColor => "TeamColor",
        Property::AutoAssignable => "AutoAssignable",
        Property::Gravity => "Gravity",
        Property::RespawnTime => "RespawnTime",
        Property::CharacterAutoLoads => "CharacterAutoLoads",
        Property::Part0 => "Part0",
        Property::Part1 => "Part1",
        Property::Occupant => "Occupant",
        Property::Sit => "Sit",
        Property::SeatPart => "SeatPart",
        Property::MaxSpeed => "MaxSpeed",
        Property::Torque => "Torque",
        Property::TurnSpeed => "TurnSpeed",
        Property::Throttle => "Throttle",
        Property::Steer => "Steer",
        Property::CharacterWalkSpeed => "CharacterWalkSpeed",
        Property::CharacterJumpPower => "CharacterJumpPower",
        Property::Ambient => "Ambient",
        Property::FogColor => "FogColor",
        Property::FogStart => "FogStart",
        Property::FogEnd => "FogEnd",
        Property::FallenPartsDestroyHeight => "FallenPartsDestroyHeight",
        Property::Attachment0 => "Attachment0",
        Property::Attachment1 => "Attachment1",
        Property::VectorVelocity => "VectorVelocity",
        Property::MaxForce => "MaxForce",
        Property::Force => "Force",
        Property::MaxVelocity => "MaxVelocity",
        Property::Responsiveness => "Responsiveness",
        Property::RigidityEnabled => "RigidityEnabled",
        Property::Length => "Length",
        Property::FreeLength => "FreeLength",
        Property::Stiffness => "Stiffness",
        Property::Damping => "Damping",
        Property::Thickness => "Thickness",
        Property::Velocity => "Velocity",
        Property::BodyMaxForce => "MaxForce",
        Property::BodyPower => "P",
        Property::BodyDamping => "D",
        Property::ActuatorType => "ActuatorType",
        // A hinge's motor speed; spinners use the Vector3 one below.
        Property::MotorVelocity => "AngularVelocity",
        Property::MotorMaxTorque => "MotorMaxTorque",
        Property::TargetAngle => "TargetAngle",
        Property::AngularSpeed => "AngularSpeed",
        Property::ServoMaxTorque => "ServoMaxTorque",
        Property::CurrentAngle => "CurrentAngle",
        Property::LimitsEnabled => "LimitsEnabled",
        Property::LowerLimit => "LowerLimit",
        Property::UpperLimit => "UpperLimit",
        Property::CurrentPosition => "CurrentPosition",
        Property::AngularVelocity => "AngularVelocity",
        Property::MaxTorque => "MaxTorque",
        Property::BodyMaxTorque => "MaxTorque",
        Property::SoundId => "SoundId",
        Property::AnimationId => "AnimationId",
        Property::HeadColor => "HeadColor3",
        Property::TorsoColor => "TorsoColor3",
        Property::LeftArmColor => "LeftArmColor3",
        Property::RightArmColor => "RightArmColor3",
        Property::LeftLegColor => "LeftLegColor3",
        Property::RightLegColor => "RightLegColor3",
        Property::Volume => "Volume",
        Property::PlaybackSpeed => "PlaybackSpeed",
        Property::Looped => "Looped",
        Property::Playing => "Playing",
        Property::Texture => "Texture",
        Property::Face => "Face",
        Property::DecalColor => "Color3",
        Property::StudsPerTileU => "StudsPerTileU",
        Property::StudsPerTileV => "StudsPerTileV",
        Property::Rate => "Rate",
        Property::Lifetime => "Lifetime",
        Property::Speed => "Speed",
        Property::SpreadAngle => "SpreadAngle",
        Property::EffectSize => "Size",
        Property::Acceleration => "Acceleration",
        Property::Drag => "Drag",
        Property::LightEmission => "LightEmission",
        Property::Heat => "Heat",
        Property::SecondaryColor => "SecondaryColor",
        Property::Opacity => "Opacity",
        Property::RiseVelocity => "RiseVelocity",
        Property::SparkleColor => "SparkleColor",
        Property::Adornee => "Adornee",
        Property::FillColor => "FillColor",
        Property::FillTransparency => "FillTransparency",
        Property::OutlineColor => "OutlineColor",
        Property::OutlineTransparency => "OutlineTransparency",
        Property::ZIndex => "ZIndex",
        Property::BorderSizePixel => "BorderSizePixel",
        Property::BorderColor => "BorderColor3",
        Property::ClipsDescendants => "ClipsDescendants",
        Property::TextTransparency => "TextTransparency",
        Property::TextXAlignment => "TextXAlignment",
        Property::TextYAlignment => "TextYAlignment",
        Property::TextWrapped => "TextWrapped",
        Property::TextScaled => "TextScaled",
        Property::CanvasSize => "CanvasSize",
        Property::CanvasPosition => "CanvasPosition",
        Property::ScrollBarThickness => "ScrollBarThickness",
        Property::Image => "Image",
        Property::ImageColor => "ImageColor3",
        Property::ImageTransparency => "ImageTransparency",
        Property::ScaleType => "ScaleType",
        Property::PaddingLeft => "PaddingLeft",
        Property::PaddingRight => "PaddingRight",
        Property::PaddingTop => "PaddingTop",
        Property::PaddingBottom => "PaddingBottom",
        Property::CellSize => "CellSize",
        Property::CellPadding => "CellPadding",
        Property::AspectRatio => "AspectRatio",
        Property::Scale => "Scale",
        // A joint's C0 and C1 are CFrames; scripts reach them through
        // `joint::C0` and `joint::C1`, made from these halves.
        Property::C0Position => "C0Position",
        Property::C0Orientation => "C0Orientation",
        Property::C1Position => "C1Position",
        Property::C1Orientation => "C1Orientation",
        Property::DesiredAngle => "DesiredAngle",
        Property::PrimaryPart => "PrimaryPart",
        Property::Density => "Density",
        Property::Haze => "Haze",
        Property::BloomIntensity => "Intensity",
        Property::BloomSize => "Size",
        Property::Threshold => "Threshold",
        Property::Contrast => "Contrast",
        Property::Saturation => "Saturation",
        Property::TintColor => "TintColor",
        Property::SelectionColor => "Color3",
        Property::LineThickness => "LineThickness",
        Property::Width0 => "Width0",
        Property::Width1 => "Width1",
        Property::MinSize => "MinSize",
        Property::MaxSize => "MaxSize",
        Property::MinTextSize => "MinTextSize",
        Property::MaxTextSize => "MaxTextSize",
        Property::StarCount => "StarCount",
        Property::CelestialBodiesShown => "CelestialBodiesShown",
        Property::SunAngularSize => "SunAngularSize",
        Property::MoonAngularSize => "MoonAngularSize",
        Property::TrailLifetime => "Lifetime",
        Property::PixelsPerStud => "PixelsPerStud",
        Property::SpawnShield => "Duration",
        Property::TorqueVector => "Torque",
        Property::TargetOrientation => "TargetOrientation",
        Property::GradientColor0 => "Color0",
        Property::GradientColor1 => "Color1",
        Property::GradientTransparency0 => "Transparency0",
        Property::GradientTransparency1 => "Transparency1",
        Property::GradientRotation => "Rotation",
        Property::GradientOffset => "Offset",
        Property::MeshId => "MeshId",
    }
}

/// The property of `class` that a script-facing name refers to.
pub fn property_from_name(class: ClassKind, name: &str) -> Option<Property> {
    class.properties().iter().copied().find(|p| property_name(*p) == name)
}

/// The name scripts see for a kind of property value.
pub fn value_kind_name(kind: &str) -> &str {
    if let Some(e) = EnumKind::ALL.iter().find(|e| e.kind_name() == kind) {
        return enum_name(*e);
    }
    match kind {
        "Vec3" => vector3::TYPE_NAME,
        "Color3" => color3::TYPE_NAME,
        "bool" => "boolean",
        "ref" => INSTANCE_TYPE_NAME,
        "Vec2" => gui_values::VECTOR2_TYPE_NAME,
        "UDim2" => gui_values::UDIM2_TYPE_NAME,
        "UDim" => gui_values::UDIM_TYPE_NAME,
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enum_names_round_trip() {
        for &kind in EnumKind::ALL {
            assert_eq!(enum_from_name(enum_name(kind)), Some(kind));
            for &item in kind.items() {
                assert_eq!(enum_item_from_name(kind, enum_item_name(item)), Some(item));
            }
        }
    }

    #[test]
    fn class_names_round_trip() {
        for &class in ClassKind::ALL {
            assert_eq!(class_from_name(class_name(class)), Some(class));
        }
    }

    #[test]
    fn property_names_round_trip() {
        for &class in ClassKind::ALL {
            for &property in class.properties() {
                assert_eq!(property_from_name(class, property_name(property)), Some(property));
            }
        }
    }
}

/// Older and less-used parts of the scripting API (see `compat.rs`).
pub mod compat {
    pub const BRICK_COLOR: &str = "BrickColor";
    pub const RAY: &str = "Ray";
    pub const DATE_TIME: &str = "DateTime";
    pub const PHYSICAL_PROPERTIES: &str = "PhysicalProperties";

    /// BrickColor members and constructors.
    pub const NAME: &str = "Name";
    pub const NUMBER: &str = "Number";
    pub const COLOR: &str = "Color";
    pub const R: &str = "r";
    pub const G: &str = "g";
    pub const B: &str = "b";
    pub const RANDOM: &str = "random";
    pub const PALETTE: &str = "palette";
    /// Shortcut constructors, each one colour.
    pub const SHORTCUTS: &[(&str, &str)] = &[
        ("White", "White"),
        ("Gray", "Medium stone grey"),
        ("DarkGray", "Dark stone grey"),
        ("Black", "Black"),
        ("Red", "Bright red"),
        ("Yellow", "Bright yellow"),
        ("Green", "Dark green"),
        ("Blue", "Bright blue"),
    ];
    /// The named colours: name, number and red, green, blue.
    pub const BRICK_COLORS: &[(&str, u16, [u8; 3])] = &[
        ("White", 1, [242, 243, 243]),
        ("Medium stone grey", 194, [163, 162, 165]),
        ("Dark stone grey", 199, [99, 95, 98]),
        ("Light stone grey", 208, [229, 228, 223]),
        ("Black", 26, [27, 42, 53]),
        ("Really black", 1003, [17, 17, 17]),
        ("Institutional white", 1001, [248, 248, 248]),
        ("Bright red", 21, [196, 40, 28]),
        ("Really red", 1004, [255, 0, 0]),
        ("Maroon", 1031, [117, 0, 0]),
        ("Persimmon", 1027, [255, 89, 89]),
        ("Bright blue", 23, [13, 105, 172]),
        ("Really blue", 1010, [0, 0, 255]),
        ("Deep blue", 1012, [33, 84, 185]),
        ("Navy blue", 1011, [0, 32, 96]),
        ("Medium blue", 102, [110, 153, 202]),
        ("Pastel blue", 1024, [128, 187, 219]),
        ("Cyan", 1019, [4, 175, 236]),
        ("Toothpaste", 1020, [0, 255, 255]),
        ("Teal", 1018, [18, 238, 212]),
        ("Bright bluish green", 107, [0, 143, 156]),
        ("Bright green", 37, [75, 151, 75]),
        ("Dark green", 28, [40, 127, 71]),
        ("Earth green", 141, [39, 70, 45]),
        ("Lime green", 1021, [0, 255, 0]),
        ("Camo", 1022, [58, 125, 21]),
        ("Grime", 1023, [127, 142, 100]),
        ("Br. yellowish green", 119, [164, 189, 71]),
        ("Bright yellow", 24, [245, 205, 48]),
        ("New Yeller", 1009, [255, 255, 0]),
        ("Cool yellow", 226, [253, 234, 141]),
        ("Gold", 1029, [239, 184, 56]),
        ("Bright orange", 106, [218, 133, 65]),
        ("Deep orange", 1005, [255, 176, 0]),
        ("Bright violet", 104, [107, 50, 124]),
        ("Magenta", 1015, [170, 0, 170]),
        ("Pink", 1016, [255, 102, 204]),
        ("Hot pink", 1032, [255, 0, 191]),
        ("Brown", 217, [124, 92, 70]),
        ("Reddish brown", 192, [105, 64, 40]),
        ("Nougat", 18, [204, 142, 105]),
        ("Sand red", 153, [149, 121, 119]),
        ("Fossil", 1030, [159, 161, 172]),
    ];
    /// The colour a BrickColor falls back to.
    pub const DEFAULT_BRICK_COLOR: &str = "Medium stone grey";

    /// Ray members.
    pub const ORIGIN: &str = "Origin";
    pub const DIRECTION: &str = "Direction";
    pub const UNIT: &str = "Unit";
    pub const CLOSEST_POINT: &str = "ClosestPoint";
    pub const DISTANCE: &str = "Distance";

    /// DateTime members.
    pub const NOW: &str = "now";
    pub const FROM_UNIX_TIMESTAMP: &str = "fromUnixTimestamp";
    pub const FROM_UNIX_TIMESTAMP_MILLIS: &str = "fromUnixTimestampMillis";
    pub const UNIX_TIMESTAMP: &str = "UnixTimestamp";
    pub const UNIX_TIMESTAMP_MILLIS: &str = "UnixTimestampMillis";
    pub const TO_ISO_DATE: &str = "ToIsoDate";
    pub const TO_UNIVERSAL_TIME: &str = "ToUniversalTime";
    pub const TO_LOCAL_TIME: &str = "ToLocalTime";
    pub const YEAR: &str = "Year";
    pub const MONTH: &str = "Month";
    pub const DAY: &str = "Day";
    pub const HOUR: &str = "Hour";
    pub const MINUTE: &str = "Minute";
    pub const SECOND: &str = "Second";
    pub const MILLISECOND: &str = "Millisecond";

    /// PhysicalProperties members.
    pub const DENSITY: &str = "Density";
    pub const FRICTION: &str = "Friction";
    pub const ELASTICITY: &str = "Elasticity";
    pub const FRICTION_WEIGHT: &str = "FrictionWeight";
    pub const ELASTICITY_WEIGHT: &str = "ElasticityWeight";

    /// Part members.
    pub const BRICK_COLOR_PROPERTY: &str = "BrickColor";
    pub const ASSEMBLY_MASS: &str = "AssemblyMass";
    pub const APPLY_ANGULAR_IMPULSE: &str = "ApplyAngularImpulse";
    pub const SET_NETWORK_OWNER: &str = "SetNetworkOwner";
    pub const GET_NETWORK_OWNER: &str = "GetNetworkOwner";
    pub const SET_NETWORK_OWNERSHIP_AUTO: &str = "SetNetworkOwnershipAuto";
    pub const CAN_SET_NETWORK_OWNERSHIP: &str = "CanSetNetworkOwnership";
    pub const GET_CONNECTED_PARTS: &str = "GetConnectedParts";
    pub const GET_JOINTS: &str = "GetJoints";

    /// Model members.
    pub const MOVE_TO: &str = "MoveTo";
    pub const GET_BOUNDING_BOX: &str = "GetBoundingBox";
    pub const GET_EXTENTS_SIZE: &str = "GetExtentsSize";
    pub const SET_PRIMARY_PART_CFRAME: &str = "SetPrimaryPartCFrame";
    pub const GET_PRIMARY_PART_CFRAME: &str = "GetPrimaryPartCFrame";
    pub const TRANSLATE_BY: &str = "TranslateBy";

    /// Workspace members.
    pub const FIND_PART_ON_RAY: &str = "FindPartOnRay";
    pub const FIND_PART_ON_RAY_WITH_IGNORE_LIST: &str = "FindPartOnRayWithIgnoreList";
    pub const FIND_PART_ON_RAY_WITH_WHITELIST: &str = "FindPartOnRayWithWhitelist";
    pub const GET_SERVER_TIME_NOW: &str = "GetServerTimeNow";
    pub const DISTRIBUTED_GAME_TIME: &str = "DistributedGameTime";

    /// Lighting members.
    pub const GET_MINUTES_AFTER_MIDNIGHT: &str = "GetMinutesAfterMidnight";
    pub const SET_MINUTES_AFTER_MIDNIGHT: &str = "SetMinutesAfterMidnight";

    /// The root's members.
    pub const BIND_TO_CLOSE: &str = "BindToClose";
    pub const IS_LOADED: &str = "IsLoaded";

    /// Humanoid members.
    pub const MOVE_DIRECTION: &str = "MoveDirection";
    pub const DISPLAY_NAME: &str = "DisplayName";

    /// Enumeration tables.
    pub const GET_ENUM_ITEMS: &str = "GetEnumItems";

    /// TweenService.
    pub const GET_VALUE: &str = "GetValue";

    /// The input object's state.
    pub const USER_INPUT_STATE: &str = "UserInputState";

    /// Services with nothing (or little) behind them yet, so scripts that
    /// use them still run.
    pub const MARKETPLACE_SERVICE: &str = "MarketplaceService";
    pub const BADGE_SERVICE: &str = "BadgeService";
    pub const TELEPORT_SERVICE: &str = "TeleportService";
    pub const SOUND_SERVICE: &str = "SoundService";
    pub const PHYSICS_SERVICE: &str = "PhysicsService";
    pub const GUI_SERVICE: &str = "GuiService";
    pub const TEXT_SERVICE: &str = "TextService";
    pub const CONTENT_PROVIDER: &str = "ContentProvider";
    pub const VR_SERVICE: &str = "VRService";
    pub const HAPTIC_SERVICE: &str = "HapticService";
    pub const POLICY_SERVICE: &str = "PolicyService";
    pub const MESSAGING_SERVICE: &str = "MessagingService";
    pub const CONTEXT_ACTION_SERVICE: &str = "ContextActionService";

    /// Their members.
    pub const PROMPT_PURCHASE: &str = "PromptPurchase";
    pub const PROMPT_GAME_PASS_PURCHASE: &str = "PromptGamePassPurchase";
    pub const PROMPT_PRODUCT_PURCHASE: &str = "PromptProductPurchase";
    pub const USER_OWNS_GAME_PASS_ASYNC: &str = "UserOwnsGamePassAsync";
    pub const PLAYER_OWNS_ASSET: &str = "PlayerOwnsAsset";
    pub const GET_PRODUCT_INFO: &str = "GetProductInfo";
    pub const AWARD_BADGE: &str = "AwardBadge";
    pub const USER_HAS_BADGE_ASYNC: &str = "UserHasBadgeAsync";
    pub const TELEPORT: &str = "Teleport";
    pub const TELEPORT_ASYNC: &str = "TeleportAsync";
    pub const TELEPORT_TO_PLACE_INSTANCE: &str = "TeleportToPlaceInstance";
    pub const PLAY_LOCAL_SOUND: &str = "PlayLocalSound";
    pub const REGISTER_COLLISION_GROUP: &str = "RegisterCollisionGroup";
    pub const COLLISION_GROUP_SET_COLLIDABLE: &str = "CollisionGroupSetCollidable";
    pub const COLLISION_GROUPS_ARE_COLLIDABLE: &str = "CollisionGroupsAreCollidable";
    pub const IS_COLLISION_GROUP_REGISTERED: &str = "IsCollisionGroupRegistered";
    pub const UNREGISTER_COLLISION_GROUP: &str = "UnregisterCollisionGroup";
    pub const GET_REGISTERED_COLLISION_GROUPS: &str = "GetRegisteredCollisionGroups";
    pub const CREATE_COLLISION_GROUP: &str = "CreateCollisionGroup";
    pub const SET_PART_COLLISION_GROUP: &str = "SetPartCollisionGroup";
    pub const MASK: &str = "mask";
    pub const IS_TEN_FOOT_INTERFACE: &str = "IsTenFootInterface";
    pub const GET_GUI_INSET: &str = "GetGuiInset";
    pub const GET_TEXT_SIZE: &str = "GetTextSize";
    pub const FILTER_STRING_ASYNC: &str = "FilterStringAsync";
    pub const GET_NON_CHAT_STRING_FOR_BROADCAST_ASYNC: &str = "GetNonChatStringForBroadcastAsync";
    pub const GET_NON_CHAT_STRING_FOR_USER_ASYNC: &str = "GetNonChatStringForUserAsync";
    pub const GET_CHAT_FOR_USER_ASYNC: &str = "GetChatForUserAsync";
    pub const PRELOAD_ASYNC: &str = "PreloadAsync";
    pub const VR_ENABLED: &str = "VREnabled";
    pub const IS_VIBRATION_SUPPORTED: &str = "IsVibrationSupported";
    pub const GET_POLICY_INFO_FOR_PLAYER_ASYNC: &str = "GetPolicyInfoForPlayerAsync";
    pub const ARE_ADS_ALLOWED: &str = "AreAdsAllowed";
    pub const PUBLISH_ASYNC: &str = "PublishAsync";
    pub const SUBSCRIBE_ASYNC: &str = "SubscribeAsync";
    pub const DATA: &str = "Data";
    pub const SENT: &str = "Sent";
    pub const DISCONNECT: &str = "Disconnect";
    pub const BIND_ACTION: &str = "BindAction";
    pub const BIND_ACTION_AT_PRIORITY: &str = "BindActionAtPriority";
    pub const UNBIND_ACTION: &str = "UnbindAction";
    pub const UNBIND_ALL_ACTIONS: &str = "UnbindAllActions";
    /// What scripts get back when they ask about something that isn't here.
    pub const NOT_AVAILABLE: &str = "isn't available in Libroxel yet";
}
