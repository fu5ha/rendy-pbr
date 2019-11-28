use rendy::{
    command::{QueueId, RenderPassEncoder},
    factory::Factory,
    graph::{render::*, GraphContext, NodeBuffer, NodeImage},
    hal::{device::Device, pso::DescriptorPool},
    resource::{DescriptorSetLayout, Handle},
    shader::{PathBufShaderInfo, ShaderKind, SourceLanguage},
};

use rendy::hal;

use crate::node::env_preprocess::Aux;

use std::borrow::Cow;

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

    static ref SHADERS: rendy::shader::ShaderSetBuilder = rendy::shader::ShaderSetBuilder::default()
        .with_vertex(&*VERTEX).unwrap()
        .with_fragment(&*FRAGMENT).unwrap();
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
        vec![hal::pso::ColorBlendDesc {
            mask: hal::pso::ColorMask::ALL,
            blend: None,
        }]
    }

    fn depth_stencil(&self) -> Option<hal::pso::DepthStencilDesc> {
        None
    }

    fn load_shader_set(
        &self,
        factory: &mut Factory<B>,
        aux: &Aux<B>,
    ) -> rendy::shader::ShaderSet<B> {
        let mut spec_constants = rendy::shader::SpecConstantSet::default();
        spec_constants.fragment = Some(hal::pso::Specialization {
            constants: Cow::from(vec![hal::pso::SpecializationConstant {
                id: 0,
                range: 0..4,
            }]),
            data: Cow::from(
                &unsafe { std::mem::transmute::<&u32, &[u8; 4]>(&aux.irradiance_theta_samples) }
                    [0..4],
            ),
        });
        SHADERS.build(factory, spec_constants).unwrap()
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
    ) -> Result<Pipeline<B>, hal::pso::CreationError> {
        assert!(buffers.is_empty());
        assert!(images.is_empty());
        assert!(set_layouts.len() == 1);

        let mut pool = unsafe {
            factory
                .create_descriptor_pool(
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
                    hal::pso::DescriptorPoolCreateFlags::empty(),
                )
                .unwrap()
        };

        let set = unsafe {
            let set = pool.allocate_set(&set_layouts[0].raw()).unwrap();
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
        unsafe {
            encoder.bind_graphics_descriptor_sets(layout, 0, Some(&self.set), std::iter::empty());
            encoder.draw(0..6, 0..6);
        }
    }

    fn dispose(mut self, factory: &mut Factory<B>, _aux: &Aux<B>) {
        unsafe {
            self.pool.reset();
            factory.destroy_descriptor_pool(self.pool);
        }
    }
}
