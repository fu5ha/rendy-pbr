use rendy::{
    command::{QueueId, RenderPassEncoder},
    factory::Factory,
    graph::{render::*, GraphContext, ImageAccess, NodeBuffer, NodeImage},
    hal::{device::Device, pso::DescriptorPool},
    resource::{
        Buffer, BufferInfo, DescriptorSetLayout, Escape, Filter, Handle, ImageView, ImageViewInfo,
        Sampler, SamplerDesc, ViewKind, WrapMode,
    },
    shader::{PathBufShaderInfo, ShaderKind, SourceLanguage},
};

use rendy::hal;

use std::mem::size_of;

use crate::node::pbr::Aux;

lazy_static::lazy_static! {
    static ref VERTEX: PathBufShaderInfo = PathBufShaderInfo::new(
        std::path::PathBuf::from(crate::application_root_dir()).join("assets/shaders/fullscreen_triangle.vert"),
        ShaderKind::Vertex,
        SourceLanguage::GLSL,
        "main",
    );

    static ref FRAGMENT: PathBufShaderInfo = PathBufShaderInfo::new(
        std::path::PathBuf::from(crate::application_root_dir()).join("assets/shaders/tonemap.frag"),
        ShaderKind::Fragment,
        SourceLanguage::GLSL,
        "main",
    );

    static ref SHADERS: rendy::shader::ShaderSetBuilder = rendy::shader::ShaderSetBuilder::default()
        .with_vertex(&*VERTEX).unwrap()
        .with_fragment(&*FRAGMENT).unwrap();
}

#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct TonemapperArgs {
    pub exposure: f32,
    pub curve: i32,
    pub comparison_factor: f32,
}

impl std::fmt::Display for TonemapperArgs {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Exposure: {}, Curve: {}", self.exposure, self.curve)
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct UniformArgs {
    tonemapper: TonemapperArgs,
}

#[derive(Debug, PartialEq, Eq)]
struct Settings {
    align: u64,
}

impl From<&Aux> for Settings {
    fn from(aux: &Aux) -> Self {
        Self::from_aux(aux)
    }
}

impl From<&mut Aux> for Settings {
    fn from(aux: &mut Aux) -> Self {
        Self::from_aux(aux)
    }
}

impl Settings {
    const UNIFORM_SIZE: u64 = size_of::<UniformArgs>() as u64;

    fn from_aux(aux: &Aux) -> Self {
        Settings { align: aux.align }
    }

    #[inline]
    fn buffer_frame_size(&self) -> u64 {
        ((Self::UNIFORM_SIZE - 1) / self.align + 1) * self.align
    }

