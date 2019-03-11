use winit::{WindowEvent, DeviceEvent, MouseButton, ElementState, MouseScrollDelta, ModifiersState, VirtualKeyCode};

use gfx_hal as hal;

use crate::node::pbr::Aux;

#[derive(Clone, Copy)]
pub struct MouseState {
    left: ElementState,
    right: ElementState,
    middle: ElementState,
    pos: winit::dpi::LogicalPosition,
}

pub const ROTATE_SENSITIVITY: f32 = 0.005;
pub const TRANSLATE_SENSITIVITY: f32 = 0.01;
pub const ZOOM_MOUSE_SENSITIVITY: f32 = 0.025;
pub const ZOOM_SCROLL_SENSITIVITY: f32 = 1.0;
pub const EXPOSURE_ADJUST_SENSITIVITY: f32 = 0.1;

    
fn try_add_instance_array_size_x(
    ia_size: (usize, usize, usize),
    max: usize
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
    max: usize
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
    max: usize
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
    mouse: MouseState,
    modifiers: ModifiersState,
    window_size: winit::dpi::LogicalSize,
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

    fn calc_comparison_factor(&self) -> f32 {
        self.mouse.pos.x as f32 / self.window_size.width as f32
    }

    pub fn handle_window_event<B: hal::Backend>(&mut self, event: &WindowEvent, aux: &mut Aux<B>) {
        match event {
            WindowEvent::CursorMoved { position, modifiers, .. } => {
                self.modifiers = *modifiers;
                self.mouse.pos = *position;
                if let (
                    MouseState { left: ElementState::Pressed, .. },
                    ModifiersState { ctrl: true, .. }
                ) = (self.mouse, self.modifiers) {
                    aux.tonemapper_args.comparison_factor = self.calc_comparison_factor();
                }
            },
            WindowEvent::MouseInput {state, button, modifiers, .. } => {
                self.modifiers = *modifiers;
                match button {
                    MouseButton::Left => {
                        self.mouse.left = *state;
                        if modifiers.ctrl {
                            aux.tonemapper_args.comparison_factor = self.calc_comparison_factor();
                        }
                    },
                    MouseButton::Right => {
                        self.mouse.right = *state;
                    },
                    MouseButton::Middle => {
                        self.mouse.middle = *state;
                    },
                    _ => ()
                }
            },
            WindowEvent::KeyboardInput { input, .. } => {
                self.modifiers = input.modifiers;
                if let Some(kc) = input.virtual_keycode {
                    match (kc, input.state, self.modifiers) {
                        // Array size controls
                        (VirtualKeyCode::X, ElementState::Pressed, ModifiersState { shift: false, .. }) => {
                            aux.instance_array_size = try_add_instance_array_size_x(aux.instance_array_size, aux.scene.max_obj_instances[0]);
                        },
                        (VirtualKeyCode::X, ElementState::Pressed, ModifiersState { shift: true, .. }) => {
                            aux.instance_array_size.0 = (aux.instance_array_size.0 - 1).max(1);
                        },
                        (VirtualKeyCode::Y, ElementState::Pressed, ModifiersState { shift: false, .. }) => {
                            aux.instance_array_size = try_add_instance_array_size_y(aux.instance_array_size, aux.scene.max_obj_instances[0]);
                        },
                        (VirtualKeyCode::Y, ElementState::Pressed, ModifiersState { shift: true, .. }) => {
                            aux.instance_array_size.1 = (aux.instance_array_size.1 - 1).max(1);
                        },
                        (VirtualKeyCode::Z, ElementState::Pressed, ModifiersState { shift: false, .. }) => {
                            aux.instance_array_size = try_add_instance_array_size_z(aux.instance_array_size, aux.scene.max_obj_instances[0]);
                        },
                        (VirtualKeyCode::Z, ElementState::Pressed, ModifiersState { shift: true, .. }) => {
                            aux.instance_array_size.2 = (aux.instance_array_size.2 - 1).max(1);
                        },
                        // Tonemapper controls
                        (VirtualKeyCode::E, ElementState::Pressed, ModifiersState { shift: false, .. }) => {
                            aux.tonemapper_args.exposure += EXPOSURE_ADJUST_SENSITIVITY;
                        },
                        (VirtualKeyCode::E, ElementState::Pressed, ModifiersState { shift: true, .. }) => {
                            aux.tonemapper_args.exposure -= EXPOSURE_ADJUST_SENSITIVITY;
                        },
                        (VirtualKeyCode::A, ElementState::Pressed, ModifiersState { .. }) => aux.tonemapper_args.curve = 0,
                        (VirtualKeyCode::U, ElementState::Pressed, ModifiersState { .. }) => aux.tonemapper_args.curve = 1,
                        (VirtualKeyCode::C, ElementState::Pressed, ModifiersState { .. }) => aux.tonemapper_args.curve = 2,
                        _ => (),
                    }
                }
            },
            _ => (),
        }
    }
    pub fn handle_device_event<B: hal::Backend>(&mut self, event: &DeviceEvent, aux: &mut Aux<B>) {
        match event {
            DeviceEvent::MouseMotion {delta} => {
                match (self.mouse, self.modifiers) {
                    (MouseState { left: ElementState::Pressed, .. }, ModifiersState { ctrl: false, .. }) => {
                        aux.scene.camera.yaw += -delta.0 as f32 * ROTATE_SENSITIVITY;
                        aux.scene.camera.pitch += delta.1 as f32 * ROTATE_SENSITIVITY;
                        aux.scene.camera.pitch = aux.scene.camera.pitch
                            .max(-std::f32::consts::FRAC_PI_2 + 0.0001)
                            .min(std::f32::consts::FRAC_PI_2 - 0.0001);
                        aux.scene.camera.update_view();
                    },
                    (MouseState { middle: ElementState::Pressed, .. }, ModifiersState { ctrl: false, .. }) => {
                        let m_vec = nalgebra::Vector3::new(-delta.0 as f32, delta.1 as f32, 0.0) * TRANSLATE_SENSITIVITY;
                        let rot = aux.scene.camera.view.rotation.inverse();
                        let m_vec = rot * m_vec;
                        aux.scene.camera.focus = aux.scene.camera.focus + m_vec;
                        aux.scene.camera.update_view();
                    },
                    (MouseState { right: ElementState::Pressed, .. }, ModifiersState { ctrl: false, .. }) => {
                        let amount = -delta.0 as f32 * ZOOM_MOUSE_SENSITIVITY;
                        aux.scene.camera.dist += amount;
                        aux.scene.camera.dist = aux.scene.camera.dist.max(0.0);
                        aux.scene.camera.update_view();
                    },
                    _ => (),
                }
            },
            DeviceEvent::MouseWheel {delta} => {
                let amount = match delta {
                    MouseScrollDelta::LineDelta(_, y) => {
                        -y as f32 * ZOOM_SCROLL_SENSITIVITY
                    },
                    MouseScrollDelta::PixelDelta(delta) => {
                        -delta.y as f32 * ZOOM_SCROLL_SENSITIVITY * 0.05
                    },
                };
                aux.scene.camera.dist += amount;
                aux.scene.camera.dist = aux.scene.camera.dist.max(0.0);
                aux.scene.camera.update_view();
            }
            _ => (),
        }
    }
}