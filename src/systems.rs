use crate::{asset, components, input, node};
use nalgebra::Similarity3;
use rendy::hal;
use specs::prelude::*;

use std::collections::HashSet;

pub struct InputSystem;

impl<'a> System<'a> for InputSystem {
    type SystemData = (Read<'a, input::EventBucket>, Write<'a, input::InputState>);

    fn run(&mut self, (events, mut input): Self::SystemData) {
        for event in events.0.iter() {
            match event {
                winit::Event::WindowEvent { event, .. } => {
                    input.update_with_window_event(&event);
                }
                _ => (),
            }
        }
    }
}

pub struct PbrAuxInputSystem {
    pub helmet_mesh: asset::MeshHandle,
}

fn try_add_instance_array_size_x(ia_size: (u8, u8, u8), max: u16) -> (u8, u8, u8) {
    let mut n_ia_size = ia_size;
    n_ia_size.0 = n_ia_size.0.checked_add(1).unwrap_or(u8::max_value());
    if n_ia_size.0 as u16 * n_ia_size.1 as u16 * n_ia_size.2 as u16 <= max {
        n_ia_size
    } else {
        ia_size
    }
}

fn try_add_instance_array_size_y(ia_size: (u8, u8, u8), max: u16) -> (u8, u8, u8) {
    let mut n_ia_size = ia_size;
    n_ia_size.1 = n_ia_size.1.checked_add(1).unwrap_or(u8::max_value());
    if n_ia_size.0 as u16 * n_ia_size.1 as u16 * n_ia_size.2 as u16 <= max {
        n_ia_size
    } else {
        ia_size
    }
}

fn try_add_instance_array_size_z(ia_size: (u8, u8, u8), max: u16) -> (u8, u8, u8) {
    let mut n_ia_size = ia_size;
    n_ia_size.2 = n_ia_size.2.checked_add(1).unwrap_or(u8::max_value());
    if n_ia_size.0 as u16 * n_ia_size.1 as u16 * n_ia_size.2 as u16 <= max {
        n_ia_size
    } else {
        ia_size
    }
}

impl<'a> System<'a> for PbrAuxInputSystem {
    type SystemData = (
        Read<'a, input::EventBucket>,
        Read<'a, input::InputState>,
        Read<'a, asset::MeshStorage>,
        Write<'a, node::pbr::Aux>,
    );

