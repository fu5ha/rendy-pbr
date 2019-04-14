use rendy::{
    command::{DrawIndexedCommand, QueueId, RenderPassEncoder},
    factory::Factory,
    graph::{render::*, GraphContext, NodeBuffer, NodeImage},
    hal::{pso::DescriptorPool, Device},
    memory::MemoryUsageValue,
    mesh::{AsVertex, PosNormTangTex, Transform},
    resource::{
        Buffer, BufferInfo, DescriptorSetLayout, Escape, Filter, Handle, Sampler, SamplerInfo,
        WrapMode,
    },
    shader::{Shader, ShaderKind, SourceLanguage, StaticShaderInfo},
};

use std::mem::size_of;

use rendy::hal;

use crate::{
    asset, components,
    node::pbr::{Aux, CameraArgs},
    systems,
};

lazy_static::lazy_static! {
    static ref VERTEX: StaticShaderInfo = StaticShaderInfo::new(
        concat!(env!("CARGO_MANIFEST_DIR"), "/assets/shaders/pbr.vert"),
        ShaderKind::Vertex,
        SourceLanguage::GLSL,
        "main",
    );

    static ref FRAGMENT: StaticShaderInfo = StaticShaderInfo::new(
        concat!(env!("CARGO_MANIFEST_DIR"), "/assets/shaders/pbr.frag"),
        ShaderKind::Fragment,
        SourceLanguage::GLSL,
        "main",
    );
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct UniformArgs {
    camera: CameraArgs,
    num_lights: i32,
    lights: [super::LightData; crate::MAX_LIGHTS],
}

#[derive(Debug, Default)]
pub struct PipelineDesc;

#[derive(Debug)]
pub struct Pipeline<B: hal::Backend> {
    descriptor_pool: B::DescriptorPool,
    uniform_indirect_buffer: Escape<Buffer<B>>,
    transform_buffer: Escape<Buffer<B>>,
    texture_sampler: Escape<Sampler<B>>,
    frame_sets: Vec<B::DescriptorSet>,
    mat_sets: Vec<B::DescriptorSet>,
    settings: Settings,
}

#[derive(Debug, PartialEq, Eq)]
struct Settings {
    align: u64,
    num_primitives: usize,
    max_mesh_instances: Vec<u16>,
    total_max_mesh_instances: u64,
}

impl Settings {
    const UNIFORM_SIZE: u64 = size_of::<UniformArgs>() as u64;

    fn from_world<B: hal::Backend>(world: &specs::World) -> Self {
        let aux = world.read_resource::<Aux>();

        let mesh_storage = world.read_resource::<asset::MeshStorage>();
        let primitive_storage = world.read_resource::<asset::PrimitiveStorage<B>>();

        let max_mesh_instances = mesh_storage
            .0
            .iter()
            .map(|mesh| mesh.max_instances)
            .collect::<Vec<_>>();

        let total_max_mesh_instances = max_mesh_instances.iter().map(|n| *n as u64).sum();

        Settings {
            align: aux.align,
            num_primitives: primitive_storage.0.len(),
            max_mesh_instances,
            total_max_mesh_instances,
        }
    }

    #[inline]
    fn transform_size(&self) -> u64 {
        size_of::<Transform>() as u64 * self.total_max_mesh_instances
    }

    #[inline]
    fn indirect_size(&self) -> u64 {
        size_of::<DrawIndexedCommand>() as u64 * self.num_primitives as u64
    }

    #[inline]
    fn uniform_indirect_buffer_frame_size(&self) -> u64 {
        ((Self::UNIFORM_SIZE + self.indirect_size() - 1) / self.align + 1) * self.align
    }

    #[inline]
    fn transform_buffer_frame_size(&self) -> u64 {
        ((self.transform_size() - 1) / self.align + 1) * self.align
    }

    #[inline]
    fn uniform_offset(&self, index: u64) -> u64 {
        self.uniform_indirect_buffer_frame_size() * index as u64
    }

    #[inline]
    fn transforms_offset(&self, index: u64) -> u64 {
        self.transform_buffer_frame_size() * index as u64
    }

    #[inline]
    fn indirect_offset(&self, index: u64) -> u64 {
        self.uniform_offset(index) + Self::UNIFORM_SIZE
    }

    #[inline]
    fn mesh_transforms_index(&self, mesh_index: usize) -> usize {
        self.max_mesh_instances[0..mesh_index]
            .iter()
            .map(|n| *n as usize)
            .sum::<usize>()
    }

    #[inline]
    fn instance_transform_index(&self, mesh_index: usize, instance: u16) -> usize {
        self.mesh_transforms_index(mesh_index) + instance as usize
    }

    #[inline]
    fn primitive_indirect_offset(&self, prim_index: usize) -> u64 {
        prim_index as u64 * size_of::<DrawIndexedCommand>() as u64
    }
}

impl<B> SimpleGraphicsPipelineDesc<B, specs::World> for PipelineDesc
where
    B: hal::Backend,
{
    type Pipeline = Pipeline<B>;

    fn layout(&self) -> Layout {
        let all_layout = SetLayout {
            bindings: vec![
                hal::pso::DescriptorSetLayoutBinding {
                    binding: 0,
                    ty: hal::pso::DescriptorType::UniformBuffer,
                    count: 1,
                    stage_flags: hal::pso::ShaderStageFlags::GRAPHICS,
                    immutable_samplers: false,
                },
                // Texture maps sampler
                hal::pso::DescriptorSetLayoutBinding {
                    binding: 1,
                    ty: hal::pso::DescriptorType::Sampler,
                    count: 1,
                    stage_flags: hal::pso::ShaderStageFlags::FRAGMENT,
                    immutable_samplers: false,
                },
            ],
        };
        // SampledImage for each texture map, can reuse same sampler
        let mut bindings = Vec::with_capacity(4);
        for i in 0..4 {
            bindings.push(hal::pso::DescriptorSetLayoutBinding {
                binding: i,
                ty: hal::pso::DescriptorType::SampledImage,
                count: 1,
                stage_flags: hal::pso::ShaderStageFlags::FRAGMENT,
                immutable_samplers: false,
            });
        }
        let material_layout = SetLayout { bindings };
        Layout {
            sets: vec![all_layout, material_layout],
            push_constants: Vec::new(),
        }
    }

    fn vertices(
        &self,
    ) -> Vec<(
        Vec<hal::pso::Element<hal::format::Format>>,
        hal::pso::ElemStride,
        hal::pso::InstanceRate,
    )> {
        vec![
            PosNormTangTex::VERTEX.gfx_vertex_input_desc(0),
            Transform::VERTEX.gfx_vertex_input_desc(1),
        ]
    }

    fn load_shader_set<'a>(
        &self,
        storage: &'a mut Vec<B::ShaderModule>,
        factory: &mut Factory<B>,
        _aux: &specs::World,
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

    fn build<'a>(
        self,
        _ctx: &GraphContext<B>,
        factory: &mut Factory<B>,
        _queue: QueueId,
        world: &specs::World,
        buffers: Vec<NodeBuffer>,
        images: Vec<NodeImage>,
        set_layouts: &[Handle<DescriptorSetLayout<B>>],
    ) -> Result<Pipeline<B>, failure::Error> {
        assert!(buffers.is_empty());
        assert!(images.is_empty());
        assert_eq!(set_layouts.len(), 2);

        let aux = world.read_resource::<Aux>();
        let frames = aux.frames;
        let material_storage = world.read_resource::<asset::MaterialStorage<B>>();

        let num_mats = material_storage.0.len();
        let mut descriptor_pool = unsafe {
            factory.create_descriptor_pool(
                frames + num_mats,
                vec![
                    hal::pso::DescriptorRangeDesc {
                        ty: hal::pso::DescriptorType::UniformBuffer,
                        count: frames,
                    },
                    hal::pso::DescriptorRangeDesc {
                        ty: hal::pso::DescriptorType::Sampler,
                        count: frames,
                    },
                    hal::pso::DescriptorRangeDesc {
                        ty: hal::pso::DescriptorType::SampledImage,
                        count: num_mats * 4,
                    },
                ],
            )?
        };

        let settings = Settings::from_world::<B>(world);

        let uniform_indirect_buffer = factory.create_buffer(
            BufferInfo {
                size: settings.uniform_indirect_buffer_frame_size() * frames as u64,
                usage: hal::buffer::Usage::UNIFORM | hal::buffer::Usage::INDIRECT,
            },
            MemoryUsageValue::Dynamic,
        )?;
        let transform_buffer = factory.create_buffer(
            BufferInfo {
                size: settings.transform_buffer_frame_size() * frames as u64,
                usage: hal::buffer::Usage::VERTEX,
            },
            MemoryUsageValue::Dynamic,
        )?;

        let texture_sampler =
            factory.create_sampler(SamplerInfo::new(Filter::Linear, WrapMode::Clamp))?;

        let mut frame_sets = Vec::with_capacity(frames);
        for index in 0..frames {
            unsafe {
                let set = descriptor_pool.allocate_set(&set_layouts[0].raw())?;
                factory.write_descriptor_sets(vec![
                    hal::pso::DescriptorSetWrite {
                        set: &set,
                        binding: 0,
                        array_offset: 0,
                        descriptors: Some(hal::pso::Descriptor::Buffer(
                            uniform_indirect_buffer.raw(),
                            Some(settings.uniform_offset(index as u64))
                                ..Some(
                                    settings.uniform_offset(index as u64) + Settings::UNIFORM_SIZE,
                                ),
                        )),
                    },
                    hal::pso::DescriptorSetWrite {
                        set: &set,
                        binding: 1,
                        array_offset: 0,
                        descriptors: Some(hal::pso::Descriptor::Sampler(texture_sampler.raw())),
                    },
                ]);
                frame_sets.push(set);
            }
        }

        let mut mat_sets = Vec::new();

        for mat_data in material_storage.0.iter() {
            unsafe {
                let set = descriptor_pool.allocate_set(&set_layouts[1].raw())?;
                factory.write_descriptor_sets(vec![
                    hal::pso::DescriptorSetWrite {
                        set: &set,
                        binding: 0,
                        array_offset: 0,
                        descriptors: Some(hal::pso::Descriptor::Image(
                            mat_data.albedo.view().raw(),
                            hal::image::Layout::ShaderReadOnlyOptimal,
                        )),
                    },
                    hal::pso::DescriptorSetWrite {
                        set: &set,
                        binding: 1,
                        array_offset: 0,
                        descriptors: Some(hal::pso::Descriptor::Image(
                            mat_data.normal.view().raw(),
                            hal::image::Layout::ShaderReadOnlyOptimal,
                        )),
                    },
                    hal::pso::DescriptorSetWrite {
                        set: &set,
                        binding: 2,
                        array_offset: 0,
                        descriptors: Some(hal::pso::Descriptor::Image(
                            mat_data.metallic_roughness.view().raw(),
                            hal::image::Layout::ShaderReadOnlyOptimal,
                        )),
                    },
                    hal::pso::DescriptorSetWrite {
                        set: &set,
                        binding: 3,
                        array_offset: 0,
                        descriptors: Some(hal::pso::Descriptor::Image(
                            mat_data.ao.view().raw(),
                            hal::image::Layout::ShaderReadOnlyOptimal,
                        )),
                    },
                ]);
                mat_sets.push(set);
            }
        }

        Ok(Pipeline {
            descriptor_pool,
            uniform_indirect_buffer,
            transform_buffer,
            texture_sampler,
            frame_sets,
            mat_sets,
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
        if self.settings != Settings::from_world::<B>(world) {
            unimplemented!();
        }

        use rendy::memory::Write;
        use specs::{prelude::*, storage::UnprotectedStorage};

        let lights = world.read_storage::<components::Light>();
        let transforms = world.read_storage::<components::GlobalTransform>();

        let mut n_lights = 0;
        let mut lights_data = [Default::default(); crate::MAX_LIGHTS];
        for (light, transform) in (&lights, &transforms).join() {
            if n_lights >= crate::MAX_LIGHTS {
                break;
            }

            lights_data[n_lights] = super::LightData {
                pos: nalgebra::Point3::from(transform.0.column(3).xyz()),
                color: light.color,
                intensity: light.intensity,
                _pad: 0f32,
            };

            n_lights += 1;
        }
        let cameras = world.read_storage::<components::Camera>();
        let active_cameras = world.read_storage::<components::ActiveCamera>();
        let camera_args: CameraArgs = (&active_cameras, &cameras, &transforms)
            .join()
            .map(|(_, cam, trans)| (cam, trans).into())
            .next()
            .expect("No active camera!");
        unsafe {
            factory
                .upload_visible_buffer(
                    &mut self.uniform_indirect_buffer,
                    self.settings.uniform_offset(index as u64),
                    &[UniformArgs {
                        camera: camera_args,
                        num_lights: n_lights as i32,
                        lights: lights_data,
                    }],
                )
                .unwrap()
        };

        let instance_cache = world.read_resource::<systems::InstanceCache>();
        // log::debug!("cache: {:?}", *instance_cache);
        let mesh_storage = world.read_resource::<asset::MeshStorage>();
        let primitive_storage = world.read_resource::<asset::PrimitiveStorage<B>>();

        let indirect_offset = self.settings.indirect_offset(index as u64);
        let indirect_size = self.settings.indirect_size();
        let indirect_end = indirect_offset + indirect_size;
        {
            let mut indirects_mapped = self
                .uniform_indirect_buffer
                .map(factory.device(), indirect_offset..indirect_end)
                .unwrap();
            let mut indirects_writer = unsafe {
                indirects_mapped
                    .write(factory.device(), 0..indirect_size)
                    .unwrap()
            };
            let indirects_slice = unsafe { indirects_writer.slice() };

            for dirty_mesh in instance_cache.dirty_mesh_indirects[index].iter() {
                for prim_index in mesh_storage.0[*dirty_mesh].primitives.iter() {
                    let command = DrawIndexedCommand {
                        index_count: primitive_storage.0[*prim_index].mesh_data.len(),
                        instance_count: instance_cache.mesh_instance_counts[*dirty_mesh],
                        first_index: 0,
                        vertex_offset: 0,
                        first_instance: 0,
                    };

                    indirects_slice[*prim_index] = command;
                }
            }
        }

        let mesh_instance_storage = world.read_resource::<systems::MeshInstanceStorage>();
        let entities = world.entities();

        let transforms_offset = self.settings.transforms_offset(index as u64);
        let transforms_size = self.settings.transform_size();
        let transforms_end = transforms_offset + self.settings.transform_size();
        {
            let mut transforms_mapped = self
                .transform_buffer
                .map(factory.device(), transforms_offset..transforms_end)
                .unwrap();
            let mut transforms_writer = unsafe {
                transforms_mapped
                    .write(factory.device(), 0..transforms_size)
                    .unwrap()
            };
            let transforms_slice = unsafe { transforms_writer.slice() };

            for (entity, transform, _) in (
                &entities,
                &transforms,
                &instance_cache.dirty_entities[index],
            )
                .join()
            {
                let systems::MeshInstance { mesh, instance } =
                    unsafe { mesh_instance_storage.0.get(entity.id()) };
                let idx = self.settings.instance_transform_index(*mesh, *instance);
                transforms_slice[idx] = transform.0;
            }
        }

        PrepareResult::DrawRecord
    }

    fn draw(
        &mut self,
        layout: &B::PipelineLayout,
        mut encoder: RenderPassEncoder<'_, B>,
        index: usize,
        world: &specs::World,
    ) {
        let primitive_storage = world.read_resource::<asset::PrimitiveStorage<B>>();
        encoder.bind_graphics_descriptor_sets(
            layout,
            0,
            Some(&self.frame_sets[index]),
            std::iter::empty(),
        );
        let transforms_offset = self.settings.transforms_offset(index as u64);
        let indirect_offset = self.settings.indirect_offset(index as u64);
        for (mat_idx, set) in self.mat_sets.iter().enumerate() {
            encoder.bind_graphics_descriptor_sets(layout, 1, Some(set), std::iter::empty());
            for (prim_idx, primitive) in primitive_storage
                .0
                .iter()
                .enumerate()
                .filter(|(_, primitive)| primitive.mat == mat_idx)
            {
                assert!(primitive
                    .mesh_data
                    .bind(&[PosNormTangTex::VERTEX], &mut encoder)
                    .is_ok());
                encoder.bind_vertex_buffers(
                    1,
                    std::iter::once((
                        self.transform_buffer.raw(),
                        transforms_offset
                            + self.settings.mesh_transforms_index(primitive.mesh_handle) as u64
                                * size_of::<Transform>() as u64,
                    )),
                );
                encoder.draw_indexed_indirect(
                    self.uniform_indirect_buffer.raw(),
                    indirect_offset + self.settings.primitive_indirect_offset(prim_idx),
                    1,
                    size_of::<DrawIndexedCommand>() as u32,
                );
            }
        }
    }

    fn dispose(mut self, factory: &mut Factory<B>, _world: &specs::World) {
        unsafe {
            self.descriptor_pool.reset();
            factory.destroy_descriptor_pool(self.descriptor_pool);
        }
    }
}
