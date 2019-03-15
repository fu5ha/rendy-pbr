use winit::{
    DeviceEvent, ElementState, ModifiersState, MouseButton, MouseScrollDelta, VirtualKeyCode,
    WindowEvent,
};

use gfx_hal as hal;

use crate::node::pbr::Aux;

pub struct EventBucket(pub Vec<winit::Event>);

#[derive(Clone, Copy)]
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

fn try_add_instance_array_size_x(
    ia_size: (usize, usize, usize),
    max: usize,
) -> (usize, usize, usize) {
    let mut n_ia_size = ia_size;
    n_ia_size.0 += 1;
    if n_ia_size.0 * n_ia_size.1 * n_ia_size.2 <= max {
        n_ia_size
    } else {
        ia_size
    }
}

fn try_add_instance_array_size_y(
    ia_size: (usize, usize, usize),
    max: usize,
) -> (usize, usize, usize) {
    let mut n_ia_size = ia_size;
    n_ia_size.1 += 1;
    if n_ia_size.0 * n_ia_size.1 * n_ia_size.2 <= max {
        n_ia_size
    } else {
        ia_size
    }
}

fn try_add_instance_array_size_z(
    ia_size: (usize, usize, usize),
    max: usize,
) -> (usize, usize, usize) {
    let mut n_ia_size = ia_size;
    n_ia_size.2 += 1;
    if n_ia_size.0 * n_ia_size.1 * n_ia_size.2 <= max {
        n_ia_size
    } else {
        ia_size
    }
}

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
        winit::Event::WindowEvent { event, .. } => {
            use winit::{MouseButton, WindowEvent};
            match event {
                WindowEvent::CursorMoved {
                    position,
                    modifiers,
                    ..
                } => {
                    input.modifiers = modifiers;
                    input.mouse.pos = position;
                }
                WindowEvent::MouseInput {
                    state,
                    button,
                    modifiers,
                    ..
                } => {
                    input.modifiers = modifiers;
                    match button {
                        MouseButton::Left => {
                            input.mouse.left = state;
                        }
                        MouseButton::Right => {
                            input.mouse.right = state;
                        }
                        MouseButton::Middle => {
                            input.mouse.middle = state;
                        }
                        _ => (),
                    }
                }
                WindowEvent::KeyboardInput {
                    input: key_input, ..
                } => {
                    input.modifiers = key_input.modifiers;
                }
                _ => (),
            }
        }
    }
    pub fn handle_device_event<B: hal::Backend>(&mut self, event: &DeviceEvent, aux: &mut Aux<B>) {
    }
}
