#![cfg_attr(
    not(any(feature = "dx12", feature = "metal", feature = "vulkan")),
    allow(unused)
)]

use rendy::{
    command::{DrawIndexedCommand, QueueId, RenderPassEncoder, Supports, Graphics},
    factory::{Config, Factory},
    graph::{present::PresentNode, render::*, GraphBuilder, NodeBuffer, NodeImage},
    hal::{pso::DescriptorPool, Device},
    memory::MemoryUsageValue,
    mesh::{AsVertex, Mesh, PosNormTex, Transform},
    resource::{buffer::Buffer, image::{Filter, WrapMode}, sampler::Sampler},
    shader::{Shader, ShaderKind, SourceLanguage, StaticShaderInfo},
    texture::{pixel::{Rgba8Srgb, Rgba8Unorm}, Texture, TextureBuilder},
};

use std::{
    collections::{HashMap, hash_map::{Entry, DefaultHasher}},
    hash::{Hash, Hasher},
    fs::File,
    io::Read,
    mem::size_of,
    path::Path,
    time,
};

use derivative::Derivative;

use gfx_hal as hal;

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

const MAX_LIGHTS: usize = 32;

#[derive(Clone, Copy)]
#[repr(C, align(16))]
struct CameraArgs {
    proj: nalgebra::Matrix4<f32>,
    view: nalgebra::Matrix4<f32>,
    camera_pos: nalgebra::Vector3<f32>,
    _pad: f32,
}

impl From<Camera> for CameraArgs {
    fn from(cam: Camera) -> Self {
        CameraArgs {
            proj: {
                let mut proj = cam.proj.to_homogeneous();
                proj[(1, 1)] *= -1.0;
                proj
            },
            view: cam.view.inverse().to_homogeneous(),
            camera_pos: cam.view.translation.vector,
            _pad: 0.0,
        }   
    }
}

#[derive(Clone, Copy)]
struct Light {
    pos: nalgebra::Vector3<f32>,
    _pad: f32,
    color: [f32; 3],
    _pad1: f32,
    intensity: f32,
}

#[derive(Clone, Copy)]
#[repr(C, align(16))]
struct MeshUniformArgs {
    camera: CameraArgs,
    num_lights: i32,
    _pad: [i32; 3],
    lights: [Light; MAX_LIGHTS]
}

#[derive(Clone, Copy)]
#[repr(C, align(16))]
struct DepthUniformArgs {
    camera: CameraArgs,
}

#[derive(Debug, Clone, Copy)]
struct Camera {
    view: nalgebra::Isometry3<f32>,
    proj: nalgebra::Perspective3<f32>,
}

struct GltfBuffers(Vec<Vec<u8>>);

impl GltfBuffers {
    pub fn load_from_gltf<P: AsRef<Path>>(base_path: P, gltf: &gltf::Gltf) -> Self {
        use gltf::buffer::Source;
        let mut buffers = vec![];
        for (index, buffer) in gltf.buffers().enumerate() {
            let data = match buffer.source() {
                Source::Uri(uri) => {
                    if uri.starts_with("data:") {
                        unimplemented!();
                    } else {
                        let mut file = File::open(base_path.as_ref().join(uri)).unwrap();
                        let mut data: Vec<u8> = Vec::with_capacity(file.metadata().unwrap().len() as usize);
                        file.read_to_end(&mut data);
                        data
                    }
                },
                Source::Bin => unimplemented!(),
            };

            assert!(data.len() >= buffer.length());
            buffers.push(data);
        }
        GltfBuffers(buffers)
    }
    /// Obtain the contents of a loaded buffer.
    pub fn buffer(&self, buffer: &gltf::Buffer<'_>) -> Option<&[u8]> {
        self.0.get(buffer.index()).map(Vec::as_slice)
    }

