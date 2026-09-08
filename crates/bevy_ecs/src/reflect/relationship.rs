use crate::{
    bundle::{
        insert_relationship_in_bundle_writer, insert_relationship_target_in_bundle_writer,
        BundleWriter,
    },
    component::ComponentsRegistrator,
    entity::Entity,
    hierarchy::{ChildOf, Children},
    relationship::RelationshipTarget,
};
use bevy_reflect::CreateTypeData;

#[derive(Clone)]
pub struct ReflectRelationshipTarget {
    /// The function that will be called to add the relationship to the spawned related scene.
    pub insert_relationship:
        unsafe fn(&mut BundleWriter, &mut ComponentsRegistrator, target: Entity),
    /// The function that will be called to add the relationship target to the spawned scene with the given capacity.
    pub insert_relationship_target: unsafe fn(&mut BundleWriter, &mut ComponentsRegistrator, usize),
}

impl<T: RelationshipTarget> CreateTypeData<T> for ReflectRelationshipTarget {
    fn create_type_data(_input: ()) -> Self {
        ReflectRelationshipTarget {
            insert_relationship: insert_relationship_in_bundle_writer::<T::Relationship>,
            insert_relationship_target: insert_relationship_target_in_bundle_writer::<T>,
        }
    }
}
