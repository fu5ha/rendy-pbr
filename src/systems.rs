use crate::{asset, components, input, node};
use nalgebra::Similarity3;
use rendy::hal;
use specs::{prelude::*, storage::UnprotectedStorage};

use std::collections::HashSet;

pub use crate::transform::systems::*;

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

impl<'a> System<'a> for PbrAuxInputSystem {
    type SystemData = (
        Read<'a, input::EventBucket>,
        Read<'a, input::InputState>,
        Read<'a, asset::MeshStorage>,
        Write<'a, node::pbr::Aux>,
        Write<'a, HelmetArraySize>,
    );

    fn run(
        &mut self,
        (events, input, mesh_storage, mut aux, mut helmet_array_size): Self::SystemData,
    ) {
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
                                        helmet_array_size.try_add_x(mesh.max_instances);
                                    }
                                    (
                                        VirtualKeyCode::X,
                                        ElementState::Pressed,
                                        ModifiersState { shift: true, .. },
                                    ) => {
                                        helmet_array_size.try_sub_x();
                                    }
                                    (
                                        VirtualKeyCode::Y,
                                        ElementState::Pressed,
                                        ModifiersState { shift: false, .. },
                                    ) => {
                                        helmet_array_size.try_add_y(mesh.max_instances);
                                    }
                                    (
                                        VirtualKeyCode::Y,
                                        ElementState::Pressed,
                                        ModifiersState { shift: true, .. },
                                    ) => {
                                        helmet_array_size.try_sub_y();
                                    }
                                    (
                                        VirtualKeyCode::Z,
                                        ElementState::Pressed,
                                        ModifiersState { shift: false, .. },
                                    ) => {
                                        helmet_array_size.try_add_z(mesh.max_instances);
                                    }
                                    (
                                        VirtualKeyCode::Z,
                                        ElementState::Pressed,
                                        ModifiersState { shift: true, .. },
                                    ) => {
                                        helmet_array_size.try_sub_z();
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

    fn run(
        &mut self,
        (events, input, mut transforms, active_cameras, mut cameras): Self::SystemData,
    ) {
        use input::{
            MouseState, ROTATE_SENSITIVITY, TRANSLATE_SENSITIVITY, ZOOM_MOUSE_SENSITIVITY,
            ZOOM_SCROLL_SENSITIVITY,
        };
        use winit::{DeviceEvent, ElementState, ModifiersState, MouseScrollDelta};
        if let Some((_, transform, camera)) = (&active_cameras, &mut transforms, &mut cameras)
            .join()
            .next()
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
                nalgebra::UnitQuaternion::face_towards(
                    &(eye - camera.focus),
                    &nalgebra::Vector3::y(),
                ),
                1.0,
            );
        }
    }
}

#[derive(Default)]
pub struct HelmetArrayEntities(pub Vec<Entity>);

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct HelmetArraySize {
    pub x: u8,
    pub y: u8,
    pub z: u8,
}

impl HelmetArraySize {
    pub fn size(&self) -> usize {
        self.x as usize * self.y as usize * self.z as usize
    }

    pub fn generate_transforms(&self) -> Vec<nalgebra::Similarity3<f32>> {
        let x_size = 3.0;
        let y_size = 4.0;
        let z_size = 4.0;
        let mut transforms = Vec::with_capacity(self.size());
        for x in 0..self.x {
            for y in 0..self.y {
                for z in 0..self.z {
                    transforms.push(nalgebra::Similarity3::from_parts(
                        nalgebra::Translation3::new(
                            (x as f32 * x_size) - (x_size * (self.x - 1) as f32 * 0.5),
                            (y as f32 * y_size) - (y_size * (self.y - 1) as f32 * 0.5),
                            (z as f32 * z_size) - (z_size * (self.z - 1) as f32 * 0.5),
                        ),
                        nalgebra::UnitQuaternion::identity(),
                        1.0,
                    ));
                }
            }
        }
        transforms
    }

    pub fn try_add_x(&mut self, max: u16) {
        let mut n_size = *self;
        n_size.x = n_size.x.checked_add(1).unwrap_or(u8::max_value());
        if n_size.size() <= max as _ {
            *self = n_size
        }
    }

    pub fn try_add_y(&mut self, max: u16) {
        let mut n_size = *self;
        n_size.y = n_size.y.checked_add(1).unwrap_or(u8::max_value());
        if n_size.size() <= max as _ {
            *self = n_size
        }
    }

    pub fn try_add_z(&mut self, max: u16) {
        let mut n_size = *self;
        n_size.z = n_size.z.checked_add(1).unwrap_or(u8::max_value());
        if n_size.size() <= max as _ {
            *self = n_size
        }
    }

    pub fn try_sub_x(&mut self) {
        self.x = (self.x - 1).max(1);
    }

    pub fn try_sub_y(&mut self) {
        self.y = (self.y - 1).max(1);
    }

    pub fn try_sub_z(&mut self) {
        self.z = (self.z - 1).max(1);
    }
}

