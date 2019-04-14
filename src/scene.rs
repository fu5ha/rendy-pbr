use crate::{asset, components};

use rendy::hal;
use serde::Deserialize;
use specs::prelude::*;

use std::{
    convert::{TryFrom, TryInto},
    fs::File,
    path::Path,
};

pub type BasePath = String;
pub type Filename = String;

pub type GltfFileIndex = usize;

#[derive(Debug, Deserialize)]
pub struct SceneConfig {
    pub gltf_sources: Vec<(BasePath, Filename)>,
    pub entities: Vec<SceneEntity>,
}

impl SceneConfig {
    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Self, failure::Error> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(path.as_ref());
        let file = File::open(path).unwrap();
        let reader = std::io::BufReader::new(file);
        ron::de::from_reader(reader).map_err(From::from)
    }

    pub fn load<B: hal::Backend>(
        mut self,
        aspect: f32,
        factory: &mut rendy::factory::Factory<B>,
        queue: rendy::command::QueueId,
        world: &mut specs::World,
    ) -> Result<
        (
            asset::MaterialStorage<B>,
            asset::PrimitiveStorage<B>,
            asset::MeshStorage,
            Vec<specs::Entity>,
        ),
        failure::Error,
    > {
        let mut mesh_storage = Vec::new();
        let mut primitive_storage = Vec::new();
        let mut material_storage = Vec::new();
        let mut scene_entities = Vec::new();
        // (node, mesh, material)
        let mut gltf_file_offsets = vec![(0, 0, 0)];

        let (gltfs, basepaths): (Vec<_>, Vec<_>) = self
            .gltf_sources
            .drain(..)
            .map(|path| {
                let base_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(path.0);
                let file = File::open(base_path.join(path.1)).unwrap();
                let reader = std::io::BufReader::new(file);
                (gltf::Gltf::from_reader(reader).unwrap(), base_path.clone())
            })
            .unzip();

        for (source_index, (gltf, base_path)) in gltfs.iter().zip(basepaths.iter()).enumerate() {
            let gltf_buffers = asset::GltfBuffers::load_from_gltf(base_path, gltf)?;

            let offsets = gltf_file_offsets[source_index];
            let base_mesh_index = offsets.0;
            let base_material_index = offsets.1;
            for _ in 0..gltf.meshes().len() {
                mesh_storage.push(None);
            }
            for _ in 0..gltf.materials().len() {
                material_storage.push(None);
            }

            for mesh in gltf.meshes() {
                asset::load_gltf_mesh(
                    &mesh,
                    256,
                    base_path,
                    &gltf_buffers,
                    base_mesh_index,
                    base_material_index,
                    &mut material_storage,
                    &mut primitive_storage,
                    &mut mesh_storage,
                    factory,
                    queue,
                )?;
            }

            gltf_file_offsets.push((
                mesh_storage.len(),
                material_storage.len(),
                offsets.2 + gltf.nodes().len(),
            ))
        }

        let mut active_camera_de = false;
        for (i, scene_entity) in self.entities.iter().enumerate() {
            let mut entity_builder = world.create_entity();

            let transform = match &scene_entity.transform {
                TransformSource::Gltf(gltf_node) => {
                    let src: GltfFileIndex = gltf_node.into();
                    let node: gltf::Node =
                        GltfNodeWrapper::from((&gltfs[src], gltf_node)).try_into()?;
                    components::Transform::from(node.transform())
                }
                TransformSource::Manual(transform) => transform.clone(),
            };
            entity_builder = entity_builder.with(transform);

            match &scene_entity.mesh {
                Some(MeshSource::Node(gltf_node)) => {
                    let src: GltfFileIndex = gltf_node.into();
                    if src >= gltfs.len() {
                        failure::bail!("Data source Gltf File for entity: {} out of bounds", i);
                    }
                    let node: gltf::Node =
                        GltfNodeWrapper::from((&gltfs[src], gltf_node)).try_into()?;
                    let node_mesh = node.mesh().ok_or(failure::format_err!(
                        "Entity with Combined data refers to node with no Mesh: {:?}",
                        gltf_node
                    ))?;
                    entity_builder = entity_builder.with(components::Mesh(
                        gltf_file_offsets[src].0 + node_mesh.index(),
                    ));
                }
                Some(MeshSource::Mesh(mesh)) => {
                    let mesh = match mesh {
                        GltfMesh::Index(src, idx) => components::Mesh(
                            gltfs[*src]
                                .meshes()
                                .nth(*idx)
                                .ok_or(failure::format_err!(
                                    "GltfMesh refers to mesh that does not exist: {:?}",
                                    mesh
                                ))?
                                .index()
                                + gltf_file_offsets[*src].1,
                        ),
                        GltfMesh::Name(src, name) => components::Mesh(
                            gltfs[*src]
                                .meshes()
                                .find(|mesh| {
                                    if let Some(mesh_name) = mesh.name() {
                                        if mesh_name == name {
                                            true
                                        } else {
                                            false
                                        }
                                    } else {
                                        false
                                    }
                                })
                                .ok_or(failure::format_err!(
                                    "GltfMesh refers to mesh that does not exist: {:?}",
                                    mesh
                                ))?
                                .index()
                                + gltf_file_offsets[*src].1,
                        ),
                    };
                    entity_builder = entity_builder.with(mesh);
                }
                None => (),
            }

            if let Some(light) = &scene_entity.light {
                entity_builder = entity_builder.with(*light);
            }

            if let Some(camera_data) = &scene_entity.camera {
                entity_builder = entity_builder.with(components::Camera {
                    yaw: camera_data.yaw,
                    pitch: camera_data.pitch,
                    dist: camera_data.distance,
                    focus: nalgebra::Point3::from(camera_data.focus_point),
                    proj: nalgebra::Perspective3::new(
                        aspect,
                        camera_data.fov,
                        camera_data.znear,
                        camera_data.zfar,
                    ),
                });
                if camera_data.active {
                    if !active_camera_de {
                        active_camera_de = true;
                        entity_builder = entity_builder.with(components::ActiveCamera);
                    } else {
                        failure::bail!("Attempted to load multiple active cameras");
                    }
                }
            }

            scene_entities.push(entity_builder.build());
        }

        for (i, scene_entity) in self.entities.iter().enumerate() {
            if let Some(parent_idx) = scene_entity.parent {
                let mut parent_storage = world.write_storage::<components::Parent>();
                parent_storage.insert(
                    scene_entities[i],
                    components::Parent::new(scene_entities[parent_idx]),
                )?;
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

        Ok((
            material_storage,
            primitive_storage,
            mesh_storage,
            scene_entities,
        ))
    }
}

#[derive(Debug, Deserialize)]
pub enum GltfNode {
    Index(GltfFileIndex, usize),
    Name(GltfFileIndex, String),
}

impl From<&GltfNode> for GltfFileIndex {
    fn from(node: &GltfNode) -> Self {
        match node {
            GltfNode::Index(src, _) => *src,
            GltfNode::Name(src, _) => *src,
        }
    }
}

struct GltfNodeWrapper<'a> {
    gltf: &'a gltf::Gltf,
    gltf_node: &'a GltfNode,
}

