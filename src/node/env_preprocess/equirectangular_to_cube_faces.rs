use genmesh::{
    generators::{IndexedPolygon, SharedVertex},
    Triangulate,
};

use rendy::{
    command::{QueueId, RenderPassEncoder},
    factory::Factory,
    graph::{render::*, GraphContext, NodeBuffer, NodeImage},
    hal::{pso::DescriptorPool, Device},
    memory::MemoryUsageValue,
    mesh::{AsVertex, Mesh, Position},
    resource::{Buffer, BufferInfo, DescriptorSetLayout, Escape, Handle},
    shader::{PathBufShaderInfo, Shader, ShaderKind, SourceLanguage},
};

use rendy::hal;

use crate::node::env_preprocess::Aux;

#[derive(Clone, Copy)]
#[repr(C)]
pub struct UniformArgs {
    proj: nalgebra::Matrix4<f32>,
    views: [nalgebra::Matrix4<f32>; 6],
}

lazy_static::lazy_static! {
    static ref VERTEX: PathBufShaderInfo = PathBufShaderInfo::new(
        std::path::PathBuf::from(crate::application_root_dir()).join("assets/shaders/equirectangular_to_cube_faces.vert"),
        ShaderKind::Vertex,
        SourceLanguage::GLSL,
        "main",
    );

    static ref FRAGMENT: PathBufShaderInfo = PathBufShaderInfo::new(
        std::path::PathBuf::from(crate::application_root_dir()).join("assets/shaders/equirectangular_to_cube_faces.frag"),
        ShaderKind::Fragment,
        SourceLanguage::GLSL,
        "main",
    );
}

#[derive(Debug, PartialEq, Eq)]
pub struct Settings {
    align: u64,
}

impl<B: hal::Backend> From<&Aux<B>> for Settings {
    fn from(aux: &Aux<B>) -> Self {
        Self::from_aux(aux)
    }
}

impl<B: hal::Backend> From<&mut Aux<B>> for Settings {
    fn from(aux: &mut Aux<B>) -> Self {
        Self::from_aux(aux)
    }
}

impl Settings {
    const UNIFORM_SIZE: u64 = std::mem::size_of::<UniformArgs>() as u64;

