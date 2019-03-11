use rendy::{
    mesh::{Mesh, PosNormTangTex},
    factory::{Factory, ImageState},
    command::QueueId,
    texture::{pixel::{Rgba8Srgb, Rgba8Unorm}, Texture, TextureBuilder},
};
use gfx_hal as hal;
use derivative::Derivative;

use std::{
    collections::{HashMap, hash_map::{DefaultHasher, Entry}},
    hash::{Hash, Hasher},
    fs::File,
    path::Path,
    io::Read,
};

use crate::scene::Object;
use crate::Backend;

#[derive(Clone, Copy, Default)]
#[repr(C, align(16))]
pub struct Factors {
    pub albedo: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
}

#[derive(Derivative)]
#[derivative(Eq, PartialEq)]
pub struct Material<B: hal::Backend> {
    #[derivative(PartialEq="ignore")]
    pub factors: Factors,
    #[derivative(PartialEq="ignore")]
    pub albedo: Texture<B>,
    #[derivative(PartialEq="ignore")]
    pub normal: Texture<B>,
    #[derivative(PartialEq="ignore")]
    pub metallic_roughness: Texture<B>,
    #[derivative(PartialEq="ignore")]
    pub ao: Texture<B>,
    pub hash: u64,
}
pub struct GltfBuffers(pub Vec<Vec<u8>>);

impl GltfBuffers {
    pub fn load_from_gltf<P: AsRef<Path>>(base_path: P, gltf: &gltf::Gltf) -> Self {
        use gltf::buffer::Source;
        let mut buffers = vec![];
        for (_index, buffer) in gltf.buffers().enumerate() {
            let data = match buffer.source() {
                Source::Uri(uri) => {
                    if uri.starts_with("data:") {
                        unimplemented!();
                    } else {
                        let mut file = File::open(base_path.as_ref().join(uri)).unwrap();
                        let mut data: Vec<u8> = Vec::with_capacity(file.metadata().unwrap().len() as usize);
                        file.read_to_end(&mut data).expect("Failed to read gltf binary data");
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
    #[allow(unused)]
    pub fn view(&self, view: &gltf::buffer::View<'_>) -> Option<&[u8]> {
        self.buffer(&view.buffer()).map(|data| {
            let begin = view.offset();
            let end = begin + view.length();
            &data[begin..end]
        })
    }
}

pub fn object_from_gltf<P: AsRef<Path>, B: hal::Backend>(
    mesh: &gltf::Mesh<'_>,
    base_dir: P,
    buffers: &GltfBuffers,
    material_storage: &mut HashMap<u64, Material<B>>,
    factory: &mut Factory<B>,
    queue: QueueId,
) -> Object<B> {
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
    let tangents = reader
        .read_tangents()
        .unwrap()
        .map(|t| [t[0], t[1], t[2]]);
    let uvs = reader.read_tex_coords(0).unwrap().into_f32();

    let vertices = positions
        .zip(normals.zip(tangents.zip(uvs)))
        .map(|(pos, (norm, (tang, uv)))|
            PosNormTangTex {
                position: pos.into(),
                normal: norm.into(),
                tangent: tang.into(),
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
        let factors = Factors {
            albedo: pbr_met_rough.base_color_factor(),
            metallic: pbr_met_rough.metallic_factor(),
            roughness: pbr_met_rough.roughness_factor(),
        };

        let albedo = load_gltf_texture(
            factory,
            queue,
            &base_dir,
            pbr_met_rough.base_color_texture().unwrap().texture(),
            true,
        );

        let metallic_roughness = load_gltf_texture(
            factory,
            queue,
            &base_dir,
            pbr_met_rough.metallic_roughness_texture().unwrap().texture(),
            false,
        );

        let normal = load_gltf_texture(
            factory,
            queue,
            &base_dir,
            material.normal_texture().unwrap().texture(),
            false,
        );

        let ao = load_gltf_texture(
            factory,
            queue,
            &base_dir,
            material.occlusion_texture().unwrap().texture(),
            false,
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

fn gltf_texture_uri(texture: gltf::Texture<'_>) -> String {
    if let gltf::image::Source::Uri { uri, .. } = texture.source().source() {
        String::from(uri)
    } else {
        unimplemented!();
    }
}

fn load_gltf_texture<B, P>(factory: &mut Factory<B>, queue: QueueId, base_dir: P, texture: gltf::Texture<'_>, srgb: bool)
    -> Texture<B>
    where B: hal::Backend,
        P: AsRef<Path>
{
    match texture.source().source() {
        gltf::image::Source::View { .. } => unimplemented!(),
        gltf::image::Source::Uri { uri, .. } => {
            load_texture_from_file(factory, queue, base_dir.as_ref().join(uri), srgb)
        }
    }
}

fn load_texture_from_file<P, B>(factory: &mut Factory<B>, queue: QueueId, path: P, srgb: bool)
    -> Texture<B>
    where B: hal::Backend,
        P: AsRef<Path>,
{
    let mut file = File::open(&path).unwrap();
    let mut tex_bytes: Vec<u8> = Vec::with_capacity(file.metadata().unwrap().len() as usize);
    file.read_to_end(&mut tex_bytes).expect(&format!("Failed to read texture data for: {:?}", path.as_ref().to_str()));

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
                ImageState {
                    queue,
                    stage: hal::pso::PipelineStage::FRAGMENT_SHADER,
                    access: hal::image::Access::SHADER_READ,
                    layout: hal::image::Layout::ShaderReadOnlyOptimal,
                },
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
                ImageState {
                    queue,
                    stage: hal::pso::PipelineStage::FRAGMENT_SHADER,
                    access: hal::image::Access::SHADER_READ,
                    layout: hal::image::Layout::ShaderReadOnlyOptimal,
                },
                factory,
            )
            .unwrap()
    }
}
