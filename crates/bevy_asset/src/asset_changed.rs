//! Defines the [`AssetChanged`] query filter.
//!
//! Like [`Changed`](bevy_ecs::prelude::Changed), but for [`Asset`]s,
//! and triggers whenever the handle or the underlying asset changes.

use crate::AsAssetId;
use bevy_ecs::change_detection::DetectChanges;
use bevy_ecs::component::Components;
use bevy_ecs::query::NestedQuery;
use bevy_ecs::world::Ref;
use bevy_ecs::{
    archetype::Archetype,
    change_detection::Tick,
    component::ComponentId,
    prelude::{Entity, World},
    query::{FilteredAccess, FilteredAccessSet, QueryData, QueryFilter, WorldQuery},
    storage::{Table, TableRow},
    world::unsafe_world_cell::UnsafeWorldCell,
};
use core::marker::PhantomData;

/// Filter that selects entities with an `A` for an asset that changed
/// after the system last ran, where `A` is a component that implements
/// [`AsAssetId`].
///
/// Unlike `Changed<A>`, this is true whenever the asset for the `A`
/// in `ResMut<Assets<A>>` changed. For example, when a mesh changed through the
/// [`Assets<Mesh>::get_mut`] method, `AssetChanged<Mesh>` will iterate over all
/// entities with the `Handle<Mesh>` for that mesh. Meanwhile, `Changed<Handle<Mesh>>`
/// will iterate over no entities.
///
/// Swapping the actual `A` component is a common pattern. So you
/// should check for _both_ `AssetChanged<A>` and `Changed<A>` with
/// `Or<(Changed<A>, AssetChanged<A>)>`.
///
/// # Quirks
///
/// - Asset changes are registered in the [`AssetEventSystems`] system set.
/// - Removed assets are not detected.
///
/// The list of changed assets only gets updated in the [`AssetEventSystems`] system set,
/// which runs in `PostUpdate`. Therefore, `AssetChanged` will only pick up asset changes in schedules
/// following [`AssetEventSystems`] or the next frame. Consider adding the system in the `Last` schedule
/// after [`AssetEventSystems`] if you need to react without frame delay to asset changes.
///
/// # Performance
///
/// When at least one `A` is updated, this will
/// read a hashmap once per entity with an `A` component. The
/// runtime of the query is proportional to how many entities with an `A`
/// it matches.
///
/// If no `A` asset updated since the last time the system ran, then no lookups occur.
///
/// [`AssetEventSystems`]: crate::AssetEventSystems
/// [`Assets<Mesh>::get_mut`]: crate::Assets::get_mut
pub struct AssetChanged<A: AsAssetId>(PhantomData<A>);

type AssetChangedInner<A> = (
    &'static A,
    NestedQuery<Ref<'static, <A as AsAssetId>::Asset>>,
);

#[expect(unsafe_code, reason = "WorldQuery is an unsafe trait.")]
// SAFETY: `ROQueryFetch<Self>` is the same as `QueryFetch<Self>`
unsafe impl<A: AsAssetId> WorldQuery for AssetChanged<A> {
    type Fetch<'w> = (<AssetChangedInner<A> as WorldQuery>::Fetch<'w>, Tick);
    type State = <AssetChangedInner<A> as WorldQuery>::State;

    fn shrink_fetch<'wlong: 'wshort, 'wshort>(fetch: Self::Fetch<'wlong>) -> Self::Fetch<'wshort> {
        let (fetch, last_run) = fetch;
        let fetch = <AssetChangedInner<A> as WorldQuery>::shrink_fetch(fetch);
        (fetch, last_run)
    }

    unsafe fn init_fetch<'w, 's>(
        world: UnsafeWorldCell<'w>,
        state: &'s Self::State,
        last_run: Tick,
        this_run: Tick,
    ) -> Self::Fetch<'w> {
        // SAFETY: All safety requirements are satisfied by the caller.
        let fetch = unsafe {
            <AssetChangedInner<A> as WorldQuery>::init_fetch(world, state, last_run, this_run)
        };
        (fetch, last_run)
    }

    const IS_DENSE: bool = <AssetChangedInner<A> as WorldQuery>::IS_DENSE;

    unsafe fn set_archetype<'w, 's>(
        fetch: &mut Self::Fetch<'w>,
        state: &'s Self::State,
        archetype: &'w Archetype,
        table: &'w Table,
    ) {
        // SAFETY: All safety requirements are satisfied by the caller.
        unsafe {
            <AssetChangedInner<A> as WorldQuery>::set_archetype(
                &mut fetch.0,
                state,
                archetype,
                table,
            )
        }
    }

    unsafe fn set_table<'w, 's>(
        fetch: &mut Self::Fetch<'w>,
        state: &Self::State,
        table: &'w Table,
    ) {
        // SAFETY: All safety requirements are satisfied by the caller.
        unsafe { <AssetChangedInner<A> as WorldQuery>::set_table(&mut fetch.0, state, table) }
    }

    #[inline]
    fn update_component_access(state: &Self::State, access: &mut FilteredAccess) {
        <AssetChangedInner<A> as WorldQuery>::update_component_access(state, access)
    }

    // ChangedAsset accesses both the asset and the AssetChanges<A> resource.
    // In order to access two different entities we implement init_nested_access.
    fn init_nested_access(
        state: &Self::State,
        system_name: Option<&str>,
        component_access_set: &mut FilteredAccessSet,
        world: UnsafeWorldCell,
    ) {
        <AssetChangedInner<A> as WorldQuery>::init_nested_access(
            state,
            system_name,
            component_access_set,
            world,
        )
    }

    fn init_state(world: &mut World) -> Self::State {
        <AssetChangedInner<A> as WorldQuery>::init_state(world)
    }

    fn get_state(components: &Components) -> Option<Self::State> {
        <AssetChangedInner<A> as WorldQuery>::get_state(components)
    }

    fn matches_component_set(
        state: &Self::State,
        set_contains_id: &impl Fn(ComponentId) -> bool,
    ) -> bool {
        <AssetChangedInner<A> as WorldQuery>::matches_component_set(state, set_contains_id)
    }

    fn update_archetypes(state: &mut Self::State, world: UnsafeWorldCell) {
        <AssetChangedInner<A> as WorldQuery>::update_archetypes(state, world);
    }
}

