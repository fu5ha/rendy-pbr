#![cfg_attr(
    not(any(feature = "dx12", feature = "metal", feature = "vulkan")),
    allow(unused)
)]

use rendy::{
    command::{DrawIndexedCommand, QueueId, RenderPassEncoder},
    factory::{Config, Factory},
    graph::{present::PresentNode, render::*, GraphBuilder, NodeBuffer, NodeImage},
    hal::{pso::DescriptorPool, Device},
    memory::MemoryUsageValue,
    mesh::{AsVertex, Mesh, PosNormTex, Transform},
    resource::buffer::Buffer,
    shader::{Shader, ShaderKind, SourceLanguage, StaticShaderInfo},
    texture::{pixel::Rgba8Srgb, Texture, TextureBuilder},
};

use gfx_hal as hal;

use std::{mem::size_of, time};

use genmesh::{
    generators::{IndexedPolygon, SharedVertex},
    Triangulate,
};

use rand::distributions::{Distribution, Uniform};

use winit::{EventsLoop, WindowBuilder, Event, WindowEvent};

#[cfg(feature = "dx12")]
type Backend = rendy::dx12::Backend;

#[cfg(feature = "metal")]
type Backend = rendy::metal::Backend;

#[cfg(feature = "vulkan")]
type Backend = rendy::vulkan::Backend;

#[cfg(feature = "empty")]
type Backend = rendy::empty::Backend;

lazy_static::lazy_static! {
    static ref VERTEX: StaticShaderInfo = StaticShaderInfo::new(
        concat!(env!("CARGO_MANIFEST_DIR"), "/examples/shaders/instanced_cube/cube.vert"),
        ShaderKind::Vertex,
        SourceLanguage::GLSL,
        "main",
    );

    static ref FRAGMENT: StaticShaderInfo = StaticShaderInfo::new(
        concat!(env!("CARGO_MANIFEST_DIR"), "/examples/shaders/instanced_cube/cube.frag"),
        ShaderKind::Fragment,
        SourceLanguage::GLSL,
        "main",
    );
}

#[derive(Clone, Copy)]
#[repr(C, align(16))]
struct CameraArgs {
    proj: nalgebra::Matrix4<f32>,
    view: nalgebra::Matrix4<f32>,
    camera_pos: nalgebra::Vector3<f32>,
    _pad: f32,
}

#[derive(Clone, Copy)]
#[repr(C, align(16))]
struct DepthUniformArgs {
    camera: CameraArgs,
}

#[derive(Debug)]
struct Camera {
    view: nalgebra::Isometry3<f32>,
    proj: nalgebra::Perspective3<f32>,
}

struct Object<B: hal::Backend> {
    mesh: Mesh<B>,
    material: Material<B>,
}

struct Material<B: hal::Backend> {
    albedo: Texture<B>,
    normal: Texture<B>,
    metallic_roughness: Texture<B>,
    ao: Texture<B>,
}

struct Environment<B: hal::Backend> {
    mesh: Mesh<B>,
    tex: Texture<B>,
}

struct Objects<B: hal::Backend> {
    objects: Vec<(Object<B>, Vec<nalgebra::Matrix4<f32>>)>,
}

struct Scene<B: hal::Backend> {
    camera: Camera,
    objects: Vec<(Object<B>, Vec<nalgebra::Matrix4<f32>>)>,
    environment: Environment<B>,
}

struct Aux<B: hal::Backend> {
    frames: usize,
    align: u64,
    scene: Scene<B>,
}


#[derive(Debug, Default)]
struct MeshRenderPipelineDesc;

impl MeshRenderPipelineDesc {
    const MAX_OBJECTS: usize = 20_000;

    const UNIFORM_SIZE: u64 = size_of::<CameraArgs>() as u64;
    const TRANSFORMS_SIZE: u64 = size_of::<Transform>() as u64 * Self::MAX_OBJECTS as u64;
    const INDIRECT_SIZE: u64 = size_of::<DrawIndexedCommand>() as u64;

