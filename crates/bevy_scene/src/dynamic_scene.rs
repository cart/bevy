use crate::{
    asset_loader::BsnToSceneError, NameEntityReference, RelatedResolvedScenes, ResolveContext,
    ResolveSceneError, ResolvedScene, Scene, ScenePatch,
};
use bevy_asset::AssetPath;
use bevy_bsn::types::{BsnFields, BsnType, BsnValue};
use bevy_ecs::reflect::{IntoErasedFn, ReflectErasedFn, ReflectRelationshipTarget};
use bevy_reflect::{ApplyError, PartialReflect, Reflect, ReflectMut, TypeInfo, TypeInfoError};
use std::any::{type_name_of_val, TypeId};
use thiserror::Error;

#[derive(Default)]
pub struct DynamicScene {
    pub entries: Vec<DynamicSceneEntry>,
}

impl Scene for DynamicScene {
    fn resolve(
        self,
        context: &mut ResolveContext,
        scene: &mut ResolvedScene,
    ) -> bevy_ecs::error::Result<(), ResolveSceneError> {
        for entry in self.entries {
            match entry {
                DynamicSceneEntry::Name(name_entity_reference) => {
                    name_entity_reference.resolve(context, scene)?;
                }
                DynamicSceneEntry::DynamicPatch(dynamic_patch) => {
                    dynamic_patch.resolve(context, scene)?;
                }
                DynamicSceneEntry::RelatedEntities(dynamic_related_entities) => {
                    dynamic_related_entities.resolve(context, scene)?;
                }
                DynamicSceneEntry::SceneAsset(asset_path) => {
                    if let Some(handle) = context.assets.get_handle::<ScenePatch>(&asset_path)
                        && let Some(scene_patch) = context.patches.get(&handle)
                    {
                        scene.include_cached(handle)?;
                        context.cached = Some(scene_patch);
                    } else {
                        return Err(ResolveSceneError::MissingSceneDependency(asset_path));
                    }
                }
            }
        }
        Ok(())
    }

    fn register_dependencies(&self, dependencies: &mut crate::SceneDependencies) {
        for entry in &self.entries {
            match entry {
                DynamicSceneEntry::SceneAsset(path) => {
                    dependencies.register::<ScenePatch>(path.clone());
                }
                DynamicSceneEntry::RelatedEntities(dynamic_related_scenes) => {
                    dynamic_related_scenes.register_dependencies(dependencies);
                }
                _ => {}
            }
        }
    }
}