    /// Obtain the contents of a loaded buffer view.
    pub fn view(&self, view: &gltf::buffer::View<'_>) -> Option<&[u8]> {
        self.buffer(&view.buffer()).map(|data| {
            let begin = view.offset();
            let end = begin + view.length();
            &data[begin..end]
        })
    }

    /// Take the loaded buffer data.
    pub fn take(self) -> Vec<Vec<u8>> {
        self.0
    }
}

struct Object<B: hal::Backend> {
    mesh: Mesh<B>,
    material: u64,
}

impl<B: hal::Backend> Object<B> {
    fn load_from_gltf<P: AsRef<Path>>(
        mesh: &gltf::Mesh<'_>,
        base_dir: P,
        buffers: &GltfBuffers,
        material_storage: &mut HashMap<u64, Material<B>>,
        factory: &mut Factory<B>,
        queue: QueueId,
    ) -> Self {
        if mesh.primitives().len() != 1 {
            unimplemented!();
        }

        let primitive = mesh.primitives().next().unwrap();
        let reader = primitive.reader(|buf_id| buffers.buffer(&buf_id));


        let indices = reader.read_indices()
            .unwrap()
            .into_u32()
            .collect::<Vec<u32>>();

        let positions = reader.read_positions().unwrap();
        let normals = reader.read_normals().unwrap();
        let uvs = reader.read_tex_coords(0).unwrap().into_f32();

        let vertices = positions
            .zip(normals.zip(uvs))
            .map(|(pos, (norm, uv))|
                PosNormTex {
                    position: pos.into(),
                    normal: norm.into(),
                    tex_coord: uv.into(),
                })
            .collect::<Vec<_>>();
        
        let mesh = Mesh::<Backend>::builder()
            .with_indices(&indices[..])
            .with_vertices(&vertices[..])
            .build(queue, factory)
            .unwrap();
        
        let material = primitive.material();

        let pbr_met_rough = material.pbr_metallic_roughness();

        let mut hasher = DefaultHasher::new();
        gltf_texture_uri(pbr_met_rough.base_color_texture().unwrap().texture()).hash(&mut hasher);
        gltf_texture_uri(pbr_met_rough.metallic_roughness_texture().unwrap().texture()).hash(&mut hasher);
        gltf_texture_uri(material.normal_texture().unwrap().texture()).hash(&mut hasher);
        gltf_texture_uri(material.occlusion_texture().unwrap().texture()).hash(&mut hasher);

        let hash = hasher.finish();

        if let Entry::Vacant(e) = material_storage.entry(hash) {
            let mut factors = Factors {
                albedo: pbr_met_rough.base_color_factor(),
                metallic: pbr_met_rough.metallic_factor(),
                roughness: pbr_met_rough.roughness_factor(),
            };

            let albedo = load_gltf_texture(
                factory,
                queue,
                &base_dir,
                pbr_met_rough.base_color_texture().unwrap().texture()
            );

            let metallic_roughness = load_gltf_texture(
                factory,
                queue,
                &base_dir,
                pbr_met_rough.metallic_roughness_texture().unwrap().texture()
            );

            let normal = load_gltf_texture(
                factory,
                queue,
                &base_dir,
                material.normal_texture().unwrap().texture()
            );

            let ao = load_gltf_texture(
                factory,
                queue,
                &base_dir,
                material.occlusion_texture().unwrap().texture()
            );

            e.insert(Material{
                factors,
                albedo,
                metallic_roughness,
                normal,
                ao,
                hash,
            });
        }

        Object {
            mesh,
            material: hash,
        }
    }
}

fn gltf_texture_uri(texture: gltf::Texture<'_>) -> String {
    if let gltf::image::Source::Uri {uri, mime_type} = texture.source().source() {
        String::from(uri)
    } else {
        unimplemented!();
    }
}

