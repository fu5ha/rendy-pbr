use crate::components;
use derivative::Derivative;

pub mod mesh;
pub mod tonemap;

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct CameraArgs {
    pub proj: nalgebra::Matrix4<f32>,
    pub view: nalgebra::Matrix4<f32>,
    pub camera_pos: nalgebra::Point3<f32>,
}

impl From<(&components::Camera, &components::Transform)> for CameraArgs {
    fn from((cam, trans): (&components::Camera, &components::Transform)) -> Self {
        CameraArgs {
            proj: {
                let mut proj = cam.proj.to_homogeneous();
                proj[(1, 1)] *= -1.0;
                proj
            },
            view: trans.0.inverse().to_homogeneous(),
            camera_pos: nalgebra::Point3::from(trans.0.isometry.translation.vector),
        }
    }
}

#[derive(Derivative, Clone, Copy)]
#[derivative(Default)]
#[repr(C)]
pub struct LightData {
    #[derivative(Default(value = "nalgebra::Point3::<f32>::origin()"))]
    pub pos: nalgebra::Point3<f32>,
    pub intensity: f32,
    pub color: [f32; 3],
    pub _pad: f32,
}

#[derive(Default)]
pub struct Aux {
    pub frames: usize,
    pub align: u64,
    pub instance_array_size: (u8, u8, u8),
    pub tonemapper_args: tonemap::TonemapperArgs,
}