pub enum DynamicSceneEntry {
    Name(NameEntityReference),
    DynamicPatch(DynamicPatch),
    RelatedEntities(DynamicRelatedScenes),
    SceneAsset(AssetPath<'static>),
}

pub struct DynamicPatch {
    // PERF: to save on memory here we could try to construct this vtable statically and store a static ref?
    pub type_id: TypeId,
    pub into_erased: IntoErasedFn,
    pub reflect_erased: ReflectErasedFn,
    pub default: fn() -> Box<dyn Reflect>,
    pub patch_type: DynamicPatchType,
}

impl Scene for DynamicPatch {
    fn resolve(
        self,
        context: &mut ResolveContext,
        scene: &mut ResolvedScene,
    ) -> bevy_ecs::error::Result<(), ResolveSceneError> {
        let erased = scene.get_or_insert_erased_template(context, self.type_id, || {
            let template = (self.default)();
            (self.into_erased)(template)
        });
        let reflect = (self.reflect_erased)(erased);
        self.patch_type.apply_to_reflect(reflect)?;
        Ok(())
    }
}

pub enum DynamicPatchType {
    Struct { fields: Vec<DynamicFieldPatch> },
    TupleStruct { fields: Vec<DynamicValue> },
}

impl DynamicPatchType {
    pub(crate) fn from_bsn_type(
        bsn_type: &BsnType,
        type_info: &'static TypeInfo,
    ) -> Result<Self, BsnToDynamicValueError> {
        Ok(match &bsn_type.fields {
            BsnFields::NamedFields(bsn_named_fields) => {
                let struct_info = type_info.as_struct().map_err(|error| {
                    BsnToDynamicValueError::MismatchedPatchType {
                        type_path: type_info.type_path(),
                        error,
                    }
                })?;
                let mut fields = Vec::new();
                for field in bsn_named_fields {
                    let field_index = struct_info.index_of(&field.name).ok_or_else(|| {
                        BsnToDynamicValueError::InvalidStructField {
                            type_path: type_info.type_path(),
                            field: field.name.to_string(),
                        }
                    })?;
                    // unwrap is ok because we just looked up the field index
                    let field_info = struct_info.field_at(field_index).unwrap();
                    fields.push(DynamicFieldPatch {
                        index: field_index,
                        value: DynamicValue::from_bsn(
                            &field.value,
                            field_info.type_info().unwrap(),
                        )
                        .map_err(|e| {
                            BsnToDynamicValueError::StructNamedFieldValueError {
                                type_path: type_info.type_path(),
                                field: field_info.name(),
                                error: Box::new(e),
                            }
                        })?,
                    });
                }
                DynamicPatchType::Struct { fields }
            }
            BsnFields::UnnamedFields(bsn_values) => {
                let struct_info = type_info.as_tuple_struct().map_err(|error| {
                    BsnToDynamicValueError::MismatchedPatchType {
                        type_path: type_info.type_path(),
                        error,
                    }
                })?;
                let mut fields = Vec::new();
                for (index, bsn_value) in bsn_values.iter().enumerate() {
                    let field_info = struct_info.field_at(index).ok_or_else(|| {
                        BsnToDynamicValueError::InvalidFieldIndex {
                            type_path: type_info.type_path(),
                            index,
                        }
                    })?;
                    fields.push(
                        DynamicValue::from_bsn(bsn_value, field_info.type_info().unwrap())
                            .map_err(|e| BsnToDynamicValueError::StructUnnamedFieldValueError {
                                type_path: type_info.type_path(),
                                field: index,
                                error: Box::new(e),
                            })?,
                    );
                }
                DynamicPatchType::TupleStruct { fields }
            }
            BsnFields::Unit => DynamicPatchType::Struct { fields: Vec::new() },
        })
    }