    fn run(&mut self, (events, input, mesh_storage, mut aux): Self::SystemData) {
        use input::MouseState;
        use winit::{ElementState, ModifiersState, VirtualKeyCode, WindowEvent};

        let mesh = &mesh_storage.0[self.helmet_mesh];

        let mut input = (*input).clone();
        for event in events.0.iter() {
            match event {
                winit::Event::WindowEvent { event, .. } => {
                    input.update_with_window_event(&event);
                    match event {
                        WindowEvent::CursorMoved { .. } | WindowEvent::MouseInput { .. } => {
                            if let (
                                MouseState {
                                    left: ElementState::Pressed,
                                    ..
                                },
                                ModifiersState { ctrl: true, .. },
                            ) = (input.mouse, input.modifiers)
                            {
                                aux.tonemapper_args.comparison_factor =
                                    input.calc_comparison_factor();
                            }
                        }
                        WindowEvent::KeyboardInput {
                            input: key_input, ..
                        } => {
                            if let Some(kc) = key_input.virtual_keycode {
                                match (kc, key_input.state, input.modifiers) {
                                    // Array size controls
                                    (
                                        VirtualKeyCode::X,
                                        ElementState::Pressed,
                                        ModifiersState { shift: false, .. },
                                    ) => {
                                        aux.instance_array_size = try_add_instance_array_size_x(
                                            aux.instance_array_size,
                                            mesh.max_instances,
                                        );
                                    }
                                    (
                                        VirtualKeyCode::X,
                                        ElementState::Pressed,
                                        ModifiersState { shift: true, .. },
                                    ) => {
                                        aux.instance_array_size.0 =
                                            (aux.instance_array_size.0 - 1).max(1);
                                    }
                                    (
                                        VirtualKeyCode::Y,
                                        ElementState::Pressed,
                                        ModifiersState { shift: false, .. },
                                    ) => {
                                        aux.instance_array_size = try_add_instance_array_size_y(
                                            aux.instance_array_size,
                                            mesh.max_instances,
                                        );
                                    }
                                    (
                                        VirtualKeyCode::Y,
                                        ElementState::Pressed,
                                        ModifiersState { shift: true, .. },
                                    ) => {
                                        aux.instance_array_size.1 =
                                            (aux.instance_array_size.1 - 1).max(1);
                                    }
                                    (
                                        VirtualKeyCode::Z,
                                        ElementState::Pressed,
                                        ModifiersState { shift: false, .. },
                                    ) => {
                                        aux.instance_array_size = try_add_instance_array_size_z(
                                            aux.instance_array_size,
                                            mesh.max_instances,
                                        );
                                    }
                                    (
                                        VirtualKeyCode::Z,
                                        ElementState::Pressed,
                                        ModifiersState { shift: true, .. },
                                    ) => {
                                        aux.instance_array_size.2 =
                                            (aux.instance_array_size.2 - 1).max(1);
                                    }
                                    // Tonemapper controls
                                    (
                                        VirtualKeyCode::E,
                                        ElementState::Pressed,
                                        ModifiersState { shift: false, .. },
                                    ) => {
                                        aux.tonemapper_args.exposure +=
                                            input::EXPOSURE_ADJUST_SENSITIVITY;
                                    }
                                    (
                                        VirtualKeyCode::E,
                                        ElementState::Pressed,
                                        ModifiersState { shift: true, .. },
                                    ) => {
                                        aux.tonemapper_args.exposure -=
                                            input::EXPOSURE_ADJUST_SENSITIVITY;
                                    }
                                    (
                                        VirtualKeyCode::A,
                                        ElementState::Pressed,
                                        ModifiersState { .. },
                                    ) => aux.tonemapper_args.curve = 0,
                                    (
                                        VirtualKeyCode::U,
                                        ElementState::Pressed,
                                        ModifiersState { .. },
                                    ) => aux.tonemapper_args.curve = 1,
                                    (
                                        VirtualKeyCode::C,
                                        ElementState::Pressed,
                                        ModifiersState { .. },
                                    ) => aux.tonemapper_args.curve = 2,
                                    _ => (),
                                }
                            }
                        }
                        _ => (),
                    }
                }
                _ => (),
            }
        }
    }
}

pub struct CameraInputSystem;

impl<'a> System<'a> for CameraInputSystem {
    type SystemData = (
        Read<'a, input::EventBucket>,
        Read<'a, input::InputState>,
        WriteStorage<'a, components::Transform>,
        ReadStorage<'a, components::ActiveCamera>,
        WriteStorage<'a, components::Camera>,
    );

