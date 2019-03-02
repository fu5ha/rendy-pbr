use winit::{WindowEvent, DeviceEvent, MouseButton, ElementState, MouseScrollDelta};

use gfx_hal as hal;

pub struct MouseState {
    left: ElementState,
    right: ElementState,
    middle: ElementState,
}

pub const ROTATE_SENSITIVITY: f32 = 0.005;
pub const TRANSLATE_SENSITIVITY: f32 = 0.01;
pub const ZOOM_MOUSE_SENSITIVITY: f32 = 0.01;
pub const ZOOM_SCROLL_SENSITIVITY: f32 = 0.5;

pub struct InputState {
    focused: bool,
    mouse: MouseState,
}

impl InputState {
    pub fn new() -> Self {
        InputState {
            focused: true,
            mouse: MouseState {
                left: ElementState::Released,
                right: ElementState::Released,
                middle: ElementState::Released,
            }
        }
    }

    pub fn handle_window_event<B: hal::Backend>(&mut self, event: &WindowEvent, _aux: &mut crate::Aux<B>) {
        match event {
            WindowEvent::MouseInput {state, button, ..} => {
                match button {
                    MouseButton::Left => {
                        self.mouse.left = *state;
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
            _ => (),
        }
    }
    pub fn handle_device_event<B: hal::Backend>(&mut self, event: &DeviceEvent, aux: &mut crate::Aux<B>) {
        match event {
            DeviceEvent::MouseMotion {delta} => {
                match self.mouse {
                    MouseState { left: ElementState::Pressed, .. } => {
                        aux.scene.camera.yaw += -delta.0 as f32 * ROTATE_SENSITIVITY;
                        aux.scene.camera.pitch += delta.1 as f32 * ROTATE_SENSITIVITY;
                        aux.scene.camera.pitch = aux.scene.camera.pitch
                            .max(-std::f32::consts::FRAC_PI_2 + 0.0001)
                            .min(std::f32::consts::FRAC_PI_2 - 0.0001);
                        aux.scene.camera.update_view();
                    },
                    MouseState { middle: ElementState::Pressed, .. } => {
                        let m_vec = nalgebra::Vector3::new(-delta.0 as f32, delta.1 as f32, 0.0) * TRANSLATE_SENSITIVITY;
                        let rot = aux.scene.camera.view.rotation.inverse();
                        let m_vec = rot * m_vec;
                        aux.scene.camera.focus = aux.scene.camera.focus + m_vec;
                        aux.scene.camera.update_view();
                    },
                    MouseState { right: ElementState::Pressed, .. } => {
                        let amount = delta.0 as f32 * ZOOM_MOUSE_SENSITIVITY;
                        aux.scene.camera.dist += amount;
                        aux.scene.camera.dist = aux.scene.camera.dist.max(0.0);
                        aux.scene.camera.update_view();
                    },
                    _ => (),
                }
            },
            DeviceEvent::MouseWheel {delta} => {
                if self.focused {
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
            }
            _ => (),
        }
    }
}