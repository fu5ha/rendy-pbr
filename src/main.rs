#![cfg_attr(
    not(any(feature = "dx12", feature = "metal", feature = "vulkan")),
    allow(unused)
)]

use rendy::{
    command::{Graphics, Supports},
    factory::{Config, Factory, ImageState},
    graph::{present::PresentNode, render::*, GraphBuilder},
    memory::MemoryUsageValue,
    resource::image::TextureUsage,
};

use std::{fs::File, path::Path, time};

use rendy::hal;

use specs::prelude::*;

use winit::{Event, EventsLoop, WindowBuilder, WindowEvent};

mod asset;
mod components;
mod input;
mod node;
mod systems;

pub const CUBEMAP_RES: u32 = 512;
pub const MAX_LIGHTS: usize = 32;

#[cfg(feature = "dx12")]
pub type Backend = rendy::dx12::Backend;

#[cfg(feature = "metal")]
pub type Backend = rendy::metal::Backend;

#[cfg(feature = "vulkan")]
pub type Backend = rendy::vulkan::Backend;

#[cfg(feature = "empty")]
pub type Backend = rendy::empty::Backend;

pub fn generate_instances(size: (u8, u8, u8)) -> Vec<nalgebra::Similarity3<f32>> {
    let x_size = 3.0;
    let y_size = 4.0;
    let z_size = 4.0;
    let mut instances = Vec::with_capacity(size.0 as usize * size.1 as usize * size.2 as usize);
    for x in 0..size.0 {
        for y in 0..size.1 {
            for z in 0..size.2 {
                instances.push(nalgebra::Similarity3::from_parts(
                    nalgebra::Translation3::new(
                        (x as f32 * x_size) - (x_size * (size.0 - 1) as f32 * 0.5),
                        (y as f32 * y_size) - (y_size * (size.1 - 1) as f32 * 0.5),
                        (z as f32 * z_size) - (z_size * (size.2 - 1) as f32 * 0.5),
                    ),
                    nalgebra::UnitQuaternion::identity(),
                    1.0,
                ));
            }
        }
    }
    instances
}

