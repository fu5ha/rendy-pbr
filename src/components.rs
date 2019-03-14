use specs::prelude::*;

pub struct Transform(pub nalgebra::Similarity3<f32>);

impl Component for Transform {
    type Storage = FlaggedStorage<Self, VecStorage<Self>>;
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
            &(self.focus
                + (self.dist
                    * nalgebra::Vector3::new(
                        self.yaw.sin() * self.pitch.cos(),
                        self.pitch.sin(),
                        self.yaw.cos() * self.pitch.cos(),
                    ))),
            &self.focus,
            &nalgebra::Vector3::y(),
        );
    }
}

impl Component for Camera {
    type Storage = HashMapStorage<Self>;
}

#[derive(Clone, Copy)]
pub struct Light {
    pub pos: nalgebra::Vector3<f32>,
    pub intensity: f32,
    pub color: [f32; 3],
    pub _pad: f32,
}

impl Component for Light {
    type Storage = DenseVecStorage<Self>;
}

pub struct Material {
    pub mat: Entity,
}

impl Component for Material {
    type Storage = DenseVecStorage<Self>;
}

pub struct Mesh {
    pub mesh: Entity,
}

impl Component for Mesh {
    type Storage = DenseVecStorage<Self>;
}

// pub struct Environment<B: hal::Backend> {
//     mesh: Mesh<B>,
//     hdr: Texture<B>,
//     irradiance: Texture<B>,
//     spec_filtered: Texture<B>,
//     bdrf: Texture<B>,
// }
