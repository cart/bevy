//! Reconciliation is the process of incrementally updating entities, components,
//! and relationships from a [`Scene`] by storing state from previous reconciliations.
//!
//! When a scene is reconciled, it will:
//! - Remove components that were previously inserted during reconciliation but should no longer be present.
//!    - This includes components that were present in the previous bundle but absent in the new one
//!    - and components that were explicit in the previous bundle but implicit (required components) in the new one.
//! - Insert the new components onto the entity.
//!    - *Note*: There is no diffing of values involved here, components are re-inserted on every reconciliation.
//! - Map related entities to previously reconciled entities by their
//!   [`ReconcileAnchor`]s, or otherwise spawn new entities, as well as despawn any outdated related entities.
//! - Recursively reconcile related entities.
//! - Store the state of the reconciliation in a [`ReconcileReceipt`] component on the entity.
//!
//! # Caveats
//! - Currently not integrated with the deferred/async scene systems, all dependencies must be loaded before reconciliation.
//!
//! # Example
//! ```
//! use bevy_ecs::prelude::*;
//! use bevy_scene2::prelude::*;
//!
//! fn update_ui_system(ui_root: Single<Entity, With<UiRoot>>, mut commands: Commands) {
//!    commands.entity(*ui_root).reconcile_scene(bsn! {
//!        Node [
//!            #NamedEntity Text("This child will be mapped to/recycle the same entity (among siblings) on every reconciliation"),
//!            Text("This child will recycle an existing unnamed entity if it exists")
//!        ],
//!    });
//! }
//! ```
use core::any::TypeId;
use indexmap::IndexMap;

use bevy_asset::{AssetServer, Assets};
use bevy_ecs::{
    bundle::BundleId,
    component::{Component, ComponentId},
    entity::Entity,
    error::{warn, Result},
    name::Name,
    system::EntityCommands,
    template::{EntityScopes, ScopedEntities, TemplateContext},
    world::{EntityWorldMut, Mut, World},
};
use bevy_platform::collections::HashSet;
use bevy_utils::TypeIdMap;

use crate::{PatchContext, ResolvedRelatedScenes, ResolvedScene, Scene, ScenePatch};

/// An identifier for related entities during scene reconciliation.
#[derive(Hash, Eq, PartialEq, Clone, Debug)]
pub enum ReconcileAnchor {
    /// The entity uses an automatic incrementing ID.
    Auto(u64),
    /// The entity has been explicitly keyed with a [`Name`].
    Named(Name),
}

/// Holds state for scene reconciliation, tracking the previous bundle and related entities.
#[derive(Component, Default, Clone, Debug)]
pub struct ReconcileReceipt {
    /// Id of the bundle that was inserted on the previous reconciliation.
    pub bundle_id: Option<BundleId>,
    /// The anchors of the related entities on the previous reconciliation.
    pub anchors: TypeIdMap<IndexMap<ReconcileAnchor, Entity>>,
}

impl ReconcileReceipt {
    fn take_or_default(entity: &mut EntityWorldMut) -> Self {
        entity
            .get_mut::<ReconcileReceipt>()
            .map_or_else(ReconcileReceipt::default, |mut r| {
                core::mem::take(r.as_mut())
            })
    }
}

pub trait EntityCommandsReconcileScene {
    /// Reconciles a scene on the entity.
    ///
    /// This will emit a warning if any templates fail to be applied.
    ///
    /// See [`crate::reconcile`] for more details on reconciliation.
    ///
    /// # Note
    /// - This is a synchronous operation and any dependencies
    ///   of the scene must be loaded before calling this method.
    /// - Unlike [`crate::SpawnScene::spawn_scene`], this method will not insert a [`crate::ScenePatchInstance`] component
    ///
    fn reconcile_scene<S: Scene>(&mut self, scene: S) -> &mut Self;
}

impl EntityCommandsReconcileScene for EntityCommands<'_> {
    fn reconcile_scene<S: Scene>(&mut self, scene: S) -> &mut Self {
        self.queue_handled(
            |mut entity: EntityWorldMut| -> Result {
                entity.reconcile_scene(scene)?;
                Ok(())
            },
            warn,
        )
    }
}

