use crate::asset;

use specs::prelude::*;

pub struct Transform(pub nalgebra::Similarity3<f32>);

impl Default for Transform {
    fn default() -> Self {
        Transform(nalgebra::Similarity3::identity())
    }
}

impl Component for Transform {
    type Storage = FlaggedStorage<Self, VecStorage<Self>>;
}

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

#[derive(Clone, Copy)]
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

pub type InstanceIndex = u16;
/// The global number instance that this entity is of its attached mesh.
/// This should only be added and changed automatically by the `InstanceCacheUpdateSystem`.
pub struct MeshInstance {
    pub mesh: asset::MeshHandle,
    pub intance: InstanceIndex,
}

impl Component for MeshInstance {
    type Storage = DenseVecStorage<Self>;
}

// pub struct Environment<B: hal::Backend> {
//     mesh: Mesh<B>,
//     hdr: Texture<B>,
//     irradiance: Texture<B>,
//     spec_filtered: Texture<B>,
//     bdrf: Texture<B>,
// }