#[expect(unsafe_code, reason = "QueryFilter is an unsafe trait.")]
// SAFETY: read-only access
unsafe impl<A: AsAssetId> QueryFilter for AssetChanged<A> {
    const IS_ARCHETYPAL: bool = false;

    #[inline]
    unsafe fn filter_fetch(
        state: &Self::State,
        fetch: &mut Self::Fetch<'_>,
        entity: Entity,
        table_row: TableRow,
    ) -> bool {
        let (fetch, last_run) = fetch;
        // SAFETY: All safety requirements are satisfied by the caller.
        let fetch =
            unsafe { <AssetChangedInner<A> as QueryData>::fetch(state, fetch, entity, table_row) };

        let Some((component, asset_query)) = fetch else {
            return false;
        };
        let id = component.as_asset_id();
        let Ok(asset_ref) = asset_query.get(id) else {
            return false;
        };

        asset_ref.is_changed_after(*last_run)
    }
}

#[cfg(test)]
#[expect(clippy::print_stdout, reason = "Allowed in tests.")]
mod tests {
    use crate::direct_access_ext::AssetCommands;
    use crate::tests::create_app;
    use crate::{AssetEventSystems, AssetId, Handle};
    use alloc::{vec, vec::Vec};
    use bevy_asset_macros::Asset;
    use bevy_ecs::system::assert_is_system;
    use core::num::NonZero;
    use std::println;

    use crate::AssetApp;
    use bevy_app::{App, AppExit, PostUpdate, Startup, Update};
    use bevy_ecs::schedule::IntoScheduleConfigs;
    use bevy_ecs::{
        component::Component,
        message::MessageWriter,
        resource::Resource,
        system::{Commands, IntoSystem, Local, Query, ResMut},
    };
    use bevy_reflect::TypePath;

    use super::*;