pub trait ReconcileScene {
    /// Reconciles the given scene on the entity.
    ///
    /// See [`crate::reconcile`] for more details on reconciliation.
    ///
    /// # Note
    /// - This is a synchronous operation and any dependencies
    ///   of the scene must be loaded before calling this method.
    /// - Unlike [`crate::SpawnScene::spawn_scene`], this method will not insert a [`crate::ScenePatchInstance`] component
    ///
    fn reconcile_scene<S: Scene>(&mut self, scene: S) -> Result<&mut Self>;
}

impl<'w> ReconcileScene for EntityWorldMut<'w> {
    fn reconcile_scene<S: Scene>(&mut self, scene: S) -> Result<&mut Self> {
        // Recursively patch the scenes onto a resolved scene
        let mut resolved_scene = ResolvedScene::default();
        let mut entity_scopes = EntityScopes::default();
        self.world_scope(|world| {
            world.resource_scope(|world, assets: Mut<AssetServer>| {
                scene.patch(
                    &mut PatchContext {
                        assets: &assets,
                        patches: world.resource::<Assets<ScenePatch>>(),
                        current_scope: entity_scopes.add_scope(),
                        entity_scopes: &mut entity_scopes,
                    },
                    &mut resolved_scene,
                );
            });
        });

        // Walk the scene to reconcile entities, spawning/despawning as needed, and build up the anchors
        let mut scoped_entities = ScopedEntities::new(entity_scopes.entity_count());
        reconcile_entities(&resolved_scene, self, &mut scoped_entities, &entity_scopes);

        // Apply the templates and relationships to the entities recursively
        reconcile_apply(
            &mut resolved_scene,
            self,
            &mut scoped_entities,
            &entity_scopes,
        )?;

        Ok(self)
    }
}

fn reconcile_entities(
    scene: &ResolvedScene,
    entity: &mut EntityWorldMut,
    scoped_entities: &mut ScopedEntities,
    entity_scopes: &EntityScopes,
) {
    let mut receipt = ReconcileReceipt::take_or_default(entity);

    entity.world_scope(|world| {
        // Reconcile new/updated related entities
        for (type_id, related) in scene.related.iter() {
            reconcile_related_entities(
                *type_id,
                related,
                &mut receipt,
                entity_scopes,
                scoped_entities,
                world,
            );
        }

        // Despawn any leftover orphans from outdated relationships
        for (type_id, anchors) in receipt.anchors.iter_mut() {
            if !scene.related.contains_key(type_id) {
                for (_, orphan_id) in anchors.drain(..) {
                    if let Ok(entity) = world.get_entity_mut(orphan_id) {
                        entity.despawn();
                    }
                }
            }
        }
    });

    entity.insert(receipt);
}

fn reconcile_related_entities(
    type_id: TypeId,
    related: &ResolvedRelatedScenes,
    receipt: &mut ReconcileReceipt,
    entity_scopes: &EntityScopes,
    scoped_entities: &mut ScopedEntities,
    world: &mut World,
) {
    // TODO: A bit wasteful to allocate a new IndexMap here each time
    let mut previous_anchors = receipt.anchors.remove(&type_id).unwrap_or_default();
    let receipt_anchors = receipt
        .anchors
        .entry(type_id)
        .or_insert_with(|| IndexMap::with_capacity(related.scenes.len()));

    let mut i = 0;
    for related_scene in related.scenes.iter() {
        // Compute the anchor for this scene, using it's name if supplied
        // or an auto-incrementing counter if not.
        let name_index = related_scene
            .template_indices
            .get(&TypeId::of::<Name>())
            .copied();
        let anchor = match name_index {
            Some(name_index) => ReconcileAnchor::Named(
                // TODO: Sanity check for duplicate names
                related_scene.templates[name_index]
                    .downcast_ref::<Name>()
                    .unwrap()
                    .clone(),
            ),
            None => {
                let anchor = ReconcileAnchor::Auto(i);
                i += 1;
                anchor
            }
        };

        // Find the existing related entity based on the anchor, or spawn a
        // new one.
        let entity_id = previous_anchors
            .shift_remove(&anchor)
            .unwrap_or_else(|| world.spawn_empty().id());

        // Update scoped entities to ensure that entity references point correctly and does not spawn duplicates
        // TODO: Share name anchors inside the same scope, to allow moving entities around while preserving identity/state
        if let Some((scope, index)) = related_scene.entity_references.first().copied() {
            scoped_entities.set(entity_scopes, scope, index, entity_id);
        }

        // Add the anchor and entity id to the receipt
        receipt_anchors.insert(anchor, entity_id);
    }

    // Clear any remaining orphans
    for orphan_id in previous_anchors.into_values() {
        if let Ok(entity) = world.get_entity_mut(orphan_id) {
            entity.despawn();
        }
    }

    // Reconcile the related entities
    for (related_scene, entity_id) in related.scenes.iter().zip(receipt_anchors.values()) {
        let mut entity = world.entity_mut(*entity_id);
        reconcile_entities(related_scene, &mut entity, scoped_entities, entity_scopes);
    }
}

