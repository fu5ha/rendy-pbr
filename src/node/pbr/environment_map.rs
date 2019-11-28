use derivative::Derivative;

use genmesh::{
    generators::{IndexedPolygon, SharedVertex},
    Triangulate,
};

use rendy::{
    command::{QueueId, RenderPassEncoder},
    factory::Factory,
    graph::{render::*, GraphContext, NodeBuffer, NodeImage},
    hal::{device::Device, pso::DescriptorPool},
    memory::MemoryUsageValue,
    mesh::{AsVertex, Mesh, Position},
    resource::{Buffer, BufferInfo, DescriptorSetLayout, Escape, Handle},
    shader::{PathBufShaderInfo, ShaderKind, SourceLanguage},
};

use rendy::hal;

use crate::{
    components,
    node::pbr::{Aux, CameraArgs},
};

#[derive(Derivative)]
#[derivative(Default)]
pub enum CubeDisplay {
    #[derivative(Default)]
    Environment,
    Irradiance,
    Specular,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct UniformArgs {
    proj: nalgebra::Matrix4<f32>,
    view: nalgebra::Matrix4<f32>,
    roughness: f32,
}

lazy_static::lazy_static! {
    static ref VERTEX: PathBufShaderInfo = PathBufShaderInfo::new(
        std::path::PathBuf::from(crate::application_root_dir()).join("assets/shaders/environment_map.vert"),
        ShaderKind::Vertex,
        SourceLanguage::GLSL,
        "main",
    );

    static ref FRAGMENT: PathBufShaderInfo = PathBufShaderInfo::new(
        std::path::PathBuf::from(crate::application_root_dir()).join("assets/shaders/environment_map.frag"),
        ShaderKind::Fragment,
        SourceLanguage::GLSL,
        "main",
    );

    static ref SHADERS: rendy::shader::ShaderSetBuilder = rendy::shader::ShaderSetBuilder::default()
        .with_vertex(&*VERTEX).unwrap()
        .with_fragment(&*FRAGMENT).unwrap();
}

#[derive(Debug, PartialEq, Eq)]
pub struct Settings {
    align: u64,
}

impl Settings {
    const UNIFORM_SIZE: u64 = std::mem::size_of::<UniformArgs>() as u64;

    fn from_world<B: hal::Backend>(world: &specs::World) -> Self {
        let aux = world.read_resource::<Aux>();
        Self { align: aux.align }
    }

    #[inline]
    fn buffer_frame_size(&self) -> u64 {
        ((Self::UNIFORM_SIZE - 1) / self.align + 1) * self.align
    }
}

#[derive(Debug, Default)]
pub struct PipelineDesc;

pub struct Pipeline<B: hal::Backend> {
    cube: Mesh<B>,
    ubo_sets: Vec<B::DescriptorSet>,
    env_cubemap_set: B::DescriptorSet,
    irradiance_cubemap_set: B::DescriptorSet,
    spec_cubemap_set: B::DescriptorSet,
    settings: Settings,
    pool: B::DescriptorPool,
    #[allow(dead_code)]
    buffer: Escape<Buffer<B>>,
}

impl<B: hal::Backend> std::fmt::Debug for Pipeline<B> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Equirect Pipeline")
    }
}