    fn run(&mut self, (events, input, mut transforms, active_cameras, mut cameras): Self::SystemData) {
        use input::{
            MouseState, ROTATE_SENSITIVITY, TRANSLATE_SENSITIVITY, ZOOM_MOUSE_SENSITIVITY,
            ZOOM_SCROLL_SENSITIVITY,
        };
        use winit::{DeviceEvent, ElementState, ModifiersState, MouseScrollDelta};
        if let Some((_, transform, camera)) =
            (&active_cameras, &mut transforms, &mut cameras).join().next()
        {
            let mut input = (*input).clone();
            for event in events.0.iter() {
                match event {
                    winit::Event::WindowEvent { event, .. } => {
                        input.update_with_window_event(&event);
                    }
                    winit::Event::DeviceEvent { event, .. } => match event {
                        DeviceEvent::MouseMotion { delta } => {
                            match (input.mouse, input.modifiers) {
                                (
                                    MouseState {
                                        left: ElementState::Pressed,
                                        ..
                                    },
                                    ModifiersState { ctrl: false, .. },
                                ) => {
                                    camera.yaw += -delta.0 as f32 * ROTATE_SENSITIVITY;
                                    camera.pitch += delta.1 as f32 * ROTATE_SENSITIVITY;
                                    camera.pitch = camera
                                        .pitch
                                        .max(-std::f32::consts::FRAC_PI_2 + 0.0001)
                                        .min(std::f32::consts::FRAC_PI_2 - 0.0001);
                                }
                                (
                                    MouseState {
                                        middle: ElementState::Pressed,
                                        ..
                                    },
                                    ModifiersState { ctrl: false, .. },
                                ) => {
                                    let m_vec = nalgebra::Vector3::new(
                                        -delta.0 as f32,
                                        delta.1 as f32,
                                        0.0,
                                    ) * TRANSLATE_SENSITIVITY;
                                    let rot = transform.0.isometry.rotation;
                                    let m_vec = rot * m_vec;
                                    camera.focus = camera.focus + m_vec;
                                }
                                (
                                    MouseState {
                                        right: ElementState::Pressed,
                                        ..
                                    },
                                    ModifiersState { ctrl: false, .. },
                                ) => {
                                    let amount = -delta.0 as f32 * ZOOM_MOUSE_SENSITIVITY;
                                    camera.dist += amount;
                                    camera.dist = camera.dist.max(0.0);
                                }
                                _ => (),
                            }
                        }
                        DeviceEvent::MouseWheel { delta } => {
                            let amount = match delta {
                                MouseScrollDelta::LineDelta(_, y) => {
                                    -y as f32 * ZOOM_SCROLL_SENSITIVITY
                                }
                                MouseScrollDelta::PixelDelta(delta) => {
                                    -delta.y as f32 * ZOOM_SCROLL_SENSITIVITY * 0.05
                                }
                            };
                            camera.dist += amount;
                            camera.dist = camera.dist.max(0.0);
                        }
                        _ => (),
                    },
                    _ => (),
                }
            }

            let eye = camera.focus
                + (camera.dist
                    * nalgebra::Vector3::new(
                        camera.yaw.sin() * camera.pitch.cos(),
                        camera.pitch.sin(),
                        camera.yaw.cos() * camera.pitch.cos(),
                    ));

            transform.0 = Similarity3::from_parts(
                nalgebra::Translation::from(eye.coords.clone()),
                // Invert direction for right handed
                nalgebra::UnitQuaternion::face_towards(&(eye - camera.focus), &nalgebra::Vector3::y()),
                1.0,
            );
        }
    }
}

pub type InstanceIndex = u16;
pub struct MeshInstance(pub InstanceIndex);

impl Component for MeshInstance {
    type Storage = DenseVecStorage<Self>;
}

#[derive(Default, Debug)]
pub struct InstanceCache {
    pub dirty_entities: Vec<BitSet>,
    pub dirty_mesh_indirects: Vec<HashSet<asset::MeshHandle>>,
    pub mesh_instance_counts: Vec<u32>,
    pub material_bitsets: Vec<BitSet>,
}

pub struct InstanceCacheUpdateSystem<B> {
    pub frames_in_flight: usize,
    pub previous_frame: usize,
    pub mesh_reader_id: ReaderId<ComponentEvent>,
    pub transform_reader_id: ReaderId<ComponentEvent>,
    pub dirty_entities_scratch: BitSet,
    pub dirty_mesh_indirects_scratch: HashSet<asset::MeshHandle>,
    pub mesh_inserted: BitSet,
    pub mesh_deleted: BitSet,
    pub mesh_entity_bitsets: Vec<BitSet>,
    pub _pd: core::marker::PhantomData<B>,
}