    const fn buffer_frame_size(align: u64) -> u64 {
        ((Self::UNIFORM_SIZE + Self::TRANSFORMS_SIZE + Self::INDIRECT_SIZE - 1) / align + 1) * align
    }

    const fn uniform_offset(index: usize, align: u64) -> u64 {
        buffer_frame_size(align) * index as u64
    }

    const fn transforms_offset(index: usize, align: u64) -> u64 {
        uniform_offset(index, align) + UNIFORM_SIZE
    }

    const fn indirect_offset(index: usize, align: u64) -> u64 {
        transforms_offset(index, align) + TRANSFORMS_SIZE
    }
}

#[derive(Debug)]
struct MeshRenderPipeline<B: gfx_hal::Backend> {
    descriptor_pool: B::DescriptorPool,
    buffer: Buffer<B>,
    sets: Vec<B::DescriptorSet>,
    cube_mesh: Mesh<B>,
    cube_texture: Texture<B>,
}

impl<B> SimpleGraphicsPipelineDesc<B, Aux> for MeshRenderPipelineDesc
where
    B: gfx_hal::Backend,
{
    type Pipeline = MeshRenderPipeline<B>;

    fn layout(&self) -> Layout {
        Layout {
            sets: vec![SetLayout {
                bindings: vec![
                    gfx_hal::pso::DescriptorSetLayoutBinding {
                        binding: 0,
                        ty: gfx_hal::pso::DescriptorType::UniformBuffer,
                        count: 1,
                        stage_flags: gfx_hal::pso::ShaderStageFlags::GRAPHICS,
                        immutable_samplers: false,
                    },
                    gfx_hal::pso::DescriptorSetLayoutBinding {
                        binding: 1,
                        ty: gfx_hal::pso::DescriptorType::SampledImage,
                        count: 1,
                        stage_flags: gfx_hal::pso::ShaderStageFlags::FRAGMENT,
                        immutable_samplers: false,
                    },
                    gfx_hal::pso::DescriptorSetLayoutBinding {
                        binding: 2,
                        ty: gfx_hal::pso::DescriptorType::Sampler,
                        count: 1,
                        stage_flags: gfx_hal::pso::ShaderStageFlags::FRAGMENT,
                        immutable_samplers: false,
                    },
                ],
            }],
            push_constants: Vec::new(),
        }
    }

    fn vertices(
        &self,
    ) -> Vec<(
        Vec<gfx_hal::pso::Element<gfx_hal::format::Format>>,
        gfx_hal::pso::ElemStride,
        gfx_hal::pso::InstanceRate,
    )> {
        vec![
            PosNormTex::VERTEX.gfx_vertex_input_desc(0),
            Transform::VERTEX.gfx_vertex_input_desc(1),
        ]
    }

    fn load_shader_set<'a>(
        &self,
        storage: &'a mut Vec<B::ShaderModule>,
        factory: &mut Factory<B>,
        _aux: &mut Aux,
    ) -> gfx_hal::pso::GraphicsShaderSet<'a, B> {
        storage.clear();

        log::trace!("Load shader module '{:#?}'", *VERTEX);
        storage.push(VERTEX.module(factory).unwrap());

        log::trace!("Load shader module '{:#?}'", *FRAGMENT);
        storage.push(FRAGMENT.module(factory).unwrap());

        gfx_hal::pso::GraphicsShaderSet {
            vertex: gfx_hal::pso::EntryPoint {
                entry: "main",
                module: &storage[0],
                specialization: gfx_hal::pso::Specialization::default(),
            },
            fragment: Some(gfx_hal::pso::EntryPoint {
                entry: "main",
                module: &storage[1],
                specialization: gfx_hal::pso::Specialization::default(),
            }),
            hull: None,
            domain: None,
            geometry: None,
        }
    }

    fn build<'a>(
        self,
        factory: &mut Factory<B>,
        queue: QueueId,
        aux: &mut Aux,
        buffers: Vec<NodeBuffer<'a, B>>,
        images: Vec<NodeImage<'a, B>>,
        set_layouts: &[B::DescriptorSetLayout],
    ) -> Result<MeshRenderPipeline<B>, failure::Error> {
        assert!(buffers.is_empty());
        assert!(images.is_empty());
        assert_eq!(set_layouts.len(), 1);

        let frames = aux.frames;

        let mut descriptor_pool = unsafe {
            factory.create_descriptor_pool(
                frames,
                vec![
                    gfx_hal::pso::DescriptorRangeDesc {
                        ty: gfx_hal::pso::DescriptorType::UniformBuffer,
                        count: frames,
                    },
                    gfx_hal::pso::DescriptorRangeDesc {
                        ty: gfx_hal::pso::DescriptorType::Sampler,
                        count: frames,
                    },
                    gfx_hal::pso::DescriptorRangeDesc {
                        ty: gfx_hal::pso::DescriptorType::SampledImage,
                        count: frames,
                    },
                ],
            )
        }
        .unwrap();

        let buffer = factory
            .create_buffer(
                aux.align,
                buffer_frame_size(aux.align) * frames as u64,
                (
                    gfx_hal::buffer::Usage::UNIFORM
                        | gfx_hal::buffer::Usage::INDIRECT
                        | gfx_hal::buffer::Usage::VERTEX,
                    MemoryUsageValue::Dynamic,
                ),
            )
            .unwrap();

        let cube = genmesh::generators::Cube::new();
        let cube_indices: Vec<_> =
            genmesh::Vertices::vertices(cube.indexed_polygon_iter().triangulate())
                .map(|i| i as u32)
                .collect();
        assert_eq!(cube_indices.len(), 36);
        let cube_vertices: Vec<_> = cube
            .shared_vertex_iter()
            .map(|v| {
                let n = v.normal;
                let p = v.pos;
                let t = if n.x != 0.0 {
                    [p.z * n.x * 0.5 + 0.5, -p.y * 0.5 + 0.5]
                } else if n.y != 0.0 {
                    [p.x * n.y * 0.5 + 0.5, -p.z * 0.5 + 0.5]
                } else {
                    [p.x * -n.z * 0.5 + 0.5, -p.y * 0.5 + 0.5]
                };
                PosNormTex {
                    position: p.into(),
                    normal: n.into(),
                    tex_coord: t.into(),
                }
            })
            .collect();

        let cube_mesh = Mesh::<Backend>::builder()
            .with_indices(&cube_indices[..])
            .with_vertices(&cube_vertices[..])
            .build(queue, factory)
            .unwrap();

        let cube_tex_bytes = include_bytes!("../assets/gltf/helmet/SciFiHelmet_BaseColor.png");
        let cube_tex_img = image::load_from_memory(&cube_tex_bytes[..])
            .unwrap()
            .to_rgba();

        let (w, h) = cube_tex_img.dimensions();

        let cube_tex_image_data: Vec<Rgba8Srgb> = cube_tex_img
            .pixels()
            .map(|p| Rgba8Srgb { repr: p.data })
            .collect::<_>();

        let cube_tex_builder = TextureBuilder::new()
            .with_kind(gfx_hal::image::Kind::D2(w, h, 1, 1))
            .with_view_kind(gfx_hal::image::ViewKind::D2)
            .with_data_width(w)
            .with_data_height(h)
            .with_data(&cube_tex_image_data);

        let cube_texture = cube_tex_builder
            .build(
                queue,
                gfx_hal::image::Access::SHADER_READ,
                gfx_hal::image::Layout::ShaderReadOnlyOptimal,
                factory,
            )
            .unwrap();

        let mut sets = Vec::with_capacity(frames);

        for index in 0..frames {
            unsafe {
                let set = descriptor_pool.allocate_set(&set_layouts[0]).unwrap();
                factory.write_descriptor_sets(vec![
                    gfx_hal::pso::DescriptorSetWrite {
                        set: &set,
                        binding: 0,
                        array_offset: 0,
                        descriptors: Some(gfx_hal::pso::Descriptor::Buffer(
                            buffer.raw(),
                            Some(uniform_offset(index, aux.align))..Some(uniform_offset(index, aux.align) + UNIFORM_SIZE),
                        )),
                    },
                    gfx_hal::pso::DescriptorSetWrite {
                        set: &set,
                        binding: 1,
                        array_offset: 0,
                        descriptors: Some(gfx_hal::pso::Descriptor::Image(
                            cube_texture.image_view.raw(),
                            gfx_hal::image::Layout::ShaderReadOnlyOptimal,
                        )),
                    },
                    gfx_hal::pso::DescriptorSetWrite {
                        set: &set,
                        binding: 2,
                        array_offset: 0,
                        descriptors: Some(gfx_hal::pso::Descriptor::Sampler(
                            cube_texture.sampler.raw(),
                        )),
                    },
                ]);
                sets.push(set);
            }
        }

        Ok(MeshRenderPipeline {
            descriptor_pool,
            buffer,
            cube_mesh,
            cube_texture,
            sets,
        })
    }
}