impl<'a> From<(&'a gltf::Gltf, &'a GltfNode)> for GltfNodeWrapper<'a> {
    fn from(tuple: (&'a gltf::Gltf, &'a GltfNode)) -> Self {
        GltfNodeWrapper {
            gltf: tuple.0,
            gltf_node: tuple.1,
        }
    }
}

impl<'a> TryFrom<GltfNodeWrapper<'a>> for gltf::Node<'a> {
    type Error = failure::Error;

    fn try_from(wrapper: GltfNodeWrapper<'a>) -> Result<Self, failure::Error> {
        let GltfNodeWrapper { gltf, gltf_node } = wrapper;
        match gltf_node {
            GltfNode::Index(_src, idx) => {
                Ok(gltf.nodes().nth(*idx).ok_or(failure::format_err!(
                    "GltfNode refers to node that does not exist: {:?}",
                    gltf_node
                ))?)
            }
            GltfNode::Name(_src, name) => Ok(gltf
                .nodes()
                .find(|node| {
                    if let Some(node_name) = node.name() {
                        if node_name == name {
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                })
                .ok_or(failure::format_err!(
                    "GltfNode refers to node that does not exist: {:?}",
                    gltf_node
                ))?),
        }
    }
}

#[derive(Debug, Deserialize)]
pub enum GltfMesh {
    Index(GltfFileIndex, usize),
    Name(GltfFileIndex, String),
}

#[derive(Debug, Deserialize)]
pub enum TransformSource {
    Gltf(GltfNode),
    Manual(components::Transform),
}

#[derive(Debug, Deserialize)]
pub enum MeshSource {
    Node(GltfNode),
    Mesh(GltfMesh),
}

#[derive(Debug, Deserialize)]
pub struct CameraData {
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub focus_point: [f32; 3],
    pub fov: f32,
    pub znear: f32,
    pub zfar: f32,
    pub active: bool,
}

pub type SceneEntityIndex = usize;

#[derive(Debug, Deserialize)]
pub struct SceneEntity {
    parent: Option<SceneEntityIndex>,
    transform: TransformSource,
    mesh: Option<MeshSource>,
    light: Option<components::Light>,
    camera: Option<CameraData>,
}
