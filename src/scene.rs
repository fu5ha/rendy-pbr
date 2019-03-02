use rendy::mesh::Mesh;

use gfx_hal as hal;

pub const MAX_LIGHTS: usize = 32;

#[derive(Clone, Copy)]
pub struct Light {
    pub pos: nalgebra::Vector3<f32>,
    pub intensity: f32,
    pub color: [f32; 3],
    pub _pad: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct Camera {
    pub yaw: f32,
    pub pitch: f32,
    pub dist: f32,
    pub focus: nalgebra::Point3<f32>,
    pub view: nalgebra::Isometry3<f32>,
    pub proj: nalgebra::Perspective3<f32>,
}

impl Camera {
    pub fn update_view(&mut self) {
        self.view = nalgebra::Isometry3::look_at_rh(
            &(self.focus + (self.dist * nalgebra::Vector3::new(
                self.yaw.sin() * self.pitch.cos(),
                self.pitch.sin(),
                self.yaw.cos() * self.pitch.cos()
            ))),
            &self.focus,
            &nalgebra::Vector3::y(),
        );
    }
}

pub struct Object<B: hal::Backend> {
    pub mesh: Mesh<B>,
    pub material: u64,
}

// pub struct Environment<B: hal::Backend> {
//     mesh: Mesh<B>,
//     hdr: Texture<B>,
//     irradiance: Texture<B>,
//     spec_filtered: Texture<B>,
//     bdrf: Texture<B>,
// }

pub struct Scene<B: hal::Backend> {
    pub camera: Camera,
    pub objects: Vec<(Object<B>, Vec<nalgebra::Matrix4<f32>>)>,
    pub max_obj_instances: Vec<usize>,
    pub lights: Vec<Light>,
    // environment: Environment<B>,
}