fn reconcile_apply(
    scene: &mut ResolvedScene,
    entity: &mut EntityWorldMut,
    scoped_entities: &mut ScopedEntities,
    entity_scopes: &EntityScopes,
) -> Result {
    // Take the receipt from the targeted entity using core::mem::take to avoid archetype moves
    let mut receipt = ReconcileReceipt::take_or_default(entity);

    let entity_id = entity.id();

    // Diff/remove components
    entity.world_scope(|world| {
        // Collect all the component IDs that will be inserted by the templates
        // TODO: Optimize?
        let mut component_ids = Vec::with_capacity(scene.templates.len());
        for template in scene.templates.iter_mut() {
            if let Some(bundle_info) = template.register_bundle(world) {
                component_ids.extend(bundle_info.iter_explicit_components());
            }
        }

        // Get the bundle ID of the new component set
        let bundle_id = world.register_dynamic_bundle(&component_ids).id();

        // Remove the components that are no longer needed
        if let Some(prev_bundle_id) = receipt.bundle_id {
            remove_components_incremental(world, entity_id, prev_bundle_id, bundle_id);
        }

        receipt.bundle_id = Some(bundle_id);
    });

    // Apply the templates to the entity
    // TODO: Insert as dynamic bundle to avoid archetype moves / incorrect hook/observer behavior
    for template in scene.templates.iter_mut() {
        template.apply(&mut TemplateContext {
            entity,
            scoped_entities,
            entity_scopes,
        })?;
    }

    entity.world_scope(|world| -> Result {
        // Reconcile components of related entities
        for (type_id, related) in scene.related.iter_mut() {
            let receipt_anchors = receipt.anchors.get(type_id).unwrap();

            // Insert the relationships
            for related_entity in receipt_anchors.values() {
                let mut entity = world.entity_mut(*related_entity);
                (related.insert)(&mut entity, entity_id);
            }

            // Recursively reconcile the components/relationships of related entities
            for (related_scene, entity_id) in
                related.scenes.iter_mut().zip(receipt_anchors.values())
            {
                let mut entity = world.entity_mut(*entity_id);
                reconcile_apply(related_scene, &mut entity, scoped_entities, entity_scopes)?;
            }
        }

        Ok(())
    })?;

    // (Re)Insert the receipt on the entity
    entity.insert(receipt);

    Ok(())
}

/// Removes components that should no longer be present when replacing a previous bundle with a new one:
///  - Components that were present in the previous bundle, but absent in the new one
///  - Components that were explicit in the previous bundle, but required (implicit) in the new one
///
/// Panics if any of the [`BundleId`]s are not registered in the world.
fn remove_components_incremental(
    world: &mut World,
    entity_id: Entity,
    prev_bundle_id: BundleId,
    new_bundle_id: BundleId,
) {
    // Compare the previous bundle with the new bundle to determine which components to remove
    // TODO: Optimize to avoid mass heap allocations
    let (new_contributed, new_required) = {
        let new_bundle_info = world
            .bundles()
            .get(new_bundle_id)
            .expect("new bundle should be registered");
        let new_contributed: HashSet<ComponentId> =
            new_bundle_info.iter_contributed_components().collect();
        let new_required: HashSet<ComponentId> =
            new_bundle_info.iter_required_components().collect();
        (new_contributed, new_required)
    };
    let prev_bundle_info = world
        .bundles()
        .get(prev_bundle_id)
        .expect("previous bundle should be registered");
    let prev_explicit: HashSet<ComponentId> = prev_bundle_info.iter_explicit_components().collect();

    let removed_components: Vec<ComponentId> = prev_bundle_info
        .iter_contributed_components()
        .filter(|id| {
            !new_contributed.contains(id)
                || (prev_explicit.contains(id) && new_required.contains(id))
        })
        .collect();

    // Remove the components that are no longer needed.
    let mut entity = world.entity_mut(entity_id);
    entity.remove_by_ids(&removed_components);
}