    fn apply_to_reflect(
        &self,
        reflect: &mut dyn PartialReflect,
    ) -> Result<(), ApplyDynamicValueError> {
        let type_info = reflect.get_represented_type_info().unwrap();
        match (reflect.reflect_mut(), self) {
            (ReflectMut::Struct(reflect), DynamicPatchType::Struct { fields }) => {
                for field_patch in fields {
                    let reflect_field =
                        reflect.field_at_mut(field_patch.index).ok_or_else(|| {
                            ApplyDynamicValueError::InvalidReflectField {
                                index: field_patch.index,
                                type_path: type_info.type_path(),
                            }
                        })?;
                    field_patch
                        .value
                        .apply_to_reflect(reflect_field)
                        .map_err(|error| ApplyDynamicValueError::FailedReflectFieldApply {
                            index: field_patch.index,
                            type_path: type_info.type_path(),
                            error: Box::new(error),
                        })?;
                }
            }
            (ReflectMut::TupleStruct(reflect), DynamicPatchType::TupleStruct { fields }) => {
                for (index, field_patch) in fields.iter().enumerate() {
                    let reflect_field = reflect.field_mut(index).ok_or_else(|| {
                        ApplyDynamicValueError::InvalidReflectField {
                            index,
                            type_path: type_info.type_path(),
                        }
                    })?;
                    field_patch
                        .apply_to_reflect(reflect_field)
                        .map_err(|error| ApplyDynamicValueError::FailedReflectFieldApply {
                            index,
                            type_path: type_info.type_path(),
                            error: Box::new(error),
                        })?;
                }
            }
            _ => {
                // TODO: error message here!
            }
        }
        Ok(())
    }
}

pub struct DynamicFieldPatch {
    pub index: usize,
    pub value: DynamicValue,
}

pub enum DynamicValue {
    Bool(bool),
    F32(f32),
    F64(f64),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    U128(u128),
    USize(usize),
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    I128(i128),
    ISize(isize),
    String(String),
    Type(DynamicPatchType),
}

#[derive(Error, Debug)]
pub enum BsnToDynamicValueError {
    #[error("Failed to convert BSN value to a scene patch value. Expected type: {expected}. Actual type: {actual}.")]
    MismatchedValue {
        expected: &'static str,
        actual: &'static str,
    },
    #[error("Failed to convert BSN struct of type \"{type_path}\" to a scene patch value because field \"{field}\" encountered the following error: {error}")]
    StructNamedFieldValueError {
        type_path: &'static str,
        field: &'static str,
        error: Box<BsnToDynamicValueError>,
    },
    #[error("Failed to convert BSN struct of type \"{type_path}\" to a scene patch value because field \"{field}\" encountered the following error: {error}")]
    StructUnnamedFieldValueError {
        type_path: &'static str,
        field: usize,
        error: Box<BsnToDynamicValueError>,
    },
    #[error("Patch for {type_path} has a mismatched type: {error}")]
    MismatchedPatchType {
        type_path: &'static str,
        error: TypeInfoError,
    },
    #[error("Patch for {type_path} references the invalid field \"{field}\"")]
    InvalidStructField {
        type_path: &'static str,
        field: String,
    },
    #[error("Patch for {type_path} references the invalid field at index {index}")]
    InvalidFieldIndex {
        type_path: &'static str,
        index: usize,
    },
}

#[derive(Error, Debug)]
pub enum ApplyDynamicValueError {
    #[error(transparent)]
    ApplyError(#[from] ApplyError),
    #[error("Reflect field index {index} is invalid for {type_path}")]
    InvalidReflectField {
        index: usize,
        type_path: &'static str,
    },
    #[error(
        "Failed to apply dynamic patch value to field index {index} on type {type_path}: {error}"
    )]
    FailedReflectFieldApply {
        index: usize,
        type_path: &'static str,
        error: Box<ApplyDynamicValueError>,
    },
}
impl DynamicValue {
    pub fn from_bsn(
        value: &BsnValue,
        type_info: &'static TypeInfo,
    ) -> Result<Self, BsnToDynamicValueError> {
        Ok(match value {
            BsnValue::Bool(value) => {
                if type_info.type_id() == TypeId::of::<bool>() {
                    DynamicValue::Bool(*value)
                } else {
                    return Err(BsnToDynamicValueError::MismatchedValue {
                        expected: type_info.type_path(),
                        actual: type_name_of_val(value),
                    });
                }
            }
            BsnValue::Float(value) => {
                if type_info.type_id() == TypeId::of::<f32>() {
                    DynamicValue::F32(*value as f32)
                } else if type_info.type_id() == TypeId::of::<f64>() {
                    DynamicValue::F64(*value)
                } else {
                    return Err(BsnToDynamicValueError::MismatchedValue {
                        expected: type_info.type_path(),
                        actual: type_name_of_val(value),
                    });
                }
            }
            BsnValue::Int(value) => {
                if type_info.type_id() == TypeId::of::<usize>() {
                    DynamicValue::USize(*value as usize)
                } else if type_info.type_id() == TypeId::of::<u8>() {
                    DynamicValue::U8(*value as u8)
                } else if type_info.type_id() == TypeId::of::<u16>() {
                    DynamicValue::U16(*value as u16)
                } else if type_info.type_id() == TypeId::of::<u32>() {
                    DynamicValue::U32(*value as u32)
                } else if type_info.type_id() == TypeId::of::<u64>() {
                    DynamicValue::U64(*value as u64)
                } else if type_info.type_id() == TypeId::of::<u128>() {
                    DynamicValue::U128(*value as u128)
                } else if type_info.type_id() == TypeId::of::<isize>() {
                    DynamicValue::ISize(*value as isize)
                } else if type_info.type_id() == TypeId::of::<i8>() {
                    DynamicValue::I8(*value as i8)
                } else if type_info.type_id() == TypeId::of::<i16>() {
                    DynamicValue::I16(*value as i16)
                } else if type_info.type_id() == TypeId::of::<i32>() {
                    DynamicValue::I32(*value as i32)
                } else if type_info.type_id() == TypeId::of::<i64>() {
                    DynamicValue::I64(*value as i64)
                } else if type_info.type_id() == TypeId::of::<i128>() {
                    DynamicValue::I128(*value as i128)
                } else {
                    return Err(BsnToDynamicValueError::MismatchedValue {
                        expected: type_info.type_path(),
                        actual: type_name_of_val(value),
                    });
                }
            }
            BsnValue::String(value) => {
                if type_info.type_id() == TypeId::of::<String>() {
                    DynamicValue::String(value.clone())
                } else {
                    return Err(BsnToDynamicValueError::MismatchedValue {
                        expected: type_info.type_path(),
                        actual: type_name_of_val(value),
                    });
                }
            }
            BsnValue::Type(value) => {
                DynamicValue::Type(DynamicPatchType::from_bsn_type(value, type_info)?)
            }
        })
    }