fn load_gltf_texture<B, P>(factory: &mut Factory<B>, queue: QueueId, base_dir: P, texture: gltf::Texture<'_>)
    -> Texture<B>
    where B: hal::Backend,
        P: AsRef<Path>
{
    match texture.source().source() {
        gltf::image::Source::View {view, mime_type} => unimplemented!(),
        gltf::image::Source::Uri {uri, mime_type} => {
            load_texture_from_file(factory, queue, base_dir.as_ref().join(uri), true)
        }
    }
}

fn load_texture_from_file<P, B>(factory: &mut Factory<B>, queue: QueueId, path: P, srgb: bool)
    -> Texture<B>
    where B: hal::Backend,
        P: AsRef<Path>,
{
    let mut file = File::open(path).unwrap();
    let mut tex_bytes: Vec<u8> = Vec::with_capacity(file.metadata().unwrap().len() as usize);
    file.read_to_end(&mut tex_bytes);

    let tex_img = image::load_from_memory(&tex_bytes[..])
        .unwrap()
        .to_rgba();

    let (w, h) = tex_img.dimensions();


    if srgb {
        let tex_img_data = tex_img
            .pixels()
            .map(|p| Rgba8Srgb { repr: p.data })
            .collect::<Vec<_>>();

        TextureBuilder::new()
            .with_kind(hal::image::Kind::D2(w, h, 1, 1))
            .with_view_kind(hal::image::ViewKind::D2)
            .with_data_width(w)
            .with_data_height(h)
            .with_data(&tex_img_data)
            .build(
                queue,
                hal::image::Access::SHADER_READ,
                hal::image::Layout::ShaderReadOnlyOptimal,
                factory,
            )
            .unwrap()
    } else {
        let tex_img_data = tex_img
            .pixels()
            .map(|p| Rgba8Unorm { repr: p.data })
            .collect::<Vec<_>>();

        TextureBuilder::new()
            .with_kind(hal::image::Kind::D2(w, h, 1, 1))
            .with_view_kind(hal::image::ViewKind::D2)
            .with_data_width(w)
            .with_data_height(h)
            .with_data(&tex_img_data)
            .build(
                queue,
                hal::image::Access::SHADER_READ,
                hal::image::Layout::ShaderReadOnlyOptimal,
                factory,
            )
            .unwrap()
    }
}

#[derive(Clone, Copy, Default)]
#[repr(C, align(16))]
struct Factors {
    albedo: [f32; 4],
    metallic: f32,
    roughness: f32,
}

#[derive(Derivative)]
#[derivative(Eq, PartialEq)]
struct Material<B: hal::Backend> {
    #[derivative(PartialEq="ignore")]
    factors: Factors,
    #[derivative(PartialEq="ignore")]
    albedo: Texture<B>,
    #[derivative(PartialEq="ignore")]
    normal: Texture<B>,
    #[derivative(PartialEq="ignore")]
    metallic_roughness: Texture<B>,
    #[derivative(PartialEq="ignore")]
    ao: Texture<B>,
    hash: u64,
}

struct Environment<B: hal::Backend> {
    mesh: Mesh<B>,
    hdr: Texture<B>,
    irradiance: Texture<B>,
    spec_filtered: Texture<B>,
    bdrf: Texture<B>,
}

struct Scene<B: hal::Backend> {
    camera: Camera,
    objects: Vec<(Object<B>, Vec<nalgebra::Matrix4<f32>>)>,
    max_obj_instances: Vec<usize>,
    lights: Vec<Light>,
    // environment: Environment<B>,
}

struct Aux<B: hal::Backend> {
    frames: usize,
    align: u64,
    scene: Scene<B>,
    material_storage: HashMap<u64, Material<B>>,
}

#[derive(Debug, Default)]
struct MeshRenderPipelineDesc;

#[derive(Debug)]
struct MeshRenderPipeline<B: hal::Backend> {
    descriptor_pool: B::DescriptorPool,
    buffer: Buffer<B>,
    texture_sampler: Sampler<B>,
    frame_sets: Vec<B::DescriptorSet>,
    mat_sets: HashMap<u64, B::DescriptorSet>,
    settings: MeshRenderPipelineSettings,
}

#[derive(Debug, PartialEq, Eq)]
struct MeshRenderPipelineSettings {
    align: u64,
    max_obj_instances: Vec<usize>,
    total_max_obj_instances: u64,
}

impl<B: hal::Backend> From<&Aux<B>> for MeshRenderPipelineSettings {
    fn from(aux: &Aux<B>) -> Self {
        Self::from_aux(aux)
    }
}

impl<B: hal::Backend> From<&mut Aux<B>> for MeshRenderPipelineSettings {
    fn from(aux: &mut Aux<B>) -> Self {
        Self::from_aux(aux)
    }
}

impl MeshRenderPipelineSettings {
    const UNIFORM_SIZE: u64 = size_of::<MeshUniformArgs>() as u64;

    fn from_aux<B: hal::Backend>(aux: &Aux<B>) -> Self {
        MeshRenderPipelineSettings {
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
        ((Self::UNIFORM_SIZE
            + self.transform_size()
            + self.indirect_size()
            - 1) / self.align + 1) * self.align
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
        self.max_obj_instances[0..obj_index].iter().map(|n| *n as u64).sum::<u64>() * size_of::<Transform>() as u64
    }

    #[inline]
    fn obj_indirect_offset(&self, obj_index: usize) -> u64 {
        obj_index as u64 * size_of::<DrawIndexedCommand>() as u64
    }
}

impl<B> SimpleGraphicsPipelineDesc<B, Aux<B>> for MeshRenderPipelineDesc
where
    B: hal::Backend,
{
    type Pipeline = MeshRenderPipeline<B>;

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
            ]
        };
        // SampledImage for each texture map, can reuse same sampler
        let mut bindings = Vec::with_capacity(4);
        for i in 0..4 {
            bindings.push(
                hal::pso::DescriptorSetLayoutBinding {
                    binding: i,
                    ty: hal::pso::DescriptorType::SampledImage,
                    count: 1,
                    stage_flags: hal::pso::ShaderStageFlags::FRAGMENT,
                    immutable_samplers: false,
                }
            );
        }
        let material_layout = SetLayout {
            bindings,
        };
        Layout {
            sets: vec![
                all_layout,
                material_layout,
            ],
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
            PosNormTex::VERTEX.gfx_vertex_input_desc(0),
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
        queue: QueueId,
        aux: &mut Aux<B>,
        buffers: Vec<NodeBuffer<'a, B>>,
        images: Vec<NodeImage<'a, B>>,
        set_layouts: &[B::DescriptorSetLayout],
    ) -> Result<MeshRenderPipeline<B>, failure::Error> {
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
            )
        }
        .unwrap();

        let settings = MeshRenderPipelineSettings::from_aux(aux);

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
            )
            .unwrap();

        let texture_sampler = factory.create_sampler(Filter::Linear, WrapMode::Clamp).unwrap();

        let mut frame_sets = Vec::with_capacity(frames);
        for index in 0..frames {
            unsafe {
                let set = descriptor_pool.allocate_set(&set_layouts[0]).unwrap();
                factory.write_descriptor_sets(vec![
                    hal::pso::DescriptorSetWrite {
                        set: &set,
                        binding: 0,
                        array_offset: 0,
                        descriptors: Some(hal::pso::Descriptor::Buffer(
                            buffer.raw(),
                            Some(settings.uniform_offset(index as u64))..Some(settings.uniform_offset(index as u64) + MeshRenderPipelineSettings::UNIFORM_SIZE),
                        )),
                    },
                    hal::pso::DescriptorSetWrite {
                        set: &set,
                        binding: 1,
                        array_offset: 0,
                        descriptors: Some(hal::pso::Descriptor::Sampler(
                            texture_sampler.raw(),
                        )),
                    },
                ]);
                frame_sets.push(set);
            }
        }

        let mut mat_sets = HashMap::new();

        for (mat_hash, material) in aux.material_storage.iter() {
            unsafe {
                let set = descriptor_pool.allocate_set(&set_layouts[1]).unwrap();
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

        Ok(MeshRenderPipeline {
            descriptor_pool,
            buffer,
            texture_sampler,
            frame_sets,
            mat_sets,
            settings,
        })
    }
}