pub struct HelmetArraySizeUpdateSystem {
    pub curr_size: HelmetArraySize,
    pub helmet_mesh: asset::MeshHandle,
}

impl<'a> System<'a> for HelmetArraySizeUpdateSystem {
    type SystemData = (
        Entities<'a>,
        Write<'a, HelmetArrayEntities>,
        Read<'a, HelmetArraySize>,
        WriteStorage<'a, components::Transform>,
        WriteStorage<'a, components::Mesh>,
    );

    fn run(
        &mut self,
        (
            entities,
            mut helmet_array_entities,
            helmet_array_size,
            mut transforms,
            mut meshes,
        ): Self::SystemData,
    ) {
        if *helmet_array_size != self.curr_size {
            while helmet_array_entities.0.len() < helmet_array_size.size() {
                helmet_array_entities.0.push(entities.create());
            }
            while helmet_array_entities.0.len() > helmet_array_size.size() {
                let entity = helmet_array_entities.0.pop().unwrap();
                entities.delete(entity).unwrap();
                meshes.remove(entity);
                transforms.remove(entity);
            }
            let new_helmet_transforms = helmet_array_size.generate_transforms();
            for (transform, entity) in new_helmet_transforms
                .into_iter()
                .zip(helmet_array_entities.0.iter())
            {
                if let Ok(entry) = transforms.entry(*entity) {
                    let entity_transform = entry.or_insert(Default::default());
                    entity_transform.0 = transform
                }
                if let Ok(entry) = meshes.entry(*entity) {
                    entry.or_insert(components::Mesh(self.helmet_mesh));
                }
            }
        }
    }
}

pub type InstanceIndex = u16;
pub struct MeshInstance {
    pub mesh: asset::MeshHandle,
    pub instance: InstanceIndex,
}

#[derive(Default)]
pub struct MeshInstanceStorage(pub DenseVecStorage<MeshInstance>);

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
    pub mesh_modified: BitSet,
    pub mesh_entity_bitsets: Vec<BitSet>,
    pub _pd: core::marker::PhantomData<B>,
}

impl<'a, B: hal::Backend> System<'a> for InstanceCacheUpdateSystem<B> {
    type SystemData = (
        Entities<'a>,
        Write<'a, InstanceCache>,
        Read<'a, asset::MeshStorage>,
        Write<'a, MeshInstanceStorage>,
        Read<'a, asset::PrimitiveStorage<B>>,
        ReadStorage<'a, components::Mesh>,
        ReadStorage<'a, components::GlobalTransform>,
    );

    fn run(
        &mut self,
        (
            entities,
            mut cache,
            mesh_storage,
            mut mesh_instance_storage,
            primitive_storage,
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
                    ComponentEvent::Inserted(id) => {
                        self.mesh_inserted.add(*id);
                    }
                    ComponentEvent::Removed(id) => {
                        self.mesh_deleted.add(*id);
                    }
                    _ => (),
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
        for (entity, _) in (&entities, &self.mesh_deleted).join() {
            let MeshInstance { mesh, instance } =
                unsafe { mesh_instance_storage.0.remove(entity.id()) };
            self.mesh_entity_bitsets[mesh].remove(entity.id());
            cache.mesh_instance_counts[mesh] -= 1;
            for primitive_idx in mesh_storage.0[mesh].primitives.iter() {
                let primitive = &primitive_storage.0[*primitive_idx];
                cache.material_bitsets[primitive.mat].remove(entity.id());
            }
            for (entity, _) in (&entities, &self.mesh_entity_bitsets[mesh]).join() {
                let mesh_instance = unsafe { mesh_instance_storage.0.get_mut(entity.id()) };
                if mesh_instance.instance > instance {
                    mesh_instance.instance -= 1;
                    self.dirty_entities_scratch.add(entity.id());
                }
            }
            self.dirty_mesh_indirects_scratch.insert(mesh);
        }
        for (entity, mesh, _) in (&entities, &meshes, &self.mesh_inserted).join() {
            unsafe {
                mesh_instance_storage.0.insert(
                    entity.id(),
                    MeshInstance {
                        mesh: mesh.0,
                        instance: cache.mesh_instance_counts[mesh.0] as InstanceIndex,
                    },
                );
            }
            cache.mesh_instance_counts[mesh.0] += 1;
            for primitive_idx in mesh_storage.0[mesh.0].primitives.iter() {
                let primitive = &primitive_storage.0[*primitive_idx];
                cache.material_bitsets[primitive.mat].add(entity.id());
            }
            self.mesh_entity_bitsets[mesh.0].add(entity.id());
            self.dirty_entities_scratch.add(entity.id());
            self.dirty_mesh_indirects_scratch.insert(mesh.0);
        }
        for i in 0..self.frames_in_flight {
            cache.dirty_entities[i] |= &self.dirty_entities_scratch;
            cache.dirty_mesh_indirects[i].extend(&self.dirty_mesh_indirects_scratch);
        }
        self.previous_frame = (self.previous_frame + 1) % self.frames_in_flight;
        self.mesh_inserted.clear();
        self.mesh_deleted.clear();
        self.mesh_modified.clear();
    }
}
