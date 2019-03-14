#![cfg_attr(
    not(any(feature = "dx12", feature = "metal", feature = "vulkan")),
    allow(unused)
)]

use rendy::{
    command::{Graphics, Supports},
    factory::{Config, Factory, ImageState},
    graph::{present::PresentNode, render::*, GraphBuilder},
    memory::MemoryUsageValue,
    resource::image::{RenderTargetSampled, TextureUsage},
};

use std::{collections::HashMap, fs::File, path::Path, time};

use gfx_hal as hal;

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

pub fn generate_instances(size: (usize, usize, usize)) -> Vec<nalgebra::Matrix4<f32>> {
    let x_size = 3.0;
    let y_size = 4.0;
    let z_size = 4.0;
    let mut instances = Vec::with_capacity(size.0 * size.1 * size.2);
    for x in 0..size.0 {
        for y in 0..size.1 {
            for z in 0..size.2 {
                instances.push(nalgebra::Matrix4::new_translation(&nalgebra::Vector3::new(
                    (x as f32 * x_size) - (x_size * (size.0 - 1) as f32 * 0.5),
                    (y as f32 * y_size) - (y_size * (size.1 - 1) as f32 * 0.5),
                    (z as f32 * z_size) - (z_size * (size.2 - 1) as f32 * 0.5),
                )));
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

    // Preprocess steps to load environment map, convert it to a cubemap,
    // and filter it for use later
    let mut env_preprocess_graph_builder =
        GraphBuilder::<Backend, node::env_preprocess::Aux<Backend>>::new();

    let mut equirect_to_faces =
        node::env_preprocess::equirectangular_to_cube_faces::Pipeline::<Backend>::builder()
            .into_subpass();

    let cube_face_images = hal::image::CUBE_FACES
        .into_iter()
        .map(|_| {
            env_preprocess_graph_builder.create_image(
                hal::image::Kind::D2(CUBEMAP_RES, CUBEMAP_RES, 1, 1),
                1,
                hal::format::Format::Rgb32Float,
                MemoryUsageValue::Data,
                None,
            )
        })
        .collect::<Vec<_>>();

    for image in cube_face_images.iter().cloned() {
        equirect_to_faces.add_color(image);
    }

    let equirect_to_faces_pass =
        env_preprocess_graph_builder.add_node(equirect_to_faces.into_pass());

    let _faces_to_cubemap_pass = env_preprocess_graph_builder.add_node(
        node::env_preprocess::faces_to_cubemap::FacesToCubemap::<Backend>::builder(
            cube_face_images,
        )
        .with_dependency(equirect_to_faces_pass),
    );

    let equirect_file = std::fs::File::open(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/environment/abandoned_hall_01_4k.hdr"
    ))?;

    let equirect_tex = rendy::texture::image::load_from_image(
        std::io::BufReader::new(equirect_file),
        Default::default(),
        TextureUsage,
        factory.physical(),
    )?
    .build(
        ImageState {
            queue,
            stage: hal::pso::PipelineStage::FRAGMENT_SHADER,
            access: hal::image::Access::SHADER_READ,
            layout: hal::image::Layout::ShaderReadOnlyOptimal,
        },
        &mut factory,
        TextureUsage,
    )?;

    let cubemap_tex = rendy::texture::TextureBuilder::new()
        .with_kind(rendy::resource::image::Kind::D2(
            CUBEMAP_RES,
            CUBEMAP_RES,
            6,
            1,
        ))
        .with_view_kind(rendy::resource::image::ViewKind::Cube)
        .with_raw_format(hal::format::Format::Rgba32Float)
        .build(
            ImageState {
                queue,
                stage: hal::pso::PipelineStage::TRANSFER,
                access: hal::image::Access::TRANSFER_WRITE,
                layout: hal::image::Layout::TransferDstOptimal,
            },
            &mut factory,
            RenderTargetSampled,
        )?;

    let mut env_preprocess_aux = node::env_preprocess::Aux {
        align,
        equirectangular_texture: equirect_tex,
        environment_cubemap: cubemap_tex,
    };

    let mut env_preprocess_graph =
        env_preprocess_graph_builder.build(&mut factory, &mut families, &mut env_preprocess_aux)?;

    env_preprocess_graph.run(&mut factory, &mut families, &mut env_preprocess_aux);

    // Main window and render graph building
    let mut event_loop = EventsLoop::new();

    let window = WindowBuilder::new()
        .with_title("rendy-pbr")
        .with_dimensions(winit::dpi::LogicalSize::new(1280.0, 960.0))
        .build(&event_loop)?;

    let mut input = input::InputState::new(window.get_inner_size().unwrap());

    event_loop.poll_events(|_| ());

    let surface = factory.create_surface(window.into());
    let aspect = surface.aspect();

    let mut pbr_graph_builder = GraphBuilder::<Backend, node::pbr::Aux<Backend>>::new();

    let hdr = pbr_graph_builder.create_image(
        surface.kind(),
        1,
        hal::format::Format::Rgb32Float,
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

    let mut material_storage = HashMap::new();

    let mut helmet: Option<scene::Object> = None;
    {
        let base_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/gltf/helmet/");
        let file = File::open(base_path.join("SciFiHelmet.gltf"))?;
        let reader = std::io::BufReader::new(file);
        let gltf = gltf::Gltf::from_reader(reader)?;

        let gltf_buffers = asset::GltfBuffers::load_from_gltf(&base_path, &gltf)?;

        let scene = gltf.scenes().next().unwrap();

        for node in scene.nodes() {
            match node.name() {
                Some("SciFiHelmet") => {
                    if let Some(mesh) = node.mesh() {
                        helmet = Some(asset::object_from_gltf(
                            &mesh,
                            &base_path,
                            &gltf_buffers,
                            &mut material_storage,
                            &mut factory,
                            queue,
                        )?);
                    }
                }
                _ => (),
            }
        }
    }

    let camera = scene::Camera {
        yaw: 0.0,
        pitch: 0.0,
        dist: 10.0,
        focus: nalgebra::Point3::new(0.0, 0.0, 0.0),
        proj: nalgebra::Perspective3::new(aspect, 3.1415 / 6.0, 1.0, 200.0),
        view: nalgebra::Isometry3::from_parts(
            nalgebra::Translation3::new(0.0, 0.0, -10.0),
            nalgebra::UnitQuaternion::identity(),
        ),
    };
    let instance_array_size = (1, 1, 1);
    let mut pbr_aux = node::pbr::Aux {
        frames,
        align,
        instance_array_size,
        scene: scene::Scene {
            camera,
            max_obj_instances: vec![512],
            objects: vec![(helmet?, generate_instances(instance_array_size))],
            lights: vec![
                scene::Light {
                    pos: nalgebra::Vector3::new(10.0, 10.0, 2.0),
                    intensity: 150.0,
                    color: [1.0, 1.0, 1.0],
                    _pad: 0.0,
                },
                scene::Light {
                    pos: nalgebra::Vector3::new(8.0, 10.0, 2.0),
                    intensity: 150.0,
                    color: [1.0, 1.0, 1.0],
                    _pad: 0.0,
                },
                scene::Light {
                    pos: nalgebra::Vector3::new(8.0, 10.0, 4.0),
                    intensity: 150.0,
                    color: [1.0, 1.0, 1.0],
                    _pad: 0.0,
                },
                scene::Light {
                    pos: nalgebra::Vector3::new(10.0, 10.0, 4.0),
                    intensity: 150.0,
                    color: [1.0, 1.0, 1.0],
                    _pad: 0.0,
                },
                scene::Light {
                    pos: nalgebra::Vector3::new(-4.0, 0.0, -5.0),
                    intensity: 250.0,
                    color: [1.0, 1.0, 1.0],
                    _pad: 0.0,
                },
                scene::Light {
                    pos: nalgebra::Vector3::new(-5.0, 5.0, -2.0),
                    intensity: 25.0,
                    color: [1.0, 1.0, 1.0],
                    _pad: 0.0,
                },
            ],
        },
        material_storage,
        tonemapper_args: node::pbr::tonemap::TonemapperArgs {
            exposure: 2.5,
            curve: 0,
            comparison_factor: 0.5,
        },
    };

    let mut pbr_graph = pbr_graph_builder.build(&mut factory, &mut families, &mut pbr_aux)?;

    let started = time::Instant::now();

    let mut frames = 0u64..;

    let mut checkpoint = started;
    let mut should_close = false;

    while !should_close {
        let start = frames.start;
        for _ in &mut frames {
            factory.maintain(&mut families);
            event_loop.poll_events(|event| match event {
                Event::WindowEvent {
                    event: WindowEvent::CloseRequested,
                    ..
                } => should_close = true,
                Event::WindowEvent { event, .. } => {
                    input.handle_window_event(&event, &mut pbr_aux);
                }
                Event::DeviceEvent { event, .. } => {
                    input.handle_device_event(&event, &mut pbr_aux);
                }
                _ => (),
            });
            pbr_aux.scene.objects[0].1 = generate_instances(pbr_aux.instance_array_size);
            pbr_graph.run(&mut factory, &mut families, &mut pbr_aux);

            let elapsed = checkpoint.elapsed();

            if should_close || elapsed > std::time::Duration::new(2, 0) {
                let frames = frames.start - start;
                let nanos = elapsed.as_secs() * 1_000_000_000 + elapsed.subsec_nanos() as u64;
                log::info!("FPS: {}", frames * 1_000_000_000 / nanos);
                log::info!("Tonemapper Settings: {}", pbr_aux.tonemapper_args);
                checkpoint += elapsed;
                break;
            }
        }
    }

    pbr_graph.dispose(&mut factory, &mut pbr_aux);
    Ok(())
}

#[cfg(not(any(feature = "dx12", feature = "metal", feature = "vulkan")))]
fn main() -> Result<(), failure::Error> {
    panic!("Specify feature: { dx12, metal, vulkan }");
    Ok(())
}