impl<B> SimpleGraphicsPipeline<B, Aux<B>> for MeshRenderPipeline<B>
where
    B: hal::Backend,
{
    type Desc = MeshRenderPipelineDesc;

    fn prepare(
        &mut self,
        factory: &Factory<B>,
        _queue: QueueId,
        _set_layouts: &[B::DescriptorSetLayout],
        index: usize,
        aux: &Aux<B>,
    ) -> PrepareResult {
        debug_assert!(aux.scene.lights.len() <= MAX_LIGHTS);
        if (self.settings != aux.into()) {
            unimplemented!();
        }

        let mut lights: [Light; MAX_LIGHTS] = [aux.scene.lights[0]; MAX_LIGHTS];
        for (i, l) in aux.scene.lights.iter().enumerate() {
            lights[i] = *l;
        }
        unsafe {
            factory
                .upload_visible_buffer(
                    &mut self.buffer,
                    self.settings.uniform_offset(index as u64),
                    &[MeshUniformArgs {
                        camera: aux.scene.camera.into(),
                        num_lights: aux.scene.lights.len() as i32,
                        _pad: [0; 3],
                        lights,
                    }],
                )
                .unwrap()
        };

        let cmds = aux.scene.objects.iter()
            .map(|(o, instances)| {
                DrawIndexedCommand {
                    index_count: o.mesh.len(),
                    instance_count: instances.len() as u32,
                    first_index: 0,
                    vertex_offset: 0,
                    first_instance: 0,
                }
            })
            .collect::<Vec<_>>();

        unsafe {
            factory
                .upload_visible_buffer(
                    &mut self.buffer,
                    self.settings.indirect_offset(index as u64),
                    &cmds,
                )
                .unwrap()
        };

        let transforms_offset = self.settings.transforms_offset(index as u64);
        aux.scene.objects.iter()
            .enumerate()
            .for_each(|(i, (obj, instances))| {
                unsafe {
                    factory
                        .upload_visible_buffer(
                            &mut self.buffer,
                            transforms_offset + self.settings.obj_transforms_offset(i),
                            &instances[..],
                        )
                        .unwrap()
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
            encoder.bind_graphics_descriptor_sets(
                layout,
                1,
                Some(set),
                std::iter::empty(),
            );
            aux.scene.objects.iter().enumerate()
                .filter(|(_, (o, _))| o.material == *mat_hash)
                .for_each(|(i, (obj, instances))|{
                    assert!(obj.mesh
                        .bind(&[PosNormTex::VERTEX], &mut encoder)
                        .is_ok());
                    encoder.bind_vertex_buffers(
                        1,
                        std::iter::once((self.buffer.raw(), transforms_offset + self.settings.obj_transforms_offset(i))),
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
            self.descriptor_pool
                .free_sets(self.frame_sets.into_iter());
            self.descriptor_pool
                .free_sets(self.mat_sets.into_iter().map(|(_, set)| set));
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

    let queue = families.as_slice()
        .iter()
        .find(|family| if let Some(Graphics) = family.capability().supports() {
            true
        } else {
            false
        })
        .unwrap()
        .as_slice()[0]
        .id();
    let mut event_loop = EventsLoop::new();

    let window = WindowBuilder::new()
        .with_title("Rendy example")
        .build(&event_loop)
        .unwrap();

    event_loop.poll_events(|_| ());

    let surface = factory.create_surface(window.into());
    let aspect = surface.aspect();

    let mut graph_builder = GraphBuilder::<Backend, Aux<Backend>>::new();

    let color = graph_builder.create_image(
        surface.kind(),
        1,
        factory.get_surface_format(&surface),
        MemoryUsageValue::Data,
        Some(hal::command::ClearValue::Color(
            [0.1, 0.3, 0.4, 1.0].into(),
        )),
    );

    let depth = graph_builder.create_image(
        surface.kind(),
        1,
        hal::format::Format::D16Unorm,
        MemoryUsageValue::Data,
        Some(hal::command::ClearValue::DepthStencil(
            hal::command::ClearDepthStencil(1.0, 0),
        )),
    );

    let pass = graph_builder.add_node(
        MeshRenderPipeline::builder()
            .into_subpass()
            .with_color(color)
            .with_depth_stencil(depth)
            .into_pass(),
    );

    let present_builder = PresentNode::builder(&factory, surface, color)
        .with_dependency(pass);

    let frames = present_builder.image_count() as usize;

    graph_builder.add_node(present_builder);

    let mut material_storage = HashMap::new();

    let base_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/gltf/helmet/");
    let file = File::open(base_path.join("SciFiHelmet.gltf")).unwrap();
    let reader = std::io::BufReader::new(file);
    let gltf = gltf::Gltf::from_reader(reader).unwrap();

    let gltf_buffers = GltfBuffers::load_from_gltf(&base_path, &gltf);

    let scene = gltf.scenes().next().unwrap();

    let mut camera: Option<Camera> = None;

    let mut helmet: Option<Object<Backend>> = None;


    for node in scene.nodes() {
        match node.name() {
            Some("Camera") => {
                if let gltf::scene::Transform::Decomposed { translation, rotation, scale } = node.transform() {
                    camera = Some(Camera {
                        proj: nalgebra::Perspective3::new(aspect, 3.1415 / 4.0, 1.0, 200.0),
                        view: nalgebra::Isometry3::from_parts(
                            nalgebra::Translation3::new(translation[0], translation[1], translation[2]),
                            nalgebra::UnitQuaternion::from_quaternion(
                                nalgebra::Quaternion::new(rotation[0], rotation[1], rotation[2], rotation[3])
                            )
                        ),
                    })
                }
            },
            Some("SciFiHelmet") => {
                if let Some(mesh) = node.mesh() {
                    helmet = Some(Object::load_from_gltf(
                        &mesh,
                        &base_path,
                        &gltf_buffers,
                        &mut material_storage,
                        &mut factory,
                        queue
                    ));
                }
            },
            _ => (),
        }
    }

    let mut aux = Aux {
        frames,
        align: hal::adapter::PhysicalDevice::limits(factory.physical())
            .min_uniform_buffer_offset_alignment,
        scene: Scene {
            camera: camera.unwrap(),
            max_obj_instances: vec![
                50,
            ],
            objects: vec![
                (helmet.unwrap(), vec![nalgebra::Matrix4::identity()]),
            ],
            lights: vec![
                Light {
                    pos: nalgebra::Vector3::new(10.0, 10.0, -10.0),
                    _pad: 0.0,
                    color: [1.0, 1.0, 1.0],
                    _pad1: 0.0,
                    intensity: 160.0,
                }
            ]
        },
        material_storage,
    };

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

    while !should_close {
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

            // if aux.scene.objects.len() < MAX_OBJECTS {
            //     aux.scene.objects.push({
            //         let z = rz.sample(&mut rng);
            //         nalgebra::Translation3::new(
            //             rxy.sample(&mut rng) * (z / 2.0 + 4.0),
            //             rxy.sample(&mut rng) * (z / 2.0 + 4.0),
            //             -z,
            //         )
            //         .to_homogeneous()
            //     })
            // }

            if should_close || elapsed > std::time::Duration::new(5, 0) {
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