    #[inline]
    fn uniform_offset(&self, index: u64) -> u64 {
        self.buffer_frame_size() * index as u64
    }
}

#[derive(Debug, Default)]
pub struct PipelineDesc;

#[derive(Debug)]
pub struct Pipeline<B: hal::Backend> {
    buffer: Escape<Buffer<B>>,
    sets: Vec<B::DescriptorSet>,
    descriptor_pool: B::DescriptorPool,
    image_sampler: Escape<Sampler<B>>,
    image_view: Escape<ImageView<B>>,
    settings: Settings,
}

impl<B> SimpleGraphicsPipelineDesc<B, specs::World> for PipelineDesc
where
    B: hal::Backend,
{
    type Pipeline = Pipeline<B>;

    fn images(&self) -> Vec<ImageAccess> {
        vec![ImageAccess {
            access: hal::image::Access::SHADER_READ,
            usage: hal::image::Usage::SAMPLED,
            layout: hal::image::Layout::ShaderReadOnlyOptimal,
            stages: hal::pso::PipelineStage::FRAGMENT_SHADER,
        }]
    }

    fn depth_stencil(&self) -> Option<hal::pso::DepthStencilDesc> {
        None
    }

    fn load_shader_set(
        &self,
        factory: &mut Factory<B>,
        _world: &specs::World,
    ) -> rendy::shader::ShaderSet<B> {
        SHADERS.build(factory, Default::default()).unwrap()
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
                    hal::pso::DescriptorSetLayoutBinding {
                        binding: 2,
                        ty: hal::pso::DescriptorType::UniformBuffer,
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
        ctx: &GraphContext<B>,
        factory: &mut Factory<B>,
        _queue: QueueId,
        world: &specs::World,
        buffers: Vec<NodeBuffer>,
        images: Vec<NodeImage>,
        set_layouts: &[Handle<DescriptorSetLayout<B>>],
    ) -> Result<Pipeline<B>, hal::pso::CreationError> {
        assert!(buffers.is_empty());
        assert!(images.len() == 1);
        assert!(set_layouts.len() == 1);

        let aux = world.read_resource::<Aux>();

        let settings: Settings = (&*aux).into();

        let frames = aux.frames;

        let mut descriptor_pool = unsafe {
            factory.create_descriptor_pool(
                frames,
                vec![
                    hal::pso::DescriptorRangeDesc {
                        ty: hal::pso::DescriptorType::Sampler,
                        count: frames,
                    },
                    hal::pso::DescriptorRangeDesc {
                        ty: hal::pso::DescriptorType::SampledImage,
                        count: frames,
                    },
                    hal::pso::DescriptorRangeDesc {
                        ty: hal::pso::DescriptorType::UniformBuffer,
                        count: frames,
                    },
                ],
                hal::pso::DescriptorPoolCreateFlags::empty(),
            )?
        };

        let image_sampler = factory
            .create_sampler(SamplerDesc::new(Filter::Nearest, WrapMode::Clamp))
            .unwrap();

        let image_handle = ctx
            .get_image(images[0].id)
            .expect("Tonemapper HDR image missing");

        let image_view = factory
            .create_image_view(
                image_handle.clone(),
                ImageViewInfo {
                    view_kind: ViewKind::D2,
                    format: hal::format::Format::Rgba32Sfloat,
                    swizzle: hal::format::Swizzle::NO,
                    range: images[0].range.clone(),
                },
            )
            .expect("Could not create tonemapper input image view");

        let buffer = factory
            .create_buffer(
                BufferInfo {
                    size: settings.buffer_frame_size() * aux.frames as u64,
                    usage: hal::buffer::Usage::UNIFORM,
                },
                rendy::memory::MemoryUsageValue::Dynamic,
            )
            .unwrap();

        let mut sets = Vec::with_capacity(frames);
        for index in 0..frames {
            unsafe {
                let set = descriptor_pool.allocate_set(&set_layouts[0].raw()).unwrap();
                factory.write_descriptor_sets(vec![
                    hal::pso::DescriptorSetWrite {
                        set: &set,
                        binding: 0,
                        array_offset: 0,
                        descriptors: Some(hal::pso::Descriptor::Sampler(image_sampler.raw())),
                    },
                    hal::pso::DescriptorSetWrite {
                        set: &set,
                        binding: 1,
                        array_offset: 0,
                        descriptors: Some(hal::pso::Descriptor::Image(
                            image_view.raw(),
                            hal::image::Layout::ShaderReadOnlyOptimal,
                        )),
                    },
                    hal::pso::DescriptorSetWrite {
                        set: &set,
                        binding: 2,
                        array_offset: 0,
                        descriptors: Some(hal::pso::Descriptor::Buffer(
                            buffer.raw(),
                            Some(settings.uniform_offset(index as u64))
                                ..Some(
                                    settings.uniform_offset(index as u64) + Settings::UNIFORM_SIZE,
                                ),
                        )),
                    },
                ]);
                sets.push(set);
            }
        }
        Ok(Pipeline {
            buffer,
            sets,
            image_view,
            image_sampler,
            descriptor_pool,
            settings,
        })
    }
}

impl<B> SimpleGraphicsPipeline<B, specs::World> for Pipeline<B>
where
    B: hal::Backend,
{
    type Desc = PipelineDesc;

    fn prepare(
        &mut self,
        factory: &Factory<B>,
        _queue: QueueId,
        _set_layouts: &[Handle<DescriptorSetLayout<B>>],
        index: usize,
        world: &specs::World,
    ) -> PrepareResult {
        let aux = world.read_resource::<Aux>();
        unsafe {
            factory
                .upload_visible_buffer(
                    &mut self.buffer,
                    self.settings.uniform_offset(index as u64),
                    &[UniformArgs {
                        tonemapper: aux.tonemapper_args,
                    }],
                )
                .unwrap()
        };
        PrepareResult::DrawReuse
    }

    fn draw(
        &mut self,
        layout: &B::PipelineLayout,
        mut encoder: RenderPassEncoder<'_, B>,
        index: usize,
        _world: &specs::World,
    ) {
        unsafe {
            encoder.bind_graphics_descriptor_sets(
                layout,
                0,
                Some(&self.sets[index]),
                std::iter::empty(),
            );
            // This is a trick from Sascha Willems which uses just the gl_VertexIndex
            // to calculate the position and uv coordinates for one full-scren "quad"
            // which is actually just a triangle with two of the vertices positioned
            // correctly off screen. This way we don't need a vertex buffer.
            encoder.draw(0..3, 0..1);
        }
    }

    fn dispose(mut self, factory: &mut Factory<B>, _world: &specs::World) {
        unsafe {
            self.descriptor_pool.reset();
            factory.destroy_descriptor_pool(self.descriptor_pool);
        }
    }
}
