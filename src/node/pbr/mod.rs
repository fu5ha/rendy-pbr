use gfx_hal as hal;

use std::collections::HashMap;

use crate::{scene, asset};

pub mod mesh;
pub mod tonemap;

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct CameraArgs {
    pub proj: nalgebra::Matrix4<f32>,
    pub view: nalgebra::Matrix4<f32>,
    pub camera_pos: nalgebra::Point3<f32>,
}

impl From<scene::Camera> for CameraArgs {
    fn from(cam: scene::Camera) -> Self {
        CameraArgs {
            proj: {
                let mut proj = cam.proj.to_homogeneous();
                proj[(1, 1)] *= -1.0;
                proj
            },
            view: cam.view.to_homogeneous(),
            camera_pos: nalgebra::Point3::from(cam.view.rotation.inverse() * (cam.view.translation.vector * -1.0)),
        }   
    }
}

pub struct Aux<B: hal::Backend> {
    pub frames: usize,
    pub align: u64,
    pub instance_array_size: (usize, usize, usize),
    pub scene: scene::Scene<B>,
    pub material_storage: HashMap<u64, asset::Material<B>>,
    pub tonemapper_args: tonemap::TonemapperArgs,
}