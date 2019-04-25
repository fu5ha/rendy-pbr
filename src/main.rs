#![cfg_attr(
    not(any(feature = "dx12", feature = "metal", feature = "vulkan")),
    allow(unused)
)]

use rendy::{
    command::{Graphics, Supports},
    factory::{Config, Factory, ImageState},
    graph::{present::PresentNode, render::*, GraphBuilder},
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

pub const ENV_CUBEMAP_RES: u32 = 1024;
pub const IRRADIANCE_CUBEMAP_RES: u32 = 64;
pub const SPEC_CUBEMAP_RES: u32 = 256;
pub const SPEC_CUBEMAP_MIP_LEVELS: u8 = 5;
pub const SPEC_BRDF_MAP_RES: u32 = 512;
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

    let scene_config = scene::SceneConfig::from_path("assets/scene.ron")?;

    let (mut factory, mut families): (Factory<Backend>, _) = rendy::factory::init(config)?;

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

    // Preprocess steps to load environment map, convert it to a cubemap,
    // and filter it for use later
    let mut env_preprocess_graph_builder =
        GraphBuilder::<Backend, node::env_preprocess::Aux<Backend>>::new();

    let mut equirect_to_faces =
        node::env_preprocess::equirectangular_to_cube_faces::Pipeline::<Backend>::builder()
            .into_subpass();

    let env_cube_face_images = hal::image::CUBE_FACES
        .into_iter()
        .map(|_| {
            env_preprocess_graph_builder.create_image(
                hal::image::Kind::D2(ENV_CUBEMAP_RES, ENV_CUBEMAP_RES, 1, 1),
                1,
                hal::format::Format::Rgba32Float,
                Some(hal::command::ClearValue::Color([0.0, 0.0, 0.0, 1.0].into())),
            )
        })
        .collect::<Vec<_>>();

    for image in env_cube_face_images.iter().cloned() {
        equirect_to_faces.add_color(image);
    }

    let equirect_to_faces_pass =
        env_preprocess_graph_builder.add_node(equirect_to_faces.into_pass());

    let faces_to_env_pass = env_preprocess_graph_builder.add_node(
        node::env_preprocess::faces_to_cubemap::FacesToCubemap::<Backend>::builder(
            env_cube_face_images.clone(),
            "environment",
            1,
        )
        .with_dependency(equirect_to_faces_pass),
    );

    // ENV TO IRRADIANCE

    let mut env_to_irradiance_faces_subpass =
        node::env_preprocess::env_to_irradiance::Pipeline::<Backend>::builder()
            .with_dependency(faces_to_env_pass)
            .into_subpass();

    let irradiance_cube_face_images = hal::image::CUBE_FACES
        .into_iter()
        .map(|_| {
            env_preprocess_graph_builder.create_image(
                hal::image::Kind::D2(IRRADIANCE_CUBEMAP_RES, IRRADIANCE_CUBEMAP_RES, 1, 1),
                1,
                hal::format::Format::Rgba32Float,
                Some(hal::command::ClearValue::Color([0.0, 0.0, 0.0, 1.0].into())),
            )
        })
        .collect::<Vec<_>>();

    for image in irradiance_cube_face_images.iter().cloned() {
        env_to_irradiance_faces_subpass.add_color(image);
    }

    let env_to_irradiance_faces_pass =
        env_preprocess_graph_builder.add_node(env_to_irradiance_faces_subpass.into_pass());

    let _irradiance_to_cube_pass = env_preprocess_graph_builder.add_node(
        node::env_preprocess::faces_to_cubemap::FacesToCubemap::<Backend>::builder(
            irradiance_cube_face_images.clone(),
            "irradiance",
            1,
        )
        .with_dependency(env_to_irradiance_faces_pass),
    );

    // ENV TO SPECULAR

    let mut env_to_spec_faces_subpasses = Vec::new();
    let mut spec_cube_face_images = Vec::new();

    for mip_level in 0..SPEC_CUBEMAP_MIP_LEVELS {
        let res = SPEC_CUBEMAP_RES / 2u32.pow(mip_level as u32);
        let mut subpass = node::env_preprocess::env_to_specular::Pipeline::<Backend>::builder()
            .with_dependency(faces_to_env_pass)
            .into_subpass();
        for _ in hal::image::CUBE_FACES.into_iter() {
            let image = env_preprocess_graph_builder.create_image(
                hal::image::Kind::D2(res, res, 1, 1),
                1,
                hal::format::Format::Rgba32Float,
                Some(hal::command::ClearValue::Color([0.0, 0.0, 0.0, 1.0].into())),
            );
            subpass.add_color(image);
            spec_cube_face_images.push(image);
        }
        env_to_spec_faces_subpasses.push(subpass);
    }

    let mut env_to_spec_faces_passes = Vec::new();
    while !env_to_spec_faces_subpasses.is_empty() {
        env_to_spec_faces_passes.push(
            env_preprocess_graph_builder
                .add_node(env_to_spec_faces_subpasses.pop().unwrap().into_pass()),
        );
    }

    let mut builder = node::env_preprocess::faces_to_cubemap::FacesToCubemap::<Backend>::builder(
        spec_cube_face_images.clone(),
        "specular",
        SPEC_CUBEMAP_MIP_LEVELS,
    );

    for pass in env_to_spec_faces_passes {
        builder.add_dependency(pass);
    }

    let _spec_to_cube_pass = env_preprocess_graph_builder.add_node(builder);

    let spec_brdf_map = env_preprocess_graph_builder.create_image(
        hal::image::Kind::D2(SPEC_BRDF_MAP_RES, SPEC_BRDF_MAP_RES, 1, 1),
        1,
        hal::format::Format::Rg32Float,
        Some(hal::command::ClearValue::Color([0.0, 0.0].into())),
    );

    let brdf_integration_pass = env_preprocess_graph_builder.add_node(
        node::env_preprocess::integrate_spec_brdf::Pipeline::builder()
            .into_subpass()
            .with_color(spec_brdf_map)
            .into_pass(),
    );

    let _brdf_to_texture = env_preprocess_graph_builder.add_node(
        node::env_preprocess::copy_to_texture::CopyToTexture::<Backend>::builder(
            spec_brdf_map,
            "spec_brdf",
        )
        .with_dependency(brdf_integration_pass),
    );

    // let dbg_color = env_preprocess_graph_builder.create_image(
    //     surface.kind(),
    //     1,
    //     factory.get_surface_format(&surface),
    //     Some(hal::command::ClearValue::Color([0.0, 0.0, 0.0, 1.0].into())),
    // );

    // let dbg_pass = env_preprocess_graph_builder.add_node(
    //     node::env_preprocess::debug::Pipeline::<Backend>::builder()
    //         .with_dependency(_spec_to_cube_pass)
    //         .with_dependency(_irradiance_to_cube_pass)
    //         .into_subpass()
    //         .with_color(dbg_color)
    //         .into_pass(),
    // );

    // env_preprocess_graph_builder.add_node(
    //     PresentNode::builder(&factory, surface, spec_brdf_map).with_dependency(_brdf_to_texture),
    // );

    let equirect_file = std::fs::File::open(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(scene_config.environment_map.clone()),
    )?;

    let equirect_tex = rendy::texture::image::load_from_image(
        std::io::BufReader::new(equirect_file),
        rendy::texture::image::ImageTextureConfig {
            repr: rendy::texture::image::Repr::Float,
            ..Default::default()
        },
    )?
    .build(
        ImageState {
            queue,
            stage: hal::pso::PipelineStage::FRAGMENT_SHADER,
            access: hal::image::Access::SHADER_READ,
            layout: hal::image::Layout::ShaderReadOnlyOptimal,
        },
        &mut factory,
    )?;

    let env_cubemap_tex = rendy::texture::TextureBuilder::new()
        .with_kind(rendy::resource::Kind::D2(
            ENV_CUBEMAP_RES,
            ENV_CUBEMAP_RES,
            6,
            1,
        ))
        .with_view_kind(rendy::resource::ViewKind::Cube)
        .with_data_width(ENV_CUBEMAP_RES)
        .with_data_height(ENV_CUBEMAP_RES)
        .with_data(vec![
            rendy::texture::pixel::Rgba32Float {
                repr: [0.0, 0.0, 0.0, 1.0]
            };
            (ENV_CUBEMAP_RES * ENV_CUBEMAP_RES * 6) as usize
        ])
        .build(
            ImageState {
                queue,
                stage: hal::pso::PipelineStage::TRANSFER,
                access: hal::image::Access::TRANSFER_WRITE,
                layout: hal::image::Layout::TransferDstOptimal,
            },
            &mut factory,
        )?;

    let irradiance_cubemap_tex = rendy::texture::TextureBuilder::new()
        .with_kind(rendy::resource::Kind::D2(
            IRRADIANCE_CUBEMAP_RES,
            IRRADIANCE_CUBEMAP_RES,
            6,
            1,
        ))
        .with_view_kind(rendy::resource::ViewKind::Cube)
        .with_data_width(IRRADIANCE_CUBEMAP_RES)
        .with_data_height(IRRADIANCE_CUBEMAP_RES)
        .with_data(vec![
            rendy::texture::pixel::Rgba32Float {
                repr: [0.0, 0.0, 0.0, 1.0]
            };
            (IRRADIANCE_CUBEMAP_RES * IRRADIANCE_CUBEMAP_RES * 6)
                as usize
        ])
        .build(
            ImageState {
                queue,
                stage: hal::pso::PipelineStage::TRANSFER,
                access: hal::image::Access::TRANSFER_WRITE,
                layout: hal::image::Layout::TransferDstOptimal,
            },
            &mut factory,
        )?;

    let spec_cubemap_tex = rendy::texture::TextureBuilder::new()
        .with_kind(rendy::resource::Kind::D2(
            SPEC_CUBEMAP_RES,
            SPEC_CUBEMAP_RES,
            6,
            1,
        ))
        .with_mip_levels(SPEC_CUBEMAP_MIP_LEVELS)
        .with_view_kind(rendy::resource::ViewKind::Cube)
        .with_data_width(SPEC_CUBEMAP_RES)
        .with_data_height(SPEC_CUBEMAP_RES)
        .with_data(vec![
            rendy::texture::pixel::Rgba32Float {
                repr: [0.0, 0.0, 0.0, 1.0]
            };
            (SPEC_CUBEMAP_RES * SPEC_CUBEMAP_RES * 6) as usize
        ])
        .build(
            ImageState {
                queue,
                stage: hal::pso::PipelineStage::TRANSFER,
                access: hal::image::Access::TRANSFER_WRITE,
                layout: hal::image::Layout::TransferDstOptimal,
            },
            &mut factory,
        )?;

    let spec_brdf_tex = rendy::texture::TextureBuilder::new()
        .with_kind(rendy::resource::Kind::D2(
            SPEC_BRDF_MAP_RES,
            SPEC_BRDF_MAP_RES,
            1,
            1,
        ))
        .with_view_kind(rendy::resource::ViewKind::D2)
        .with_data_width(SPEC_BRDF_MAP_RES)
        .with_data_height(SPEC_BRDF_MAP_RES)
        .with_data(vec![
            rendy::texture::pixel::Rg32Float { repr: [0.0, 0.0] };
            (SPEC_BRDF_MAP_RES * SPEC_BRDF_MAP_RES) as usize
        ])
        .build(
            ImageState {
                queue,
                stage: hal::pso::PipelineStage::TRANSFER,
                access: hal::image::Access::TRANSFER_WRITE,
                layout: hal::image::Layout::TransferDstOptimal,
            },
            &mut factory,
        )?;

    let mut env_preprocess_aux = node::env_preprocess::Aux {
        align,
        equirectangular_texture: equirect_tex,
        environment_cubemap: Some(env_cubemap_tex),
        irradiance_cubemap: Some(irradiance_cubemap_tex),
        spec_cubemap: Some(spec_cubemap_tex),
        spec_brdf_map: Some(spec_brdf_tex),
        queue,
        mip_level: std::sync::atomic::AtomicUsize::new(0),
    };

    let mut env_preprocess_graph =
        env_preprocess_graph_builder.build(&mut factory, &mut families, &mut env_preprocess_aux)?;

    factory.maintain(&mut families);
    env_preprocess_graph.run(&mut factory, &mut families, &mut env_preprocess_aux);
    env_preprocess_graph.dispose(&mut factory, &mut env_preprocess_aux);

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
        node::pbr::environment_map::Pipeline::builder()
            .into_subpass()
            .with_group(node::pbr::mesh::Pipeline::builder())
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
    let (material_storage, primitive_storage, mesh_storage, _scene_entities) =
        scene_config.load(aspect, &mut factory, queue, &mut world)?;

    let num_meshes = mesh_storage.0.len();
    let num_materials = material_storage.0.len();

    let pbr_aux = node::pbr::Aux {
        frames: FRAMES_IN_FLIGHT as _,
        align,
        tonemapper_args: node::pbr::tonemap::TonemapperArgs {
            exposure: 1.7,
            curve: 0,
            comparison_factor: 0.5,
        },
        cube_display: node::pbr::environment_map::CubeDisplay::Environment,
        cube_roughness: 1.0,
    };

    // Add specs resources
    world.add_resource(pbr_aux);
    world.add_resource(input);
    world.add_resource(event_bucket);
    world.add_resource(material_storage);
    world.add_resource(primitive_storage);
    world.add_resource(mesh_storage);
    world.add_resource(node::pbr::EnvironmentStorage {
        env_cube: env_preprocess_aux.environment_cubemap.take(),
        irradiance_cube: env_preprocess_aux.irradiance_cubemap.take(),
        spec_cube: env_preprocess_aux.spec_cubemap.take(),
        spec_brdf_map: env_preprocess_aux.spec_brdf_map.take(),
    });
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
