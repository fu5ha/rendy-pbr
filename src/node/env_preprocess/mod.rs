use rendy::{command::QueueId, factory::ImageState, texture::Texture};

use gfx_hal as hal;

pub mod equirectangular_to_cube_faces;
pub mod faces_to_cubemap;

pub struct Aux<B: hal::Backend> {
    pub align: u64,
    pub equirectangular_texture: Texture<B>,
    pub environment_cubemap: Texture<B>,
}

impl<B> faces_to_cubemap::FacesToCubemapResource<B> for Aux<B>
where
    B: hal::Backend,
{
    fn get_cubemap(&self) -> &Texture<B> {
        &self.environment_cubemap
    }

    fn cubemap_end_state(&self) -> ImageState {
        ImageState {
            queue: QueueId(hal::queue::QueueFamilyId(0), 0), // TODO
            stage: hal::pso::PipelineStage::FRAGMENT_SHADER,
            access: hal::image::Access::SHADER_READ,
            layout: hal::image::Layout::ShaderReadOnlyOptimal,
        }
    }
}