#[cfg(any(feature = "dx12", feature = "metal", feature = "vulkan"))]
fn main() -> Result<(), failure::Error> {
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Warn)
        .filter_module("rendy_pbr", log::LevelFilter::Trace)
        .init();

    let config: Config = Default::default();

    let (mut factory, mut families): (Factory<Backend>, _) = rendy::factory::init(config)?;

    let align = hal::adapter::PhysicalDevice::limits(factory.physical())
        .min_uniform_buffer_offset_alignment;

    let queue = families
        .as_slice()
        .iter()
        .find(|family| {
            if let Some(Graphics) = family.capability().supports() {
                true
            } else {
                false
            }
        })
        .unwrap()
        .as_slice()[0]
        .id();

    // // Preprocess steps to load environment map, convert it to a cubemap,
    // // and filter it for use later
    // let mut env_preprocess_graph_builder =
    //     GraphBuilder::<Backend, node::env_preprocess::Aux<Backend>>::new();

    // let mut equirect_to_faces =
    //     node::env_preprocess::equirectangular_to_cube_faces::Pipeline::<Backend>::builder()
    //         .into_subpass();

    // let cube_face_images = hal::image::CUBE_FACES
    //     .into_iter()
    //     .map(|_| {
    //         env_preprocess_graph_builder.create_image(
    //             hal::image::Kind::D2(CUBEMAP_RES, CUBEMAP_RES, 1, 1),
    //             1,
    //             hal::format::Format::Rgba32Float,
    //             MemoryUsageValue::Data,
    //             Some(hal::command::ClearValue::Color([0.0, 0.0, 0.0, 1.0].into())),
    //         )
    //     })
    //     .collect::<Vec<_>>();

    // for image in cube_face_images.iter().cloned() {
    //     equirect_to_faces.add_color(image);
    // }

    // let equirect_to_faces_pass =
    //     env_preprocess_graph_builder.add_node(equirect_to_faces.into_pass());

    // let _faces_to_cubemap_pass = env_preprocess_graph_builder.add_node(
    //     node::env_preprocess::faces_to_cubemap::FacesToCubemap::<Backend>::builder(
    //         cube_face_images,
    //     )
    //     .with_dependency(equirect_to_faces_pass),
    // );

    // let equirect_file = std::fs::File::open(concat!(
    //     env!("CARGO_MANIFEST_DIR"),
    //     "/assets/environment/abandoned_hall_01_4k.hdr"
    // ))?;

    // let equirect_tex = rendy::texture::image::load_from_image(
    //     std::io::BufReader::new(equirect_file),
    //     Default::default(),
    //     TextureUsage,
    //     factory.physical(),
    // )?
    // .build(
    //     ImageState {
    //         queue,
    //         stage: hal::pso::PipelineStage::FRAGMENT_SHADER,
    //         access: hal::image::Access::SHADER_READ,
    //         layout: hal::image::Layout::ShaderReadOnlyOptimal,
    //     },
    //     &mut factory,
    //     TextureUsage,
    // )?;

    // let cubemap_tex = rendy::texture::TextureBuilder::new()
    //     .with_kind(rendy::resource::image::Kind::D2(
    //         CUBEMAP_RES,
    //         CUBEMAP_RES,
    //         6,
    //         1,
    //     ))
    //     .with_view_kind(rendy::resource::image::ViewKind::Cube)
    //     .with_raw_format(hal::format::Format::Rgba32Float)
    //     .build(
    //         ImageState {
    //             queue,
    //             stage: hal::pso::PipelineStage::TRANSFER,
    //             access: hal::image::Access::TRANSFER_WRITE,
    //             layout: hal::image::Layout::TransferDstOptimal,
    //         },
    //         &mut factory,
    //         TextureUsage,
    //     )?;

    // let mut env_preprocess_aux = node::env_preprocess::Aux {
    //     align,
    //     equirectangular_texture: equirect_tex,
    //     environment_cubemap: cubemap_tex,
    // };

    // let mut env_preprocess_graph =
    //     env_preprocess_graph_builder.build(&mut factory, &mut families, &mut env_preprocess_aux)?;

    // factory.maintain(&mut families);
    // env_preprocess_graph.run(&mut factory, &mut families, &mut env_preprocess_aux);
    // env_preprocess_graph.dispose(&mut factory, &mut env_preprocess_aux);

    // Main window and render graph building
    let mut event_loop = EventsLoop::new();

    let window = WindowBuilder::new()
        .with_title("rendy-pbr")
        .with_dimensions(winit::dpi::LogicalSize::new(1280.0, 960.0))
        .build(&event_loop)?;

    let input = input::InputState::new(window.get_inner_size().unwrap());

    event_loop.poll_events(|_| ());

    let surface = factory.create_surface(window.into());
    let aspect = surface.aspect();

    let mut pbr_graph_builder = GraphBuilder::<Backend, specs::World>::new();

    let hdr = pbr_graph_builder.create_image(
        surface.kind(),
        1,
        hal::format::Format::Rgba32Float,
        MemoryUsageValue::Data,
        Some(hal::command::ClearValue::Color([0.1, 0.3, 0.4, 1.0].into())),
    );

    let color = pbr_graph_builder.create_image(
        surface.kind(),
        1,
        factory.get_surface_format(&surface),
        MemoryUsageValue::Data,
        Some(hal::command::ClearValue::Color([0.1, 0.3, 0.4, 1.0].into())),
    );

    let depth = pbr_graph_builder.create_image(
        surface.kind(),
        1,
        hal::format::Format::D16Unorm,
        MemoryUsageValue::Data,
        Some(hal::command::ClearValue::DepthStencil(
            hal::command::ClearDepthStencil(1.0, 0),
        )),
    );

    let mesh_pass = pbr_graph_builder.add_node(
        node::pbr::mesh::Pipeline::builder()
            .into_subpass()
            .with_color(hdr)
            .with_depth_stencil(depth)
            .into_pass(),
    );

    let tonemap_pass = pbr_graph_builder.add_node(
        node::pbr::tonemap::Pipeline::builder()
            .with_image(hdr)
            .into_subpass()
            .with_dependency(mesh_pass)
            .with_color(color)
            .into_pass(),
    );

    let present_builder =
        PresentNode::builder(&factory, surface, color).with_dependency(tonemap_pass);

    let frames = present_builder.image_count() as usize;

    pbr_graph_builder.add_node(present_builder);

    let mut world = specs::World::new();
    world.register::<components::Transform>();
    world.register::<components::Mesh>();
    world.register::<components::Camera>();
    world.register::<components::Light>();
    world.register::<systems::MeshInstance>();
    world.add_resource(input);

    let instance_array_size = (1, 1, 1);

    let (material_storage, primitive_storage, mesh_storage) = {
        let base_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/gltf/helmet/");
        let file = File::open(base_path.join("SciFiHelmet.gltf"))?;
        let reader = std::io::BufReader::new(file);
        let gltf = gltf::Gltf::from_reader(reader)?;

        let gltf_buffers = asset::GltfBuffers::load_from_gltf(&base_path, &gltf)?;
        let mut material_storage = Vec::with_capacity(gltf.meshes().len());
        for _ in 0..gltf.meshes().len() {
            material_storage.push(None);
        }
        let mut primitive_storage = Vec::new();
        let mut mesh_storage = Vec::with_capacity(gltf.materials().len());
        for _ in 0..gltf.materials().len() {
            mesh_storage.push(None);
        }

        let scene = gltf.scenes().next().unwrap();

        for node in scene.nodes() {
            if let Some(mesh) = node.mesh() {
                use gltf::scene::Transform;
                let node_transform = match node.transform() {
                    Transform::Matrix { .. } => unimplemented!(),
                    Transform::Decomposed {
                        translation,
                        rotation,
                        scale,
                    } => nalgebra::Similarity3::from_parts(
                        nalgebra::Translation3::new(translation[0], translation[1], translation[2]),
                        nalgebra::UnitQuaternion::from_quaternion(nalgebra::Quaternion::new(
                            rotation[0],
                            rotation[1],
                            rotation[2],
                            rotation[3],
                        )),
                        scale.iter().sum::<f32>() / 3.0,
                    ),
                };

                let (transforms, max_instances) = match node.name() {
                    Some("SciFiHelmet") => {
                        let transforms = generate_instances(instance_array_size)
                            .into_iter()
                            .map(|t| node_transform * t)
                            .collect::<Vec<_>>();
                        (transforms, 1024)
                    }
                    _ => (vec![node_transform], 1),
                };

                let mesh_handle = asset::load_gltf_mesh(
                    &mesh,
                    max_instances,
                    &base_path,
                    &gltf_buffers,
                    &mut material_storage,
                    &mut primitive_storage,
                    &mut mesh_storage,
                    &mut factory,
                    queue,
                )?;

                for transform in transforms {
                    world
                        .create_entity()
                        .with(components::Mesh(mesh_handle))
                        .with(components::Transform(transform))
                        .build();
                }
            }
        }
        let material_storage = asset::MaterialStorage(
            material_storage
                .into_iter()
                .map(|mut m| m.take().unwrap())
                .collect::<Vec<_>>(),
        );
        let primitive_storage = asset::PrimitiveStorage(
            primitive_storage
                .into_iter()
                .map(|mut p| p.take().unwrap())
                .collect::<Vec<_>>(),
        );
        let mesh_storage = asset::MeshStorage(
            mesh_storage
                .into_iter()
                .map(|mut m| m.take().unwrap())
                .collect::<Vec<_>>(),
        );
        (material_storage, primitive_storage, mesh_storage)
    };

    let num_meshes = mesh_storage.0.len();
    let num_materials = material_storage.0.len();
    world.add_resource(material_storage);
    world.add_resource(primitive_storage);
    world.add_resource(mesh_storage);

    let pbr_aux = node::pbr::Aux {
        frames,
        align,
        instance_array_size,
        tonemapper_args: node::pbr::tonemap::TonemapperArgs {
            exposure: 2.5,
            curve: 0,
            comparison_factor: 0.5,
        },
    };

    world.add_resource(pbr_aux);

    world.add_resource(systems::InstanceCache {
        dirty_entities: specs::BitSet::new(),
        dirty_mesh_indirects: Vec::new(),
        mesh_instance_counts: vec![0; num_meshes],
        material_bitsets: vec![specs::BitSet::new(); num_materials],
    });

    let camera = components::Camera {
        yaw: 0.0,
        pitch: 0.0,
        dist: 10.0,
        focus: nalgebra::Point3::new(0.0, 0.0, 0.0),
        proj: nalgebra::Perspective3::new(aspect, 3.1415 / 6.0, 1.0, 200.0),
    };

    world
        .create_entity()
        .with(camera)
        .with(components::Transform(nalgebra::Similarity3::from_parts(
            nalgebra::Translation3::new(0.0, 0.0, 10.0),
            nalgebra::UnitQuaternion::identity(),
            1.0,
        )))
        .build();
    let light_pos_intensities = vec![
        (nalgebra::Vector3::new(10.0, 10.0, 2.0), 150.0),
        (nalgebra::Vector3::new(8.0, 10.0, 2.0), 150.0),
        (nalgebra::Vector3::new(8.0, 10.0, 4.0), 150.0),
        (nalgebra::Vector3::new(10.0, 10.0, 4.0), 150.0),
        (nalgebra::Vector3::new(-4.0, 0.0, -5.0), 250.0),
        (nalgebra::Vector3::new(-5.0, 5.0, -2.0), 25.0),
    ];

    for (pos, intensity) in light_pos_intensities.into_iter() {
        world
            .create_entity()
            .with(components::Light {
                intensity,
                color: [1.0, 1.0, 1.0],
            })
            .with(components::Transform(
                nalgebra::Similarity3::identity() * nalgebra::Translation3::from(pos),
            ))
            .build();
    }

    let mut pbr_graph = pbr_graph_builder.build(&mut factory, &mut families, &mut world)?;

    let camera_transform_system = {
        let mut transforms = world.write_storage::<components::Transform>();
        (systems::CameraTransformSystem {
            reader_id: transforms.register_reader(),
            dirty: BitSet::new(),
        })
    };

    let pbr_aux_input_system = systems::PbrAuxInputSystem {
        helmet_mesh: 0 as asset::MeshHandle,
    };

    let instance_cache_update_system = systems::InstanceCacheUpdateSystem {
        transform_reader_id: world
            .write_storage::<components::Transform>()
            .register_reader(),
        mesh_reader_id: world.write_storage::<components::Mesh>().register_reader(),
        mesh_inserted: BitSet::new(),
        mesh_deleted: BitSet::new(),
        mesh_entity_bitsets: vec![BitSet::new(); num_meshes],
        _pd: core::marker::PhantomData::<Backend>,
    };

    let mut dispatcher = DispatcherBuilder::new()
        .with(systems::CameraInputSystem, "camera_input_system", &[])
        .with(
            camera_transform_system,
            "camera_transform_system",
            &["camera_input_system"],
        )
        .with(pbr_aux_input_system, "pbr_aux_input_system", &[])
        .with(
            instance_cache_update_system,
            "instance_cache_update_system",
            &["pbr_aux_input_system"],
        )
        .with(
            systems::InputSystem,
            "input_system",
            &["pbr_aux_input_system", "camera_input_system"],
        )
        .build();

    let started = time::Instant::now();

    let mut frames = 0u64..;

    let mut checkpoint = started;
    let mut should_close = false;

    while !should_close {
        let start = frames.start;
        for _ in &mut frames {
            factory.maintain(&mut families);
            {
                let mut event_bucket = world.write_resource::<input::EventBucket>();
                event_bucket.0.clear();

                event_loop.poll_events(|event| match event {
                    Event::WindowEvent {
                        event: WindowEvent::CloseRequested,
                        ..
                    } => should_close = true,
                    _ => {
                        event_bucket.0.push(event);
                    }
                });
            }

            dispatcher.dispatch(&mut world.res);

            pbr_graph.run(&mut factory, &mut families, &mut world);

            let elapsed = checkpoint.elapsed();

            if should_close || elapsed > std::time::Duration::new(2, 0) {
                let frames = frames.start - start;
                let nanos = elapsed.as_secs() * 1_000_000_000 + elapsed.subsec_nanos() as u64;
                log::info!("FPS: {}", frames * 1_000_000_000 / nanos);
                log::info!(
                    "Tonemapper Settings: {}",
                    world.read_resource::<node::pbr::Aux>().tonemapper_args
                );
                checkpoint += elapsed;
                break;
            }
        }
    }

    pbr_graph.dispose(&mut factory, &mut world);
    Ok(())
}

#[cfg(not(any(feature = "dx12", feature = "metal", feature = "vulkan")))]
fn main() -> Result<(), failure::Error> {
    panic!("Specify feature: { dx12, metal, vulkan }");
    Ok(())
}
