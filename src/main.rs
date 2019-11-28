#![cfg_attr(
    not(any(feature = "dx12", feature = "metal", feature = "vulkan")),
    allow(unused)
)]

use rendy::{
    command::{Families, Graphics, Supports},
    factory::{Config, Factory, ImageState},
    graph::{present::PresentNode, render::*, GraphBuilder},
    init::winit::{
        self,
        event::{Event, WindowEvent},
        event_loop::{ControlFlow, EventLoop},
        window::{Window, WindowBuilder},
    },
};

use std::{collections::HashSet, time};

use rendy::hal;

use specs::prelude::*;

mod asset;
mod components;
mod input;
mod node;
mod scene;
mod systems;
mod transform;

pub const ENV_CUBEMAP_RES: u32 = 512;
pub const ENV_CUBEMAP_MIP_LEVELS: u8 = 6;
pub const IRRADIANCE_CUBEMAP_RES: u32 = 64;
pub const SPEC_CUBEMAP_RES: u32 = 128;
pub const SPEC_CUBEMAP_MIP_LEVELS: u8 = 6;
pub const SPEC_BRDF_MAP_RES: u32 = 256;
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
        .filter_module("rendy_pbr", log::LevelFilter::Info)
        .init();

    match err_main() {
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

// Returns the cargo manifest directory when running the executable with cargo
// or the directory in which the executable resides otherwise,
// traversing symlinks if necessary.
pub fn application_root_dir() -> String {
    match std::env::var("CARGO_MANIFEST_DIR") {
        Ok(_) => String::from(env!("CARGO_MANIFEST_DIR")),
        Err(_) => {
            let mut path = std::env::current_exe().expect("Failed to find executable path.");
            while let Ok(target) = std::fs::read_link(path.clone()) {
                path = target;
            }
            String::from(
                path.parent()
                    .expect("Failed to get parent directory of the executable.")
                    .to_str()
                    .unwrap(),
            )
        }
    }
}

#[cfg(any(feature = "dx12", feature = "metal", feature = "vulkan"))]
fn err_main() -> Result<(), failure::Error> {
    #[cfg(feature = "rd")]
    let mut rd: renderdoc::RenderDoc<renderdoc::V120> =
        renderdoc::RenderDoc::new().expect("Failed to init renderdoc");
    #[cfg(feature = "rd")]
    use renderdoc::prelude::*;

    let config: Config = Default::default();

    let event_loop = EventLoop::new();

    let window = WindowBuilder::new()
        .with_title("rendy-pbr")
        .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 960.0));

    let rendy = rendy::init::AnyWindowedRendy::init_auto(&config, window, &event_loop).unwrap();
    rendy::with_any_windowed_rendy!((rendy)
        (factory, families, surface, window) => {
            run(event_loop, surface, window, factory, families)
        }
    )
}