impl<B> SimpleGraphicsPipeline<B, Aux> for MeshRenderPipeline<B>
where
    B: gfx_hal::Backend,
{
    type Desc = MeshRenderPipelineDesc;

    fn prepare(
        &mut self,
        factory: &Factory<B>,
        _queue: QueueId,
        _set_layouts: &[B::DescriptorSetLayout],
        index: usize,
        aux: &Aux,
    ) -> PrepareResult {
        unsafe {
            factory
                .upload_visible_buffer(
                    &mut self.buffer,
                    uniform_offset(index, aux.align),
                    &[UniformArgs {
                        proj: {
                            let mut proj = aux.scene.camera.proj.to_homogeneous();
                            proj[(1, 1)] *= -1.0;
                            proj
                        },
                        view: aux.scene.camera.view.inverse().to_homogeneous(),
                    }],
                )
                .unwrap()
        };

        unsafe {
            factory
                .upload_visible_buffer(
                    &mut self.buffer,
                    indirect_offset(index, aux.align),
                    &[DrawIndexedCommand {
                        index_count: self.cube_mesh.len(),
                        instance_count: aux.scene.objects.len() as u32,
                        first_index: 0,
                        vertex_offset: 0,
                        first_instance: 0,
                    }],
                )
                .unwrap()
        };

        unsafe {
            factory
                .upload_visible_buffer(
                    &mut self.buffer,
                    transforms_offset(index, aux.align),
                    &aux.scene.objects[..],
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
        aux: &Aux,
    ) {
        encoder.bind_graphics_descriptor_sets(
            layout,
            0,
            Some(&self.sets[index]),
            std::iter::empty(),
        );
        assert!(self
            .cube_mesh
            .bind(&[PosNormTex::VERTEX], &mut encoder)
            .is_ok());
        encoder.bind_vertex_buffers(
            1,
            std::iter::once((self.buffer.raw(), transforms_offset(index, aux.align))),
        );
        encoder.draw_indexed_indirect(
            self.buffer.raw(),
            indirect_offset(index, aux.align),
            1,
            INDIRECT_SIZE as u32,
        );
    }

    fn dispose(mut self, factory: &mut Factory<B>, _aux: &mut Aux) {
        unsafe {
            self.descriptor_pool
                .free_sets(self.sets.into_iter());
            factory.destroy_descriptor_pool(self.descriptor_pool);
        }
    }
}

#[cfg(any(feature = "dx12", feature = "metal", feature = "vulkan"))]
fn main() {
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Warn)
        .filter_module("instanced_cube", log::LevelFilter::Trace)
        .init();

    let config: Config = Default::default();

    let (mut factory, mut families): (Factory<Backend>, _) = rendy::factory::init(config).unwrap();

    let mut event_loop = EventsLoop::new();

    let window = WindowBuilder::new()
        .with_title("Rendy example")
        .build(&event_loop)
        .unwrap();

    event_loop.poll_events(|_| ());

    let surface = factory.create_surface(window.into());
    let aspect = surface.aspect();

    let mut graph_builder = GraphBuilder::<Backend, Aux>::new();

    let color = graph_builder.create_image(
        surface.kind(),
        1,
        factory.get_surface_format(&surface),
        MemoryUsageValue::Data,
        Some(gfx_hal::command::ClearValue::Color(
            [1.0, 1.0, 1.0, 1.0].into(),
        )),
    );

    let depth = graph_builder.create_image(
        surface.kind(),
        1,
        gfx_hal::format::Format::D16Unorm,
        MemoryUsageValue::Data,
        Some(gfx_hal::command::ClearValue::DepthStencil(
            gfx_hal::command::ClearDepthStencil(1.0, 0),
        )),
    );

    let pass = graph_builder.add_node(
        MeshRenderPipeline::builder()
            .into_subpass()
            .with_color(color)
            .with_depth_stencil(depth)
            .into_pass(),
    );

    let present_builder = PresentNode::builder(surface, factory.physical(), color)
        .with_dependency(pass);

    let frames = present_builder.image_count() as usize;

    graph_builder.add_node(present_builder);

    let mut aux = Aux {
        frames,
        align: gfx_hal::adapter::PhysicalDevice::limits(factory.physical())
            .min_uniform_buffer_offset_alignment,
        scene: Scene {
            camera: Camera {
                proj: nalgebra::Perspective3::new(aspect, 3.1415 / 4.0, 1.0, 200.0),
                view: nalgebra::Isometry3::identity() * nalgebra::Translation3::new(0.0, 0.0, 10.0),
            },
            objects: vec![],
        },
    };

    log::info!("{:#?}", aux.scene);

    let mut graph = graph_builder
        .build(&mut factory, &mut families, &mut aux)
        .unwrap();

    let started = time::Instant::now();

    let mut frames = 0u64..;
    let mut rng = rand::thread_rng();
    let rxy = Uniform::new(-1.0, 1.0);
    let rz = Uniform::new(0.0, 185.0);

    let mut fpss = Vec::new();
    let mut checkpoint = started;
    let mut should_close = false;

    while aux.scene.objects.len() < MAX_OBJECTS && !should_close {
        let start = frames.start;
        let from = aux.scene.objects.len();
        for _ in &mut frames {
            factory.maintain(&mut families);
            event_loop.poll_events(|event| match event {
                Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => should_close = true,
                _ => (),
            });
            graph.run(&mut factory, &mut families, &mut aux);

            let elapsed = checkpoint.elapsed();

            if aux.scene.objects.len() < MAX_OBJECTS {
                aux.scene.objects.push({
                    let z = rz.sample(&mut rng);
                    nalgebra::Translation3::new(
                        rxy.sample(&mut rng) * (z / 2.0 + 4.0),
                        rxy.sample(&mut rng) * (z / 2.0 + 4.0),
                        -z,
                    )
                    .to_homogeneous()
                })
            }

            if should_close || elapsed > std::time::Duration::new(5, 0) || aux.scene.objects.len() == MAX_OBJECTS {
                let frames = frames.start - start;
                let nanos = elapsed.as_secs() * 1_000_000_000 + elapsed.subsec_nanos() as u64;
                fpss.push((frames * 1_000_000_000 / nanos, from..aux.scene.objects.len()));
                checkpoint += elapsed;
                break;
            }
        }
    }

    log::info!("FPS: {:#?}", fpss);

    graph.dispose(&mut factory, &mut aux);
}

#[cfg(not(any(feature = "dx12", feature = "metal", feature = "vulkan")))]
fn main() {
    panic!("Specify feature: { dx12, metal, vulkan }");
}