impl<'a, B: hal::Backend> System<'a> for InstanceCacheUpdateSystem<B> {
    type SystemData = (
        Entities<'a>,
        Write<'a, InstanceCache>,
        Read<'a, asset::MeshStorage>,
        Read<'a, asset::PrimitiveStorage<B>>,
        WriteStorage<'a, MeshInstance>,
        ReadStorage<'a, components::Mesh>,
        ReadStorage<'a, components::Transform>,
    );

    fn run(
        &mut self,
        (
            entities,
            mut cache,
            mesh_storage,
            primitive_storage,
            mut mesh_instances,
            meshes,
            transforms,
        ): Self::SystemData,
    ) {
        cache.dirty_entities[self.previous_frame].clear();
        cache.dirty_mesh_indirects[self.previous_frame].clear();
        self.dirty_entities_scratch.clear();
        self.dirty_mesh_indirects_scratch.clear();
        {
            let events = meshes.channel().read(&mut self.mesh_reader_id);
            for event in events {
                match event {
                    ComponentEvent::Modified(id) => {
                        self.dirty_entities_scratch.add(*id);
                    }
                    ComponentEvent::Inserted(id) => {
                        self.mesh_inserted.add(*id);
                    }
                    ComponentEvent::Removed(id) => {
                        self.mesh_deleted.add(*id);
                    }
                };
            }
        }
        {
            let mesh_mask = meshes.mask();
            let events = transforms.channel().read(&mut self.transform_reader_id);
            for event in events {
                match event {
                    ComponentEvent::Modified(id) => {
                        if mesh_mask.contains(*id) {
                            self.dirty_entities_scratch.add(*id);
                        }
                    }
                    _ => (),
                };
            }
        }
        for (entity, mesh, _) in (&entities, &meshes, &self.mesh_inserted).join() {
            mesh_instances
                .insert(
                    entity,
                    MeshInstance(cache.mesh_instance_counts[mesh.0] as InstanceIndex),
                )
                .unwrap();
            cache.mesh_instance_counts[mesh.0] += 1;
            for primitive_idx in mesh_storage.0[mesh.0].primitives.iter() {
                let primitive = &primitive_storage.0[*primitive_idx];
                cache.material_bitsets[primitive.mat].add(entity.id());
            }
            self.mesh_entity_bitsets[mesh.0].add(entity.id());
            self.dirty_entities_scratch.add(entity.id());
            self.dirty_mesh_indirects_scratch.insert(mesh.0);
        }
        for (entity, mesh, _) in (&entities, &meshes, &self.mesh_deleted).join() {
            let deleted_idx = mesh_instances.get(entity).unwrap().0;
            mesh_instances.remove(entity);
            self.mesh_entity_bitsets[mesh.0].remove(entity.id());
            cache.mesh_instance_counts[mesh.0] -= 1;
            for primitive_idx in mesh_storage.0[mesh.0].primitives.iter() {
                let primitive = &primitive_storage.0[*primitive_idx];
                cache.material_bitsets[primitive.mat].remove(entity.id());
            }
            for (entity, mesh_instance, _) in (
                &entities,
                &mut mesh_instances,
                &self.mesh_entity_bitsets[mesh.0],
            )
                .join()
            {
                if mesh_instance.0 > deleted_idx {
                    mesh_instance.0 -= 1;
                    self.dirty_entities_scratch.add(entity.id());
                }
            }
            self.dirty_mesh_indirects_scratch.insert(mesh.0);
        }
        for i in 0..self.frames_in_flight {
            cache.dirty_entities[i] |= &self.dirty_entities_scratch;
            cache.dirty_mesh_indirects[i].extend(&self.dirty_mesh_indirects_scratch);
        }
        self.mesh_inserted.clear();
        self.mesh_deleted.clear();
        self.previous_frame = (self.previous_frame + 1) % self.frames_in_flight;
    }
}