    #[derive(Asset, TypePath, Debug)]
    struct MyAsset(usize, &'static str);

    #[derive(Component)]
    struct MyComponent(Handle<MyAsset>);

    impl AsAssetId for MyComponent {
        type Asset = MyAsset;

        fn as_asset_id(&self) -> AssetId<Self::Asset> {
            self.0.id()
        }
    }

    #[test]
    #[should_panic]
    fn should_conflict() {
        #[derive(Component)]
        struct Foo;

        fn system(
            _: Query<&Foo, AssetChanged<MyComponent>>,
            _: Query<&mut MyAsset, bevy_ecs::query::Without<Foo>>,
        ) {
        }
        assert_is_system(system);
    }

    fn run_app<Marker>(system: impl IntoSystem<(), (), Marker>) {
        let mut app = create_app().0;
        app.init_asset::<MyAsset>().add_systems(Update, system);
        app.update();
    }

    // According to a comment in QueryState::new in bevy_ecs, components on filter
    // position shouldn't conflict with components on query position.
    #[test]
    fn handle_filter_pos_ok() {
        fn compatible_filter(
            _query: Query<&mut MyComponent, AssetChanged<MyComponent>>,
            mut exit: MessageWriter<AppExit>,
        ) {
            exit.write(AppExit::Error(NonZero::<u8>::MIN));
        }
        run_app(compatible_filter);
    }

    #[derive(Default, PartialEq, Debug, Resource)]
    struct Counter(Vec<u32>);

    fn count_update(
        mut counter: ResMut<Counter>,
        assets: Query<&MyAsset>,
        query: Query<&MyComponent, AssetChanged<MyComponent>>,
    ) {
        for my_component in query.iter() {
            let asset = assets.get(my_component.0.entity()).unwrap();
            counter.0[asset.0] += 1;
        }
    }

    fn update_some(mut assets: Query<(Entity, &mut MyAsset)>, mut run_count: Local<u32>) {
        let mut update_index = |i| {
            let id = assets
                .iter()
                .find_map(|(h, a)| (a.0 == i).then_some(h))
                .unwrap();
            let (_, mut asset) = assets.get_mut(id).unwrap();
            println!("setting new value for {}", asset.0);
            asset.1 = "new_value";
        };
        match *run_count {
            0 | 1 => update_index(0),
            2 => {}
            3 => {
                update_index(0);
                update_index(1);
            }
            4.. => update_index(1),
        };
        *run_count += 1;
    }

    fn add_some(mut cmds: Commands, mut run_count: Local<u32>) {
        match *run_count {
            1 => {
                let asset = cmds.spawn_asset(MyAsset(0, "init"));
                cmds.spawn(MyComponent(asset));
            }
            0 | 2 => {}
            3 => {
                let asset1 = cmds.spawn_asset(MyAsset(1, "init"));
                let asset2 = cmds.spawn_asset(MyAsset(2, "init"));
                cmds.spawn(MyComponent(asset1));
                cmds.spawn(MyComponent(asset2));
            }
            4.. => {
                let asset = cmds.spawn_asset(MyAsset(3, "init"));
                cmds.spawn(MyComponent(asset));
            }
        };
        *run_count += 1;
    }

    #[track_caller]
    fn assert_counter(app: &App, assert: Counter) {
        assert_eq!(&assert, app.world().resource::<Counter>());
    }

    #[test]
    fn added() {
        let mut app = create_app().0;

        app.init_asset::<MyAsset>()
            .insert_resource(Counter(vec![0, 0, 0, 0]))
            .add_systems(Update, add_some)
            .add_systems(PostUpdate, count_update.after(AssetEventSystems));

        // First run of the app, `add_systems(Startup…)` runs.
        app.update(); // run_count == 0
        assert_counter(&app, Counter(vec![0, 0, 0, 0]));
        app.update(); // run_count == 1
        assert_counter(&app, Counter(vec![1, 0, 0, 0]));
        app.update(); // run_count == 2
        assert_counter(&app, Counter(vec![1, 0, 0, 0]));
        app.update(); // run_count == 3
        assert_counter(&app, Counter(vec![1, 1, 1, 0]));
        app.update(); // run_count == 4
        assert_counter(&app, Counter(vec![1, 1, 1, 1]));
    }

    #[test]
    fn changed() {
        let mut app = create_app().0;

        app.init_asset::<MyAsset>()
            .insert_resource(Counter(vec![0, 0]))
            .add_systems(Startup, |mut cmds: Commands| {
                let asset0 = cmds.spawn_asset(MyAsset(0, "init"));
                let asset1 = cmds.spawn_asset(MyAsset(1, "init"));
                cmds.spawn(MyComponent(asset0.clone()));
                cmds.spawn(MyComponent(asset0));
                cmds.spawn(MyComponent(asset1.clone()));
                cmds.spawn(MyComponent(asset1.clone()));
                cmds.spawn(MyComponent(asset1));
            })
            .add_systems(Update, update_some)
            .add_systems(PostUpdate, count_update.after(AssetEventSystems));

        // First run of the app, `add_systems(Startup…)` runs.
        app.update(); // run_count == 0

        // First run: We count the entities that were added in the `Startup` schedule
        assert_counter(&app, Counter(vec![2, 3]));

        // Second run: `update_once` updates the first asset, which is
        // associated with two entities, so `count_update` picks up two updates
        app.update(); // run_count == 1
        assert_counter(&app, Counter(vec![4, 3]));

        // Third run: `update_once` doesn't update anything, same values as last
        app.update(); // run_count == 2
        assert_counter(&app, Counter(vec![4, 3]));

        // Fourth run: We update the two assets (asset 0: 2 entities, asset 1: 3)
        app.update(); // run_count == 3
        assert_counter(&app, Counter(vec![6, 6]));

        // Fifth run: only update second asset
        app.update(); // run_count == 4
        assert_counter(&app, Counter(vec![6, 9]));
        // ibid
        app.update(); // run_count == 5
        assert_counter(&app, Counter(vec![6, 12]));
    }
}
