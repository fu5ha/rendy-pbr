use winit::{
    DeviceEvent, ElementState, ModifiersState, MouseButton, MouseScrollDelta, VirtualKeyCode,
    WindowEvent,
};

use gfx_hal as hal;

use crate::node::pbr::Aux;

pub struct EventBucket(pub Vec<winit::Event>);

#[derive(Debug, Clone, Copy)]
pub struct MouseState {
    pub left: ElementState,
    pub right: ElementState,
    pub middle: ElementState,
    pub pos: winit::dpi::LogicalPosition,
}

pub const ROTATE_SENSITIVITY: f32 = 0.005;
pub const TRANSLATE_SENSITIVITY: f32 = 0.01;
pub const ZOOM_MOUSE_SENSITIVITY: f32 = 0.025;
pub const ZOOM_SCROLL_SENSITIVITY: f32 = 1.0;
pub const EXPOSURE_ADJUST_SENSITIVITY: f32 = 0.1;

#[derive(Debug, Clone, Copy)]
pub struct InputState {
    pub mouse: MouseState,
    pub modifiers: ModifiersState,
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
        use winit::{MouseButton, WindowEvent};
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
