use crate::asset;

use serde::Deserialize;
use specs::prelude::*;

pub use crate::transform::components::*;

#[derive(Debug, Clone, Copy)]
pub struct Camera {
    pub yaw: f32,
    pub pitch: f32,
    pub dist: f32,
    pub focus: nalgebra::Point3<f32>,
    pub proj: nalgebra::Perspective3<f32>,
}

impl Component for Camera {
    type Storage = FlaggedStorage<Self, HashMapStorage<Self>>;
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Light {
    pub intensity: f32,
    pub color: [f32; 3],
}

impl Component for Light {
    type Storage = FlaggedStorage<Self, DenseVecStorage<Self>>;
}

pub struct Mesh(pub asset::MeshHandle);

impl Component for Mesh {
    type Storage = FlaggedStorage<Self, DenseVecStorage<Self>>;
}

/// Indicates that an entity is the active camera.
#[derive(Debug, Default)]
pub struct ActiveCamera;

impl Component for ActiveCamera {
    type Storage = NullStorage<Self>;
}

// pub struct Environment<B: hal::Backend> {
//     mesh: Mesh<B>,
//     hdr: Texture<B>,
//     irradiance: Texture<B>,
//     spec_filtered: Texture<B>,
//     bdrf: Texture<B>,
// }
