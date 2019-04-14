//! Local transform component.
use std::fmt;

use nalgebra::{
    self as na, Matrix4, Quaternion, Similarity3, Translation3, Unit, UnitQuaternion, Vector3,
};
use serde::{
    de::{self, Deserializer, MapAccess, SeqAccess, Visitor},
    ser::Serializer,
    Deserialize, Serialize,
};
use specs::prelude::{Component, DenseVecStorage, FlaggedStorage};

#[derive(Debug, Copy, Clone)]
pub struct GlobalTransform(pub Matrix4<f32>);

impl GlobalTransform {
    pub fn is_finite(&self) -> bool {
        self.0.as_slice().iter().all(|f| f32::is_finite(*f))
    }
}

impl Component for GlobalTransform {
    type Storage = FlaggedStorage<Self, DenseVecStorage<Self>>;
}

impl Default for GlobalTransform {
    fn default() -> Self {
        GlobalTransform(na::one())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Transform(pub Similarity3<f32>);

impl Transform {
    pub fn new(position: Translation3<f32>, rotation: UnitQuaternion<f32>, scale: f32) -> Self {
        Transform(Similarity3::from_parts(position, rotation, scale))
    }
}

impl From<gltf::scene::Transform> for Transform {
    fn from(transform: gltf::scene::Transform) -> Self {
        use gltf::scene::Transform as GltfTransform;
        match transform {
            GltfTransform::Matrix { .. } => unimplemented!(),
            GltfTransform::Decomposed {
                translation,
                rotation,
                scale,
            } => Transform::new(
                nalgebra::Translation3::new(translation[0], translation[1], translation[2]),
                nalgebra::UnitQuaternion::from_quaternion(nalgebra::Quaternion::new(
                    rotation[3],
                    rotation[0],
                    rotation[1],
                    rotation[2],
                )),
                scale.iter().sum::<f32>() / 3.0,
            ),
        }
    }
}

impl Default for Transform {
    fn default() -> Self {
        Transform(Similarity3::identity())
    }
}

impl Component for Transform {
    type Storage = FlaggedStorage<Self, DenseVecStorage<Self>>;
}

impl From<Vector3<f32>> for Transform {
    fn from(translation: Vector3<f32>) -> Self {
        Transform(Similarity3::new(translation, na::zero(), 0.0))
    }
}

impl<'de> Deserialize<'de> for Transform {
    fn deserialize<D>(deserializer: D) -> Result<Transform, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(field_identifier, rename_all = "snake_case")]
        enum Field {
            Translation,
            EulerRotation,
            QuaternionRotation,
            Scale,
        };

        struct TransformVisitor;

        impl<'de> Visitor<'de> for TransformVisitor {
            type Value = Transform;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("struct Transform")
            }

            fn visit_seq<V>(self, mut seq: V) -> Result<Self::Value, V::Error>
            where
                V: SeqAccess<'de>,
            {
                let translation: [f32; 3] = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(0, &self))?;
                let rotation: [f32; 4] = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(1, &self))?;
                let scale: f32 = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(2, &self))?;

                Ok(Transform(Similarity3::from_parts(
                    Translation3::new(translation[0], translation[1], translation[2]),
                    Unit::new_normalize(Quaternion::new(
                        rotation[0],
                        rotation[1],
                        rotation[2],
                        rotation[3],
                    )),
                    scale,
                )))
            }

            fn visit_map<V>(self, mut map: V) -> Result<Self::Value, V::Error>
            where
                V: MapAccess<'de>,
            {
                let mut translation = None;
                let mut rotation = None;
                let mut scale = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        Field::Translation => {
                            if translation.is_some() {
                                return Err(de::Error::duplicate_field("translation"));
                            }
                            translation = Some(map.next_value()?);
                        }
                        Field::EulerRotation => {
                            if rotation.is_some() {
                                return Err(de::Error::duplicate_field("rotation"));
                            }
                            let eulers: [f32; 3] = map.next_value()?;
                            rotation = Some(UnitQuaternion::from_euler_angles(
                                eulers[0], eulers[1], eulers[2],
                            ));
                        }
                        Field::QuaternionRotation => {
                            if rotation.is_some() {
                                return Err(de::Error::duplicate_field("rotation"));
                            }
                            let rotation_vals: [f32; 4] = map.next_value()?;
                            rotation = Some(UnitQuaternion::from_quaternion(Quaternion::new(
                                rotation_vals[0],
                                rotation_vals[1],
                                rotation_vals[2],
                                rotation_vals[3],
                            )));
                        }
                        Field::Scale => {
                            if scale.is_some() {
                                return Err(de::Error::duplicate_field("scale"));
                            }
                            scale = Some(map.next_value()?);
                        }
                    }
                }
                let translation: [f32; 3] = translation.unwrap_or([0.0; 3]);
                let rotation: UnitQuaternion<f32> = rotation.unwrap_or(UnitQuaternion::identity());
                let scale: f32 = scale.unwrap_or(1.0);

                let sim = Similarity3::from_parts(
                    Translation3::new(translation[0], translation[1], translation[2]),
                    rotation,
                    scale,
                );

                Ok(Transform(sim))
            }
        }

        const FIELDS: &'static [&'static str] = &["translation", "rotation", "scale"];
        deserializer.deserialize_struct("Transform", FIELDS, TransformVisitor)
    }
}

impl Serialize for Transform {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        #[derive(Serialize)]
        struct TransformValues {
            translation: [f32; 3],
            rotation: [f32; 4],
            scale: f32,
        }

        Serialize::serialize(
            &TransformValues {
                translation: self.0.isometry.translation.vector.into(),
                rotation: self.0.isometry.rotation.as_ref().coords.into(),
                scale: self.0.scaling(),
            },
            serializer,
        )
    }
}
