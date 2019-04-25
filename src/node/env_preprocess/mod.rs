use rendy::{command::QueueId, factory::ImageState, texture::Texture};

use rendy::hal;

pub mod copy_to_texture;
pub mod debug;
pub mod env_to_irradiance;
pub mod env_to_specular;
pub mod equirectangular_to_cube_faces;
pub mod faces_to_cubemap;
pub mod integrate_spec_brdf;

pub struct Aux<B: hal::Backend> {
    pub align: u64,
    pub irradiance_theta_samples: u32,
    pub spec_samples: u32,
    pub equirectangular_texture: Texture<B>,
    pub environment_cubemap: Option<Texture<B>>,
    pub irradiance_cubemap: Option<Texture<B>>,
    pub spec_cubemap: Option<Texture<B>>,
    pub spec_brdf_map: Option<Texture<B>>,
    pub queue: QueueId,
    pub mip_level: std::sync::atomic::AtomicUsize,
}

impl<B> faces_to_cubemap::FacesToCubemapResource<B> for Aux<B>
where
    B: hal::Backend,
{
    fn get_cubemap(&self, name: &str) -> &Texture<B> {
        match name {
            "environment" => self.environment_cubemap.as_ref().unwrap(),
            "irradiance" => self.irradiance_cubemap.as_ref().unwrap(),
            "specular" => self.spec_cubemap.as_ref().unwrap(),
            _ => unreachable!(),
        }
    }

    fn cubemap_end_state(&self, _name: &str) -> ImageState {
        ImageState {
            queue: self.queue,
            stage: hal::pso::PipelineStage::FRAGMENT_SHADER,
            access: hal::image::Access::SHADER_READ,
            layout: hal::image::Layout::ShaderReadOnlyOptimal,
        }
    }
}

impl<B> copy_to_texture::CopyToTextureResource<B> for Aux<B>
where
    B: hal::Backend,
{
    fn get_texture(&self, name: &str) -> &Texture<B> {
        match name {
            "spec_brdf" => self.spec_brdf_map.as_ref().unwrap(),
            _ => unreachable!(),
        }
    }

    fn texture_end_state(&self, _name: &str) -> ImageState {
        ImageState {
            queue: self.queue,
            stage: hal::pso::PipelineStage::FRAGMENT_SHADER,
            access: hal::image::Access::SHADER_READ,
            layout: hal::image::Layout::ShaderReadOnlyOptimal,
        }
    }
}
