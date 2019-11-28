use derivative::Derivative;
use rendy::init::winit::{
    self,
    event::{ElementState, Event, ModifiersState, MouseButton, WindowEvent},
};

#[derive(Default)]
pub struct EventBucket(pub Vec<Event<()>>);

#[derive(Derivative, Debug, Clone, Copy)]
#[derivative(Default)]
pub struct MouseState {
    #[derivative(Default(value = "ElementState::Released"))]
    pub left: ElementState,
    #[derivative(Default(value = "ElementState::Released"))]
    pub right: ElementState,
    #[derivative(Default(value = "ElementState::Released"))]
    pub middle: ElementState,
    #[derivative(Default(value = "winit::dpi::LogicalPosition::new(0., 0.)"))]
    pub pos: winit::dpi::LogicalPosition,
}

pub const ROTATE_SENSITIVITY: f32 = 0.005;
pub const TRANSLATE_SENSITIVITY: f32 = 0.005;
pub const ZOOM_MOUSE_SENSITIVITY: f32 = 0.0125;
pub const ZOOM_SCROLL_SENSITIVITY: f32 = 0.25;
pub const EXPOSURE_ADJUST_SENSITIVITY: f32 = 0.1;
pub const CUBE_ROUGHNESS_SENSITIVITY: f32 = 0.1;

#[derive(Derivative, Debug, Clone, Copy)]
#[derivative(Default)]
pub struct InputState {
    pub mouse: MouseState,
    pub modifiers: ModifiersState,
    #[derivative(Default(value = "winit::dpi::LogicalSize::new(0., 0.)"))]
    pub window_size: winit::dpi::LogicalSize,
}

impl InputState {
    pub fn new(window_size: winit::dpi::LogicalSize) -> Self {
        InputState {
            mouse: MouseState {
                left: ElementState::Released,
                right: ElementState::Released,
                middle: ElementState::Released,
                pos: winit::dpi::LogicalPosition::new(0.0, 0.0),
            },
            modifiers: Default::default(),
            window_size,
        }
    }

    pub fn calc_comparison_factor(&self) -> f32 {
        self.mouse.pos.x as f32 / self.window_size.width as f32
    }

    pub fn update_with_window_event(&mut self, event: &WindowEvent) {
        match *event {
            WindowEvent::CursorMoved {
                position,
                modifiers,
                ..
            } => {
                self.modifiers = modifiers;
                self.mouse.pos = position;
            }
            WindowEvent::MouseInput {
                state,
                button,
                modifiers,
                ..
            } => {
                self.modifiers = modifiers;
                match button {
                    MouseButton::Left => {
                        self.mouse.left = state;
                    }
                    MouseButton::Right => {
                        self.mouse.right = state;
                    }
                    MouseButton::Middle => {
                        self.mouse.middle = state;
                    }
                    _ => (),
                }
            }
            WindowEvent::KeyboardInput {
                input: key_input, ..
            } => {
                self.modifiers = key_input.modifiers;
            }
            _ => (),
        }
    }
}