    pub fn apply_to_reflect(
        &self,
        partial_reflect: &mut dyn PartialReflect,
    ) -> Result<(), ApplyDynamicValueError> {
        match self {
            DynamicValue::Bool(value) => {
                partial_reflect.try_apply(value)?;
            }
            DynamicValue::F32(value) => {
                partial_reflect.try_apply(value)?;
            }
            DynamicValue::F64(value) => {
                partial_reflect.try_apply(value)?;
            }
            DynamicValue::USize(value) => {
                partial_reflect.try_apply(value)?;
            }
            DynamicValue::ISize(value) => {
                partial_reflect.try_apply(value)?;
            }
            DynamicValue::U8(value) => {
                partial_reflect.try_apply(value)?;
            }
            DynamicValue::U16(value) => {
                partial_reflect.try_apply(value)?;
            }
            DynamicValue::U32(value) => {
                partial_reflect.try_apply(value)?;
            }
            DynamicValue::U64(value) => {
                partial_reflect.try_apply(value)?;
            }
            DynamicValue::U128(value) => {
                partial_reflect.try_apply(value)?;
            }
            DynamicValue::I8(value) => {
                partial_reflect.try_apply(value)?;
            }
            DynamicValue::I16(value) => {
                partial_reflect.try_apply(value)?;
            }
            DynamicValue::I32(value) => {
                partial_reflect.try_apply(value)?;
            }
            DynamicValue::I64(value) => {
                partial_reflect.try_apply(value)?;
            }
            DynamicValue::I128(value) => {
                partial_reflect.try_apply(value)?;
            }
            DynamicValue::String(value) => {
                partial_reflect.try_apply(value)?;
            }
            DynamicValue::Type(value) => value.apply_to_reflect(partial_reflect)?,
        }
        Ok(())
    }
}

pub struct DynamicRelatedScenes {
    pub scenes: Vec<DynamicScene>,
    pub reflect_relationship_target: ReflectRelationshipTarget,
    pub relationship_target_type_id: TypeId,
    pub relationship_target_name: &'static str,
}

impl Scene for DynamicRelatedScenes {
    fn resolve(
        self,
        context: &mut ResolveContext,
        scene: &mut ResolvedScene,
    ) -> bevy_ecs::error::Result<(), ResolveSceneError> {
        let related = scene
            .related
            .entry(self.relationship_target_type_id)
            .or_insert_with(|| RelatedResolvedScenes {
                insert_relationship: self.reflect_relationship_target.insert_relationship,
                insert_relationship_target: self
                    .reflect_relationship_target
                    .insert_relationship_target,
                relationship_target_name: self.relationship_target_name,
                scenes: Vec::with_capacity(self.scenes.len()),
            });
        for scene in self.scenes {
            let mut resolved_scene = ResolvedScene::default();
            scene.resolve(context, &mut resolved_scene)?;
            related.scenes.push(resolved_scene);
        }
        Ok(())
    }

    fn register_dependencies(&self, dependencies: &mut crate::SceneDependencies) {
        for related in &self.scenes {
            related.register_dependencies(dependencies);
        }
    }
}
