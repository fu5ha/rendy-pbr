use rendy::{
    command::{QueueId, RenderPassEncoder},
    factory::Factory,
    graph::{render::*, GraphContext, NodeBuffer, NodeImage},
    hal::{pso::DescriptorPool, Device},
    resource::{DescriptorSetLayout, Handle},
    shader::{PathBufShaderInfo, Shader, ShaderKind, SourceLanguage},
};

use rendy::hal;

use crate::node::env_preprocess::Aux;

lazy_static::lazy_static! {
    static ref VERTEX: PathBufShaderInfo = PathBufShaderInfo::new(
        std::path::PathBuf::from(crate::application_root_dir()).join("assets/shaders/unproject_cubemap_tex.vert"),
        ShaderKind::Vertex,
        SourceLanguage::GLSL,
        "main",
    );

    static ref FRAGMENT: PathBufShaderInfo = PathBufShaderInfo::new(
        std::path::PathBuf::from(crate::application_root_dir()).join("assets/shaders/env_to_irradiance.frag"),
        ShaderKind::Fragment,
        SourceLanguage::GLSL,
        "main",
    );
}

#[derive(Debug, Default)]
pub struct PipelineDesc;

pub struct Pipeline<B: hal::Backend> {
    set: B::DescriptorSet,
    pool: B::DescriptorPool,
}

impl<B: hal::Backend> std::fmt::Debug for Pipeline<B> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Equirect Pipeline")
    }
}

impl<B> SimpleGraphicsPipelineDesc<B, Aux<B>> for PipelineDesc
where
    B: hal::Backend,
{
    type Pipeline = Pipeline<B>;

    fn colors(&self) -> Vec<hal::pso::ColorBlendDesc> {
        vec![hal::pso::ColorBlendDesc(hal::pso::ColorMask::ALL, hal::pso::BlendState::Off,); 1]
    }

    fn depth_stencil(&self) -> Option<hal::pso::DepthStencilDesc> {
        None
    }

    fn load_shader_set<'a>(
        &self,
        storage: &'a mut Vec<B::ShaderModule>,
        factory: &mut Factory<B>,
        aux: &Aux<B>,
    ) -> hal::pso::GraphicsShaderSet<'a, B> {
        storage.clear();

        log::trace!("Load shader module '{:#?}'", *VERTEX);
        storage.push(unsafe { VERTEX.module(factory).unwrap() });

        log::trace!("Load shader module '{:#?}'", *FRAGMENT);
        storage.push(unsafe { FRAGMENT.module(factory).unwrap() });

        hal::pso::GraphicsShaderSet {
            vertex: hal::pso::EntryPoint {
                entry: "main",
                module: &storage[0],
                specialization: hal::pso::Specialization::default(),
            },
            fragment: Some(hal::pso::EntryPoint {
                entry: "main",
                module: &storage[1],
                specialization: hal::pso::Specialization {
                    constants: &[hal::pso::SpecializationConstant { id: 0, range: 0..4 }],
                    data: unsafe {
                        std::mem::transmute::<&u32, &[u8; 4]>(&aux.irradiance_theta_samples)
                    },
                },
            }),
            hull: None,
            domain: None,
            geometry: None,
        }
    }

    fn layout(&self) -> Layout {
        Layout {
            sets: vec![SetLayout {
                bindings: vec![
                    hal::pso::DescriptorSetLayoutBinding {
                        binding: 0,
                        ty: hal::pso::DescriptorType::Sampler,
                        count: 1,
                        stage_flags: hal::pso::ShaderStageFlags::FRAGMENT,
                        immutable_samplers: false,
                    },
                    hal::pso::DescriptorSetLayoutBinding {
                        binding: 1,
                        ty: hal::pso::DescriptorType::SampledImage,
                        count: 1,
                        stage_flags: hal::pso::ShaderStageFlags::FRAGMENT,
                        immutable_samplers: false,
                    },
                ],
            }],
            push_constants: Vec::new(),
        }
    }

    fn build<'a>(
        self,
        _ctx: &GraphContext<B>,
        factory: &mut Factory<B>,
        _queue: QueueId,
        aux: &Aux<B>,
        buffers: Vec<NodeBuffer>,
        images: Vec<NodeImage>,
        set_layouts: &[Handle<DescriptorSetLayout<B>>],
    ) -> Result<Pipeline<B>, failure::Error> {
        assert!(buffers.is_empty());
        assert!(images.is_empty());
        assert!(set_layouts.len() == 1);

        let mut pool = unsafe {
            factory.create_descriptor_pool(
                1,
                vec![
                    hal::pso::DescriptorRangeDesc {
                        ty: hal::pso::DescriptorType::Sampler,
                        count: 1,
                    },
                    hal::pso::DescriptorRangeDesc {
                        ty: hal::pso::DescriptorType::SampledImage,
                        count: 1,
                    },
                ],
            )?
        };

        let set = unsafe {
            let set = pool.allocate_set(&set_layouts[0].raw())?;
            factory.write_descriptor_sets(vec![
                hal::pso::DescriptorSetWrite {
                    set: &set,
                    binding: 0,
                    array_offset: 0,
                    descriptors: Some(hal::pso::Descriptor::Sampler(
                        aux.environment_cubemap.as_ref().unwrap().sampler().raw(),
                    )),
                },
                hal::pso::DescriptorSetWrite {
                    set: &set,
                    binding: 1,
                    array_offset: 0,
                    descriptors: Some(hal::pso::Descriptor::Image(
                        aux.environment_cubemap.as_ref().unwrap().view().raw(),
                        hal::image::Layout::ShaderReadOnlyOptimal,
                    )),
                },
            ]);
            set
        };

        Ok(Pipeline { set, pool })
    }
}

impl<B> SimpleGraphicsPipeline<B, Aux<B>> for Pipeline<B>
where
    B: hal::Backend,
{
    type Desc = PipelineDesc;

    fn prepare(
        &mut self,
        _factory: &Factory<B>,
        _queue: QueueId,
        _set_layouts: &[Handle<DescriptorSetLayout<B>>],
        _index: usize,
        _aux: &Aux<B>,
    ) -> PrepareResult {
        PrepareResult::DrawReuse
    }

    fn draw(
        &mut self,
        layout: &B::PipelineLayout,
        mut encoder: RenderPassEncoder<'_, B>,
        _index: usize,
        _aux: &Aux<B>,
    ) {
        encoder.bind_graphics_descriptor_sets(layout, 0, Some(&self.set), std::iter::empty());
        encoder.draw(0..6, 0..6);
    }

    fn dispose(mut self, factory: &mut Factory<B>, _aux: &Aux<B>) {
        unsafe {
            self.pool.reset();
            factory.destroy_descriptor_pool(self.pool);
        }
    }
}