    fn from_aux<B: hal::Backend>(aux: &Aux<B>) -> Self {
        Settings { align: aux.align }
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
    set: B::DescriptorSet,
    pool: B::DescriptorPool,
    #[allow(dead_code)]
    buffer: Escape<Buffer<B>>,
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

    fn vertices(
        &self,
    ) -> Vec<(
        Vec<hal::pso::Element<hal::format::Format>>,
        hal::pso::ElemStride,
        hal::pso::InstanceRate,
    )> {
        vec![Position::VERTEX.gfx_vertex_input_desc(0)]
    }

    fn colors(&self) -> Vec<hal::pso::ColorBlendDesc> {
        vec![hal::pso::ColorBlendDesc(hal::pso::ColorMask::ALL, hal::pso::BlendState::ALPHA,); 6]
    }

    fn depth_stencil(&self) -> Option<hal::pso::DepthStencilDesc> {
        None
    }

    fn load_shader_set<'a>(
        &self,
        storage: &'a mut Vec<B::ShaderModule>,
        factory: &mut Factory<B>,
        _aux: &Aux<B>,
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
                specialization: hal::pso::Specialization::default(),
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
                        ty: hal::pso::DescriptorType::UniformBuffer,
                        count: 1,
                        stage_flags: hal::pso::ShaderStageFlags::GRAPHICS,
                        immutable_samplers: false,
                    },
                    hal::pso::DescriptorSetLayoutBinding {
                        binding: 1,
                        ty: hal::pso::DescriptorType::Sampler,
                        count: 1,
                        stage_flags: hal::pso::ShaderStageFlags::FRAGMENT,
                        immutable_samplers: false,
                    },
                    hal::pso::DescriptorSetLayoutBinding {
                        binding: 2,
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
        queue: QueueId,
        aux: &Aux<B>,
        buffers: Vec<NodeBuffer>,
        images: Vec<NodeImage>,
        set_layouts: &[Handle<DescriptorSetLayout<B>>],
    ) -> Result<Pipeline<B>, failure::Error> {
        assert!(buffers.is_empty());
        assert!(images.is_empty());
        assert!(set_layouts.len() == 1);

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
            .build(queue, factory)?;

        let mut pool = unsafe {
            factory.create_descriptor_pool(
                1,
                vec![
                    hal::pso::DescriptorRangeDesc {
                        ty: hal::pso::DescriptorType::UniformBuffer,
                        count: 1,
                    },
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

        let settings: Settings = aux.into();

        let mut buffer = factory.create_buffer(
            BufferInfo {
                size: settings.buffer_frame_size(),
                usage: hal::buffer::Usage::UNIFORM,
            },
            MemoryUsageValue::Dynamic,
        )?;

        let set = unsafe {
            let set = pool.allocate_set(&set_layouts[0].raw())?;
            factory.write_descriptor_sets(vec![
                hal::pso::DescriptorSetWrite {
                    set: &set,
                    binding: 0,
                    array_offset: 0,
                    descriptors: Some(hal::pso::Descriptor::Buffer(
                        buffer.raw(),
                        Some(0)..Some(Settings::UNIFORM_SIZE),
                    )),
                },
                hal::pso::DescriptorSetWrite {
                    set: &set,
                    binding: 1,
                    array_offset: 0,
                    descriptors: Some(hal::pso::Descriptor::Sampler(
                        aux.equirectangular_texture.sampler().raw(),
                    )),
                },
                hal::pso::DescriptorSetWrite {
                    set: &set,
                    binding: 2,
                    array_offset: 0,
                    descriptors: Some(hal::pso::Descriptor::Image(
                        aux.equirectangular_texture.view().raw(),
                        hal::image::Layout::ShaderReadOnlyOptimal,
                    )),
                },
            ]);
            set
        };

        let origin = nalgebra::Point3::origin();
        unsafe {
            factory.upload_visible_buffer(
                &mut buffer,
                0,
                &[UniformArgs {
                    proj: {
                        let mut proj = nalgebra::Perspective3::<f32>::new(
                            1.0,
                            std::f32::consts::FRAC_PI_2,
                            0.1,
                            100.0,
                        )
                        .to_homogeneous();
                        proj[(1, 1)] *= -1.0;
                        proj
                    },
                    views: [
                        nalgebra::Matrix4::look_at_rh(
                            &origin,
                            &nalgebra::Point3::new(1.0, 0.0, 0.0),
                            &nalgebra::Vector3::y(),
                        ),
                        nalgebra::Matrix4::look_at_rh(
                            &origin,
                            &nalgebra::Point3::new(-1.0, 0.0, 0.0),
                            &nalgebra::Vector3::y(),
                        ),
                        nalgebra::Matrix4::look_at_rh(
                            &origin,
                            &nalgebra::Point3::new(0.0, 1.0, 0.0),
                            &nalgebra::Vector3::z(),
                        ),
                        nalgebra::Matrix4::look_at_rh(
                            &origin,
                            &nalgebra::Point3::new(0.0, -1.0, 0.0),
                            &-nalgebra::Vector3::z(),
                        ),
                        nalgebra::Matrix4::look_at_rh(
                            &origin,
                            &nalgebra::Point3::new(0.0, 0.0, 1.0),
                            &nalgebra::Vector3::y(),
                        ),
                        nalgebra::Matrix4::look_at_rh(
                            &origin,
                            &nalgebra::Point3::new(0.0, 0.0, -1.0),
                            &nalgebra::Vector3::y(),
                        ),
                    ],
                }],
            )?
        };

        Ok(Pipeline {
            cube,
            set,
            pool,
            buffer,
        })
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
        assert!(self.cube.bind(&[Position::VERTEX], &mut encoder).is_ok());
        encoder.bind_graphics_descriptor_sets(layout, 0, Some(&self.set), std::iter::empty());
        encoder.draw(0..36, 0..1);
    }

    fn dispose(mut self, factory: &mut Factory<B>, _aux: &Aux<B>) {
        unsafe {
            self.pool.reset();
            factory.destroy_descriptor_pool(self.pool);
        }
    }
}