fn run<B: hal::Backend>(
    event_loop: rendy::init::winit::event_loop::EventLoop<()>,
    surface: rendy::wsi::Surface<B>,
    window: Window,
    mut factory: Factory<B>,
    mut families: Families<B>,
) -> Result<(), failure::Error> {
    // Initialize specs and register components
    let mut world = specs::World::new();

    world.register::<components::Transform>();
    world.register::<components::GlobalTransform>();
    world.register::<components::Parent>();
    world.register::<components::Mesh>();
    world.register::<components::Camera>();
    world.register::<components::ActiveCamera>();
    world.register::<components::Light>();

    let scene_config = scene::SceneConfig::from_path("assets/scene.ron")?;

    let input = input::InputState::new(window.inner_size());
    let event_bucket = input::EventBucket(Vec::new());

    #[cfg(feature = "rd")]
    rd.start_frame_capture(std::ptr::null(), std::ptr::null());

    let size = window.inner_size().to_physical(window.hidpi_factor());
    let aspect = (size.width / size.height) as f32;

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

    // Equirectangular env map to environment cube map

    let mut preprocessed_environment_data = {
        let mut env_preprocess_graph_builder =
            GraphBuilder::<B, node::env_preprocess::Aux<B>>::new();

        let env_cube_faces_img = env_preprocess_graph_builder.create_image(
            hal::image::Kind::D2(ENV_CUBEMAP_RES, ENV_CUBEMAP_RES * 6, 1, 1),
            1,
            hal::format::Format::Rgba32Sfloat,
            Some(hal::command::ClearValue {
                color: hal::command::ClearColor {
                    float32: [0.0, 0.0, 0.0, 1.0],
                },
            }),
        );

        let equirect_to_faces_pass = env_preprocess_graph_builder.add_node(
            node::env_preprocess::equirectangular_to_cube_faces::Pipeline::<B>::builder()
                .into_subpass()
                .with_color(env_cube_faces_img)
                .into_pass(),
        );

        let faces_to_env_pass = env_preprocess_graph_builder.add_node(
            node::env_preprocess::faces_to_cubemap::FacesToCubemap::<Backend>::builder(
                vec![env_cube_faces_img],
                "environment",
                node::env_preprocess::faces_to_cubemap::CopyMips::GenerateMips,
            )
            .with_dependency(equirect_to_faces_pass),
        );

        // Environment cube map to convolved irradiance cube map

        let irradiance_cube_faces_img = env_preprocess_graph_builder.create_image(
            hal::image::Kind::D2(IRRADIANCE_CUBEMAP_RES, IRRADIANCE_CUBEMAP_RES * 6, 1, 1),
            1,
            hal::format::Format::Rgba32Sfloat,
            Some(hal::command::ClearValue {
                color: hal::command::ClearColor {
                    float32: [0.0, 0.0, 0.0, 1.0],
                },
            }),
        );

        let env_to_irradiance_faces_pass = env_preprocess_graph_builder.add_node(
            node::env_preprocess::env_to_irradiance::Pipeline::<B>::builder()
                .with_dependency(faces_to_env_pass)
                .into_subpass()
                .with_color(irradiance_cube_faces_img)
                .into_pass(),
        );

        let _irradiance_to_cube_pass = env_preprocess_graph_builder.add_node(
            node::env_preprocess::faces_to_cubemap::FacesToCubemap::<B>::builder(
                vec![irradiance_cube_faces_img],
                "irradiance",
                node::env_preprocess::faces_to_cubemap::CopyMips::CopyMips(1),
            )
            .with_dependency(env_to_irradiance_faces_pass),
        );

        // Environment cube map to convolved specular cube map with different roughnesses stored in mip levels

        let mut env_to_spec_faces_subpasses = Vec::new();
        let mut spec_cube_faces_images = Vec::new();

        for mip_level in 0..SPEC_CUBEMAP_MIP_LEVELS {
            let res = SPEC_CUBEMAP_RES / 2u32.pow(mip_level as u32);
            let mut subpass = node::env_preprocess::env_to_specular::Pipeline::<B>::builder()
                .with_dependency(faces_to_env_pass)
                .into_subpass();
            let image = env_preprocess_graph_builder.create_image(
                hal::image::Kind::D2(res, res * 6, 1, 1),
                1,
                hal::format::Format::Rgba32Sfloat,
                Some(hal::command::ClearValue {
                    color: hal::command::ClearColor {
                        float32: [0.0, 0.0, 0.0, 1.0],
                    },
                }),
            );
            subpass.add_color(image);
            spec_cube_faces_images.push(image);
            env_to_spec_faces_subpasses.push(subpass);
        }

        let mut env_to_spec_faces_passes = Vec::new();
        while !env_to_spec_faces_subpasses.is_empty() {
            env_to_spec_faces_passes.push(
                env_preprocess_graph_builder
                    .add_node(env_to_spec_faces_subpasses.pop().unwrap().into_pass()),
            );
        }

        let mut builder = node::env_preprocess::faces_to_cubemap::FacesToCubemap::<B>::builder(
            spec_cube_faces_images.clone(),
            "specular",
            node::env_preprocess::faces_to_cubemap::CopyMips::CopyMips(SPEC_CUBEMAP_MIP_LEVELS),
        );

        for pass in env_to_spec_faces_passes {
            builder.add_dependency(pass);
        }

        let _spec_to_cube_pass = env_preprocess_graph_builder.add_node(builder);

        let spec_brdf_map = env_preprocess_graph_builder.create_image(
            hal::image::Kind::D2(SPEC_BRDF_MAP_RES, SPEC_BRDF_MAP_RES, 1, 1),
            1,
            hal::format::Format::Rg32Sfloat,
            Some(hal::command::ClearValue {
                color: hal::command::ClearColor {
                    float32: [0.0, 0.0, 0.0, 1.0],
                },
            }),
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

        let equirect_file = std::fs::File::open(
            &std::path::Path::new(&application_root_dir())
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
            .with_mip_levels(rendy::texture::MipLevels::Levels(
                std::num::NonZeroU8::new(ENV_CUBEMAP_MIP_LEVELS).unwrap(),
            ))
            .with_view_kind(rendy::resource::ViewKind::Cube)
            .with_data_width(ENV_CUBEMAP_RES)
            .with_data_height(ENV_CUBEMAP_RES)
            .with_data(vec![
                rendy::texture::pixel::Rgba32Sfloat {
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
                rendy::texture::pixel::Rgba32Sfloat {
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
            .with_mip_levels(rendy::texture::MipLevels::Levels(
                std::num::NonZeroU8::new(SPEC_CUBEMAP_MIP_LEVELS).unwrap(),
            ))
            .with_view_kind(rendy::resource::ViewKind::Cube)
            .with_data_width(SPEC_CUBEMAP_RES)
            .with_data_height(SPEC_CUBEMAP_RES)
            .with_data(vec![
                rendy::texture::pixel::Rgba32Sfloat {
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
                rendy::texture::pixel::Rg32Sfloat { repr: [0.0, 0.0] };
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

        use scene::Quality;
        let mut env_preprocess_aux = node::env_preprocess::Aux {
            align,
            irradiance_theta_samples: match scene_config.environment_filter_quality {
                Quality::High => 720,
                Quality::Medium => 512,
                Quality::Low => 256,
            },
            spec_samples: match scene_config.environment_filter_quality {
                Quality::High => 8192,
                Quality::Medium => 4096,
                Quality::Low => 1024,
            },
            equirectangular_texture: equirect_tex,
            environment_cubemap: Some(env_cubemap_tex),
            irradiance_cubemap: Some(irradiance_cubemap_tex),
            spec_cubemap: Some(spec_cubemap_tex),
            spec_brdf_map: Some(spec_brdf_tex),
            queue,
            mip_level: std::sync::atomic::AtomicUsize::new(0),
        };

        let mut env_preprocess_graph = env_preprocess_graph_builder.build(
            &mut factory,
            &mut families,
            &mut env_preprocess_aux,
        )?;

        factory.maintain(&mut families);
        env_preprocess_graph.run(&mut factory, &mut families, &mut env_preprocess_aux);
        env_preprocess_graph.dispose(&mut factory, &mut env_preprocess_aux);

        env_preprocess_aux
    };

    let mut pbr_graph_builder = GraphBuilder::<B, specs::World>::new();

    let hdr = pbr_graph_builder.create_image(
        hal::image::Kind::D2(size.width as u32, size.height as u32, 1, 1),
        1,
        hal::format::Format::Rgba32Sfloat,
        Some(hal::command::ClearValue {
            color: hal::command::ClearColor {
                float32: [0.1, 0.3, 0.4, 1.0],
            },
        }),
    );

    let color = pbr_graph_builder.create_image(
        hal::image::Kind::D2(size.width as u32, size.height as u32, 1, 1),
        1,
        factory.get_surface_format(&surface),
        Some(hal::command::ClearValue {
            color: hal::command::ClearColor {
                float32: [0.1, 0.3, 0.4, 1.0],
            },
        }),
    );

    let depth = pbr_graph_builder.create_image(
        hal::image::Kind::D2(size.width as u32, size.height as u32, 1, 1),
        1,
        hal::format::Format::D32Sfloat,
        Some(hal::command::ClearValue {
            depth_stencil: hal::command::ClearDepthStencil {
                depth: 1.0,
                stencil: 0,
            },
        }),
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
        env_cube: preprocessed_environment_data.environment_cubemap.take(),
        irradiance_cube: preprocessed_environment_data.irradiance_cubemap.take(),
        spec_cube: preprocessed_environment_data.spec_cubemap.take(),
        spec_brdf_map: preprocessed_environment_data.spec_brdf_map.take(),
    });
    std::mem::drop(preprocessed_environment_data);
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

    // Dispatch once to build all needed initial state before first frame render
    dispatcher.dispatch(&mut world.res);

    let pbr_graph = pbr_graph_builder
        .with_frames_in_flight(FRAMES_IN_FLIGHT)
        .build(&mut factory, &mut families, &mut world)?;

    let started = time::Instant::now();

    let mut frames = 0u64;

    let mut checkpoint = started;

    let mut world = Some(world);
    let mut pbr_graph = Some(pbr_graph);
    event_loop.run(move |event, _, control_flow| {
        match event {
            Event::EventsCleared => {
                // Update logic then request redraw
                if let Some(world) = world.as_mut() {
                    world.maintain();
                    dispatcher.dispatch(&mut world.res);

                    world.write_resource::<input::EventBucket>().0.clear();
                    window.request_redraw();
                }
            }
            Event::WindowEvent {
                event: WindowEvent::RedrawRequested,
                ..
            } => {
                // Draw the app
                match (world.as_mut(), pbr_graph.as_mut()) {
                    (Some(world), Some(pbr_graph)) => {
                        factory.maintain(&mut families);

                        pbr_graph.run(&mut factory, &mut families, world);

                        #[cfg(feature = "rd")]
                        let renderdoc_capturing = rd.is_frame_capturing();
                        #[cfg(not(feature = "rd"))]
                        let renderdoc_capturing = false;

                        if renderdoc_capturing {
                            #[cfg(feature = "rd")]
                            rd.end_frame_capture(std::ptr::null(), std::ptr::null());
                            #[cfg(feature = "rd")]
                            rd.launch_replay_ui("rendy-pbr").unwrap();
                        }

                        let elapsed = checkpoint.elapsed();

                        frames += 1;
                        if elapsed > std::time::Duration::new(2, 0) {
                            let nanos =
                                elapsed.as_secs() * 1_000_000_000 + elapsed.subsec_nanos() as u64;
                            log::info!("FPS: {}", frames * 1_000_000_000 / nanos);
                            log::info!(
                                "Tonemapper Settings: {}",
                                world.read_resource::<node::pbr::Aux>().tonemapper_args
                            );
                            checkpoint += elapsed;
                            frames = 0;
                        }
                    }
                    _ => (),
                }
            }
            // Close on close requested
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                let mut world = world.take().unwrap();
                pbr_graph.take().unwrap().dispose(&mut factory, &mut world);
                // world must be dropped before factory so that resources held in
                // material/mesh/primitive storages can be sent back to the factory for
                // disposal before it is destroyed.
                std::mem::drop(world);

                *control_flow = ControlFlow::Exit;
            }
            // Otherwise add the event to the bucket and continue polling
            _ => {
                world.as_mut().map(|world| {
                    world.write_resource::<input::EventBucket>().0.push(event);
                });
                *control_flow = ControlFlow::Poll;
            }
        }
    });
}

#[cfg(not(any(feature = "dx12", feature = "metal", feature = "vulkan")))]
fn main() -> Result<(), failure::Error> {
    panic!("Specify feature: { dx12, metal, vulkan }");
    Ok(())
}