impl<B> SimpleGraphicsPipelineDesc<B, specs::World> for PipelineDesc
where
    B: hal::Backend,
{
    type Pipeline = Pipeline<B>;

    fn vertices(
        &self,
    ) -> Vec<(
        Vec<hal::pso::Element<hal::format::Format>>,
        hal::pso::ElemStride,
        hal::pso::VertexInputRate,
    )> {
        vec![Position::vertex().gfx_vertex_input_desc(hal::pso::VertexInputRate::Vertex)]
    }

    fn load_shader_set(
        &self,
        factory: &mut Factory<B>,
        _aux: &specs::World,
    ) -> rendy::shader::ShaderSet<B> {
        SHADERS.build(factory, Default::default()).unwrap()
    }

    fn layout(&self) -> Layout {
        Layout {
            sets: vec![
                SetLayout {
                    bindings: vec![hal::pso::DescriptorSetLayoutBinding {
                        binding: 0,
                        ty: hal::pso::DescriptorType::UniformBuffer,
                        count: 1,
                        stage_flags: hal::pso::ShaderStageFlags::VERTEX
                            | hal::pso::ShaderStageFlags::FRAGMENT,
                        immutable_samplers: false,
                    }],
                },
                SetLayout {
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
                },
            ],
            push_constants: Vec::new(),
        }
    }

    fn build<'a>(
        self,
        _ctx: &GraphContext<B>,
        factory: &mut Factory<B>,
        queue: QueueId,
        world: &specs::World,
        buffers: Vec<NodeBuffer>,
        images: Vec<NodeImage>,
        set_layouts: &[Handle<DescriptorSetLayout<B>>],
    ) -> Result<Pipeline<B>, hal::pso::CreationError> {
        assert!(buffers.is_empty());
        assert!(images.is_empty());
        assert!(set_layouts.len() == 2);

        let aux = world.read_resource::<Aux>();
        let frames = aux.frames;
        let env_storage = world.read_resource::<super::EnvironmentStorage<B>>();

        let cube = genmesh::generators::Cube::new();
        let cube_vertices: Vec<_> = cube
            .shared_vertex_iter()
            .map(|v| Position(v.pos.into()))
            .collect();

        let cube_flattened_vertices: Vec<_> =
            genmesh::Vertices::vertices(cube.indexed_polygon_iter().triangulate())
                .map(|i| cube_vertices[i])
                .collect();

        let cube = Mesh::<B>::builder()
            .with_vertices(&cube_flattened_vertices[..])
            .build(queue, factory)
            .unwrap();

        let mut pool = unsafe {
            factory
                .create_descriptor_pool(
                    frames + 3,
                    vec![
                        hal::pso::DescriptorRangeDesc {
                            ty: hal::pso::DescriptorType::UniformBuffer,
                            count: frames,
                        },
                        hal::pso::DescriptorRangeDesc {
                            ty: hal::pso::DescriptorType::Sampler,
                            count: 3,
                        },
                        hal::pso::DescriptorRangeDesc {
                            ty: hal::pso::DescriptorType::SampledImage,
                            count: 3,
                        },
                    ],
                    hal::pso::DescriptorPoolCreateFlags::empty(),
                )
                .unwrap()
        };

        let settings = Settings::from_world::<B>(world);

        let buffer = factory
            .create_buffer(
                BufferInfo {
                    size: settings.buffer_frame_size() * frames as u64,
                    usage: hal::buffer::Usage::UNIFORM,
                },
                MemoryUsageValue::Dynamic,
            )
            .unwrap();

        let mut ubo_sets = Vec::new();
        for frame in 0..frames {
            ubo_sets.push(unsafe {
                let set = pool.allocate_set(&set_layouts[0].raw()).unwrap();
                factory.write_descriptor_sets(vec![hal::pso::DescriptorSetWrite {
                    set: &set,
                    binding: 0,
                    array_offset: 0,
                    descriptors: Some(hal::pso::Descriptor::Buffer(
                        buffer.raw(),
                        Some(settings.buffer_frame_size() * frame as u64)
                            ..Some(settings.buffer_frame_size() * (frame + 1) as u64),
                    )),
                }]);
                set
            });
        }

        let env_cubemap_set = unsafe {
            let set = pool.allocate_set(&set_layouts[1].raw()).unwrap();
            factory.write_descriptor_sets(vec![
                hal::pso::DescriptorSetWrite {
                    set: &set,
                    binding: 0,
                    array_offset: 0,
                    descriptors: Some(hal::pso::Descriptor::Sampler(
                        env_storage.env_cube.as_ref().unwrap().sampler().raw(),
                    )),
                },
                hal::pso::DescriptorSetWrite {
                    set: &set,
                    binding: 1,
                    array_offset: 0,
                    descriptors: Some(hal::pso::Descriptor::Image(
                        env_storage.env_cube.as_ref().unwrap().view().raw(),
                        hal::image::Layout::ShaderReadOnlyOptimal,
                    )),
                },
            ]);
            set
        };

        let irradiance_cubemap_set = unsafe {
            let set = pool.allocate_set(&set_layouts[1].raw()).unwrap();
            factory.write_descriptor_sets(vec![
                hal::pso::DescriptorSetWrite {
                    set: &set,
                    binding: 0,
                    array_offset: 0,
                    descriptors: Some(hal::pso::Descriptor::Sampler(
                        env_storage
                            .irradiance_cube
                            .as_ref()
                            .unwrap()
                            .sampler()
                            .raw(),
                    )),
                },
                hal::pso::DescriptorSetWrite {
                    set: &set,
                    binding: 1,
                    array_offset: 0,
                    descriptors: Some(hal::pso::Descriptor::Image(
                        env_storage.irradiance_cube.as_ref().unwrap().view().raw(),
                        hal::image::Layout::ShaderReadOnlyOptimal,
                    )),
                },
            ]);
            set
        };

        let spec_cubemap_set = unsafe {
            let set = pool.allocate_set(&set_layouts[1].raw()).unwrap();
            factory.write_descriptor_sets(vec![
                hal::pso::DescriptorSetWrite {
                    set: &set,
                    binding: 0,
                    array_offset: 0,
                    descriptors: Some(hal::pso::Descriptor::Sampler(
                        env_storage.spec_cube.as_ref().unwrap().sampler().raw(),
                    )),
                },
                hal::pso::DescriptorSetWrite {
                    set: &set,
                    binding: 1,
                    array_offset: 0,
                    descriptors: Some(hal::pso::Descriptor::Image(
                        env_storage.spec_cube.as_ref().unwrap().view().raw(),
                        hal::image::Layout::ShaderReadOnlyOptimal,
                    )),
                },
            ]);
            set
        };

        Ok(Pipeline {
            cube,
            ubo_sets,
            env_cubemap_set,
            irradiance_cubemap_set,
            spec_cubemap_set,
            settings,
            pool,
            buffer,
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
        use specs::prelude::*;

        let aux = world.read_resource::<Aux>();
        let transforms = world.read_storage::<components::GlobalTransform>();
        let cameras = world.read_storage::<components::Camera>();
        let active_cameras = world.read_storage::<components::ActiveCamera>();
        let mut camera_args: CameraArgs = (&active_cameras, &cameras, &transforms)
            .join()
            .map(|(_, cam, trans)| (cam, trans).into())
            .next()
            .expect("No active camera!");

        camera_args.view.column_mut(3)[0] = 0.0;
        camera_args.view.column_mut(3)[1] = 0.0;
        camera_args.view.column_mut(3)[2] = 0.0;

        unsafe {
            factory
                .upload_visible_buffer(
                    &mut self.buffer,
                    self.settings.buffer_frame_size() * index as u64,
                    &[UniformArgs {
                        proj: camera_args.proj,
                        view: camera_args.view,
                        roughness: match aux.cube_display {
                            CubeDisplay::Irradiance => 0.0,
                            CubeDisplay::Environment => 0.0,
                            CubeDisplay::Specular => aux.cube_roughness,
                        },
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
        world: &specs::World,
    ) {
        assert!(self
            .cube
            .bind(0, &[Position::vertex()], &mut encoder)
            .is_ok());
        let cube_set = match world.read_resource::<Aux>().cube_display {
            CubeDisplay::Irradiance => &self.irradiance_cubemap_set,
            CubeDisplay::Environment => &self.env_cubemap_set,
            CubeDisplay::Specular => &self.spec_cubemap_set,
        };
        unsafe {
            encoder.bind_graphics_descriptor_sets(
                layout,
                0,
                vec![&self.ubo_sets[index], cube_set],
                std::iter::empty(),
            );
            encoder.draw(0..36, 0..1);
        }
    }

    fn dispose(mut self, factory: &mut Factory<B>, _aux: &specs::World) {
        unsafe {
            self.pool.reset();
            factory.destroy_descriptor_pool(self.pool);
        }
    }
}
