use rendy::{
    command::{DrawIndexedCommand, QueueId, RenderPassEncoder},
    factory::Factory,
    graph::{render::*, NodeBuffer, NodeImage},
    hal::{pso::DescriptorPool, Device},
    memory::MemoryUsageValue,
    mesh::{AsVertex, PosNormTangTex, Transform},
    resource::{
        buffer::Buffer,
        image::{Filter, WrapMode},
        sampler::Sampler,
    },
    shader::{Shader, ShaderKind, SourceLanguage, StaticShaderInfo},
};

use std::{collections::HashMap, mem::size_of};

use gfx_hal as hal;

use crate::{
    node::pbr::{Aux, CameraArgs},
    scene,
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
    lights: [scene::Light; scene::MAX_LIGHTS],
}

#[derive(Debug, Default)]
pub struct PipelineDesc;

#[derive(Debug)]
pub struct Pipeline<B: hal::Backend> {
    descriptor_pool: B::DescriptorPool,
    buffer: Buffer<B>,
    texture_sampler: Sampler<B>,
    frame_sets: Vec<B::DescriptorSet>,
    mat_sets: HashMap<u64, B::DescriptorSet>,
    settings: Settings,
}

#[derive(Debug, PartialEq, Eq)]
struct Settings {
    align: u64,
    max_obj_instances: Vec<usize>,
    total_max_obj_instances: u64,
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
    const UNIFORM_SIZE: u64 = size_of::<UniformArgs>() as u64;

    fn from_aux<B: hal::Backend>(aux: &Aux<B>) -> Self {
        Settings {
            align: aux.align,
            max_obj_instances: aux.scene.max_obj_instances.clone(),
            total_max_obj_instances: aux.scene.max_obj_instances.iter().map(|n| *n as u64).sum(),
        }
    }

    #[inline]
    fn transform_size(&self) -> u64 {
        size_of::<Transform>() as u64 * self.total_max_obj_instances
    }

    #[inline]
    fn indirect_size(&self) -> u64 {
        size_of::<DrawIndexedCommand>() as u64 * self.max_obj_instances.len() as u64
    }

    #[inline]
    fn buffer_frame_size(&self) -> u64 {
        ((Self::UNIFORM_SIZE + self.transform_size() + self.indirect_size() - 1) / self.align + 1)
            * self.align
    }

    #[inline]
    fn uniform_offset(&self, index: u64) -> u64 {
        self.buffer_frame_size() * index as u64
    }

    #[inline]
    fn transforms_offset(&self, index: u64) -> u64 {
        self.uniform_offset(index) + Self::UNIFORM_SIZE
    }

    #[inline]
    fn indirect_offset(&self, index: u64) -> u64 {
        self.transforms_offset(index) + self.transform_size()
    }

    #[inline]
    fn obj_transforms_offset(&self, obj_index: usize) -> u64 {
        self.max_obj_instances[0..obj_index]
            .iter()
            .map(|n| *n as u64)
            .sum::<u64>()
            * size_of::<Transform>() as u64
    }

    #[inline]
    fn obj_indirect_offset(&self, obj_index: usize) -> u64 {
        obj_index as u64 * size_of::<DrawIndexedCommand>() as u64
    }
}

