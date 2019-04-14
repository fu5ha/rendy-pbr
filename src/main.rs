#![cfg_attr(
    not(any(feature = "dx12", feature = "metal", feature = "vulkan")),
    allow(unused)
)]

use rendy::{
    command::{Graphics, Supports},
    factory::{Config, Factory, ImageState},
    graph::{present::PresentNode, render::*, GraphBuilder},
    memory::MemoryUsageValue,
};

use std::{collections::HashSet, time};

use rendy::hal;

use specs::prelude::*;

use winit::{Event, EventsLoop, WindowBuilder, WindowEvent};

mod asset;
mod components;
mod input;
mod node;
mod scene;
mod systems;
mod transform;

pub const CUBEMAP_RES: u32 = 512;
pub const MAX_LIGHTS: usize = 32;
pub const FRAMES_IN_FLIGHT: u32 = 3;

#[cfg(feature = "dx12")]
pub type Backend = rendy::dx12::Backend;

#[cfg(feature = "metal")]
pub type Backend = rendy::metal::Backend;

#[cfg(feature = "vulkan")]
pub type Backend = rendy::vulkan::Backend;

#[cfg(feature = "empty")]
pub type Backend = rendy::empty::Backend;

#[cfg(any(feature = "dx12", feature = "metal", feature = "vulkan"))]
fn main() {
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Warn)
        .filter_module("rendy_pbr", log::LevelFilter::Trace)
        .init();

    match run() {
        Err(e) => {
            if let Some(name) = e.name() {
                log::error!("Exit with {}: {}\n\nBACKTRACE:\n{}", name, e, e.backtrace());
            } else {
                log::error!(
                    "Exit with Unnamed Error: {}\n\nBACKTRACE: {}",
                    e,
                    e.backtrace()
                );
            }
        }
        _ => (),
    }
}

#[cfg(any(feature = "dx12", feature = "metal", feature = "vulkan"))]
fn run() -> Result<(), failure::Error> {
    // Initialize specs and register components
    let mut world = specs::World::new();

    world.register::<components::Transform>();
    world.register::<components::GlobalTransform>();
    world.register::<components::Parent>();
    world.register::<components::Mesh>();
    world.register::<components::Camera>();
    world.register::<components::ActiveCamera>();
    world.register::<components::Light>();

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
    let event_bucket = input::EventBucket(Vec::new());

    event_loop.poll_events(|_| ());

    let surface = factory.create_surface(window.into());
    let aspect = surface.aspect();

    let mut pbr_graph_builder = GraphBuilder::<Backend, specs::World>::new();

    let hdr = pbr_graph_builder.create_image(
        surface.kind(),
        1,
        hal::format::Format::Rgba32Float,
        Some(hal::command::ClearValue::Color([0.1, 0.3, 0.4, 1.0].into())),
    );

    let color = pbr_graph_builder.create_image(
        surface.kind(),
        1,
        factory.get_surface_format(&surface),
        Some(hal::command::ClearValue::Color([0.1, 0.3, 0.4, 1.0].into())),
    );

    let depth = pbr_graph_builder.create_image(
        surface.kind(),
        1,
        hal::format::Format::D16Unorm,
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

    pbr_graph_builder
        .add_node(PresentNode::builder(&factory, surface, color).with_dependency(tonemap_pass));

    // Hierarchy system must be added before loading scene
    let mut hierarchy_system = specs_hierarchy::HierarchySystem::<components::Parent>::new();
    specs::System::setup(&mut hierarchy_system, &mut world.res);
    let mut transform_system = systems::TransformSystem::new();
    specs::System::setup(&mut transform_system, &mut world.res);

    // Load scene from config file
    let scene_config = scene::SceneConfig::from_path("assets/scene.ron")?;
    let (material_storage, primitive_storage, mesh_storage, _scene_entities) =
        scene_config.load(aspect, &mut factory, queue, &mut world)?;

    let num_meshes = mesh_storage.0.len();
    let num_materials = material_storage.0.len();

    let pbr_aux = node::pbr::Aux {
        frames: FRAMES_IN_FLIGHT as _,
        align,
        tonemapper_args: node::pbr::tonemap::TonemapperArgs {
            exposure: 2.5,
            curve: 0,
            comparison_factor: 0.5,
        },
    };

    // Add specs resources
    world.add_resource(pbr_aux);
    world.add_resource(input);
    world.add_resource(event_bucket);
    world.add_resource(material_storage);
    world.add_resource(primitive_storage);
    world.add_resource(mesh_storage);
    world.add_resource(systems::HelmetArraySize { x: 0, y: 0, z: 0 });
    world.add_resource(systems::HelmetArrayEntities(Vec::new()));
    world.add_resource(systems::MeshInstanceStorage(Default::default()));
    world.add_resource(systems::InstanceCache {
        dirty_entities: vec![specs::BitSet::new(); FRAMES_IN_FLIGHT as _],
        dirty_mesh_indirects: vec![HashSet::new(); FRAMES_IN_FLIGHT as _],
        mesh_instance_counts: vec![0; num_meshes],
        material_bitsets: vec![specs::BitSet::new(); num_materials],
    });

    let instance_cache_update_system = {
        let mut mesh_storage = world.write_storage::<components::Mesh>();

        systems::InstanceCacheUpdateSystem {
            frames_in_flight: FRAMES_IN_FLIGHT as usize,
            previous_frame: FRAMES_IN_FLIGHT as usize - 1,
            transform_reader_id: world
                .write_storage::<components::GlobalTransform>()
                .register_reader(),
            mesh_reader_id: mesh_storage.register_reader(),
            dirty_entities_scratch: specs::BitSet::new(),
            dirty_mesh_indirects_scratch: HashSet::new(),
            mesh_inserted: mesh_storage.mask().clone(),
            mesh_deleted: BitSet::new(),
            mesh_modified: BitSet::new(),
            mesh_entity_bitsets: vec![BitSet::new(); num_meshes],
            _pd: core::marker::PhantomData::<Backend>,
        }
    };

    let mut dispatcher = DispatcherBuilder::new()
        .with(systems::CameraInputSystem, "camera_input_system", &[])
        .with(
            systems::PbrAuxInputSystem {
                helmet_mesh: 0 as asset::MeshHandle,
            },
            "pbr_aux_input_system",
            &[],
        )
        .with(
            systems::HelmetArraySizeUpdateSystem {
                curr_size: Default::default(),
                helmet_mesh: 0 as asset::MeshHandle,
            },
            "helmet_array_size_update_system",
            &["pbr_aux_input_system"],
        )
        .with(
            hierarchy_system,
            "transform_hierarchy_system",
            &[
                "helmet_array_size_update_system",
                "pbr_aux_input_system",
                "camera_input_system",
            ],
        )
        .with(
            transform_system,
            "transform_system",
            &["transform_hierarchy_system"],
        )
        .with(
            instance_cache_update_system,
            "instance_cache_update_system",
            &["transform_system"],
        )
        .with(
            systems::InputSystem,
            "input_system",
            &["pbr_aux_input_system", "camera_input_system"],
        )
        .build();

    let mut pbr_graph = pbr_graph_builder
        .with_frames_in_flight(FRAMES_IN_FLIGHT)
        .build(&mut factory, &mut families, &mut world)?;

    let started = time::Instant::now();

    let mut frames = 0u64..;

    let mut checkpoint = started;
    let mut should_close = false;

    while !should_close {
        let start = frames.start;
        for _ in &mut frames {
            factory.maintain(&mut families);
            world.maintain();
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