impl<B> SimpleGraphicsPipelineDesc<B, Aux<B>> for PipelineDesc
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
        _aux: &mut Aux<B>,
    ) -> hal::pso::GraphicsShaderSet<'a, B> {
        storage.clear();

        log::trace!("Load shader module '{:#?}'", *VERTEX);
        storage.push(VERTEX.module(factory).unwrap());

        log::trace!("Load shader module '{:#?}'", *FRAGMENT);
        storage.push(FRAGMENT.module(factory).unwrap());

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
        factory: &mut Factory<B>,
        _queue: QueueId,
        aux: &mut Aux<B>,
        buffers: Vec<NodeBuffer<'a, B>>,
        images: Vec<NodeImage<'a, B>>,
        set_layouts: &[B::DescriptorSetLayout],
    ) -> Result<Pipeline<B>, failure::Error> {
        assert!(buffers.is_empty());
        assert!(images.is_empty());
        assert_eq!(set_layouts.len(), 2);

        let frames = aux.frames;

        let num_mats = aux.material_storage.len();
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

        let settings = Settings::from_aux(aux);

        let buffer = factory
            .create_buffer(
                aux.align,
                settings.buffer_frame_size() * frames as u64,
                (
                    hal::buffer::Usage::UNIFORM
                        | hal::buffer::Usage::INDIRECT
                        | hal::buffer::Usage::VERTEX,
                    MemoryUsageValue::Dynamic,
                ),
            )?;

        let texture_sampler = factory
            .create_sampler(Filter::Linear, WrapMode::Clamp)?;

        let mut frame_sets = Vec::with_capacity(frames);
        for index in 0..frames {
            unsafe {
                let set = descriptor_pool.allocate_set(&set_layouts[0])?;
                factory.write_descriptor_sets(vec![
                    hal::pso::DescriptorSetWrite {
                        set: &set,
                        binding: 0,
                        array_offset: 0,
                        descriptors: Some(hal::pso::Descriptor::Buffer(
                            buffer.raw(),
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

        let mut mat_sets = HashMap::new();

        for (mat_hash, material) in aux.material_storage.iter() {
            unsafe {
                let set = descriptor_pool.allocate_set(&set_layouts[1])?;
                factory.write_descriptor_sets(vec![
                    hal::pso::DescriptorSetWrite {
                        set: &set,
                        binding: 0,
                        array_offset: 0,
                        descriptors: Some(hal::pso::Descriptor::Image(
                            material.albedo.image_view.raw(),
                            hal::image::Layout::ShaderReadOnlyOptimal,
                        )),
                    },
                    hal::pso::DescriptorSetWrite {
                        set: &set,
                        binding: 1,
                        array_offset: 0,
                        descriptors: Some(hal::pso::Descriptor::Image(
                            material.normal.image_view.raw(),
                            hal::image::Layout::ShaderReadOnlyOptimal,
                        )),
                    },
                    hal::pso::DescriptorSetWrite {
                        set: &set,
                        binding: 2,
                        array_offset: 0,
                        descriptors: Some(hal::pso::Descriptor::Image(
                            material.metallic_roughness.image_view.raw(),
                            hal::image::Layout::ShaderReadOnlyOptimal,
                        )),
                    },
                    hal::pso::DescriptorSetWrite {
                        set: &set,
                        binding: 3,
                        array_offset: 0,
                        descriptors: Some(hal::pso::Descriptor::Image(
                            material.ao.image_view.raw(),
                            hal::image::Layout::ShaderReadOnlyOptimal,
                        )),
                    },
                ]);
                mat_sets.insert(*mat_hash, set);
            }
        }

        Ok(Pipeline {
            descriptor_pool,
            buffer,
            texture_sampler,
            frame_sets,
            mat_sets,
            settings,
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
        factory: &Factory<B>,
        _queue: QueueId,
        _set_layouts: &[B::DescriptorSetLayout],
        index: usize,
        aux: &Aux<B>,
    ) -> PrepareResult {
        debug_assert!(aux.scene.lights.len() <= scene::MAX_LIGHTS);
        if self.settings != aux.into() {
            unimplemented!();
        }

        let mut lights = [aux.scene.lights[0]; scene::MAX_LIGHTS];
        for (i, l) in aux.scene.lights.iter().enumerate() {
            lights[i] = *l;
        }
        let camera_args: CameraArgs = aux.scene.camera.into();
        unsafe {
            factory
                .upload_visible_buffer(
                    &mut self.buffer,
                    self.settings.uniform_offset(index as u64),
                    &[UniformArgs {
                        camera: camera_args,
                        num_lights: aux.scene.lights.len() as i32,
                        lights,
                    }],
                )?
        };

        let cmds = aux
            .scene
            .objects
            .iter()
            .map(|(o, instances)| DrawIndexedCommand {
                index_count: o.mesh.len(),
                instance_count: instances.len() as u32,
                first_index: 0,
                vertex_offset: 0,
                first_instance: 0,
            })
            .collect::<Vec<_>>();

        unsafe {
            factory
                .upload_visible_buffer(
                    &mut self.buffer,
                    self.settings.indirect_offset(index as u64),
                    &cmds,
                )?
        };

        let transforms_offset = self.settings.transforms_offset(index as u64);
        aux.scene
            .objects
            .iter()
            .enumerate()
            .for_each(|(i, (_obj, instances))| {
                unsafe {
                    factory
                        .upload_visible_buffer(
                            &mut self.buffer,
                            transforms_offset + self.settings.obj_transforms_offset(i),
                            &instances[..],
                        )?
                };
            });

        PrepareResult::DrawReuse
    }

    fn draw(
        &mut self,
        layout: &B::PipelineLayout,
        mut encoder: RenderPassEncoder<'_, B>,
        index: usize,
        aux: &Aux<B>,
    ) {
        encoder.bind_graphics_descriptor_sets(
            layout,
            0,
            Some(&self.frame_sets[index]),
            std::iter::empty(),
        );
        let transforms_offset = self.settings.transforms_offset(index as u64);
        let indirect_offset = self.settings.indirect_offset(index as u64);
        for (mat_hash, set) in self.mat_sets.iter() {
            encoder.bind_graphics_descriptor_sets(layout, 1, Some(set), std::iter::empty());
            aux.scene
                .objects
                .iter()
                .enumerate()
                .filter(|(_, (o, _))| o.material == *mat_hash)
                .for_each(|(i, (obj, _instances))| {
                    assert!(obj
                        .mesh
                        .bind(&[PosNormTangTex::VERTEX], &mut encoder)
                        .is_ok());
                    encoder.bind_vertex_buffers(
                        1,
                        std::iter::once((
                            self.buffer.raw(),
                            transforms_offset + self.settings.obj_transforms_offset(i),
                        )),
                    );
                    encoder.draw_indexed_indirect(
                        self.buffer.raw(),
                        indirect_offset + self.settings.obj_indirect_offset(i),
                        1,
                        size_of::<DrawIndexedCommand> as u32,
                    );
                })
        }
    }

    fn dispose(mut self, factory: &mut Factory<B>, _aux: &mut Aux<B>) {
        unsafe {
            self.descriptor_pool.reset();
            factory.destroy_descriptor_pool(self.descriptor_pool);
        }
    }
}
