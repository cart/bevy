use crate::{
    app::{App, AppExit},
    event::Events,
    plugin::Plugin,
    stage, startup_stage, PluginGroup, PluginGroupBuilder,
};
use bevy_ecs::{
    core::{Component, FromWorld, World},
    schedule::{
        clear_trackers_system, RunOnce, Schedule, Stage, StateStage, SystemDescriptor, SystemStage,
    },
    system::{IntoExclusiveSystem, IntoSystem},
};
use bevy_utils::tracing::debug;

/// Configure [App]s using the builder pattern
pub struct AppBuilder {
    pub app: App,
}

impl Default for AppBuilder {
    fn default() -> Self {
        let mut app_builder = AppBuilder {
            app: App::default(),
        };

        app_builder
            .add_default_stages()
            .add_event::<AppExit>()
            .add_system_to_stage(stage::LAST, clear_trackers_system.exclusive_system());
        app_builder
    }
}

impl AppBuilder {
    pub fn empty() -> AppBuilder {
        AppBuilder {
            app: App::default(),
        }
    }

    pub fn run(&mut self) {
        let app = std::mem::take(&mut self.app);
        app.run();
    }

    pub fn world(&mut self) -> &World {
        &self.app.world
    }

    pub fn world_mut(&mut self) -> &mut World {
        &mut self.app.world
    }

    pub fn set_world(&mut self, world: World) -> &mut Self {
        self.app.world = world;
        self
    }

    pub fn add_stage<S: Stage>(&mut self, name: &'static str, stage: S) -> &mut Self {
        self.app.schedule.add_stage(name, stage);
        self
    }

    pub fn add_stage_after<S: Stage>(
        &mut self,
        target: &'static str,
        name: &'static str,
        stage: S,
    ) -> &mut Self {
        self.app.schedule.add_stage_after(target, name, stage);
        self
    }

    pub fn add_stage_before<S: Stage>(
        &mut self,
        target: &'static str,
        name: &'static str,
        stage: S,
    ) -> &mut Self {
        self.app.schedule.add_stage_before(target, name, stage);
        self
    }

    pub fn add_startup_stage<S: Stage>(&mut self, name: &'static str, stage: S) -> &mut Self {
        self.app
            .schedule
            .stage(stage::STARTUP, |schedule: &mut Schedule| {
                schedule.add_stage(name, stage)
            });
        self
    }

    pub fn add_startup_stage_after<S: Stage>(
        &mut self,
        target: &'static str,
        name: &'static str,
        stage: S,
    ) -> &mut Self {
        self.app
            .schedule
            .stage(stage::STARTUP, |schedule: &mut Schedule| {
                schedule.add_stage_after(target, name, stage)
            });
        self
    }

    pub fn add_startup_stage_before<S: Stage>(
        &mut self,
        target: &'static str,
        name: &'static str,
        stage: S,
    ) -> &mut Self {
        self.app
            .schedule
            .stage(stage::STARTUP, |schedule: &mut Schedule| {
                schedule.add_stage_before(target, name, stage)
            });
        self
    }

    pub fn stage<T: Stage, F: FnOnce(&mut T) -> &mut T>(
        &mut self,
        name: &str,
        func: F,
    ) -> &mut Self {
        self.app.schedule.stage(name, func);
        self
    }

    pub fn add_system(&mut self, system: impl Into<SystemDescriptor>) -> &mut Self {
        self.add_system_to_stage(stage::UPDATE, system)
    }

    pub fn add_system_to_stage(
        &mut self,
        stage_name: &'static str,
        system: impl Into<SystemDescriptor>,
    ) -> &mut Self {
        self.app.schedule.add_system_to_stage(stage_name, system);
        self
    }

    pub fn add_startup_system(&mut self, system: impl Into<SystemDescriptor>) -> &mut Self {
        self.add_startup_system_to_stage(startup_stage::STARTUP, system)
    }

    pub fn add_startup_system_to_stage(
        &mut self,
        stage_name: &'static str,
        system: impl Into<SystemDescriptor>,
    ) -> &mut Self {
        self.app
            .schedule
            .stage(stage::STARTUP, |schedule: &mut Schedule| {
                schedule.add_system_to_stage(stage_name, system)
            });
        self
    }

    pub fn on_state_enter<T: Clone + Component>(
        &mut self,
        stage: &str,
        state: T,
        system: impl Into<SystemDescriptor>,
    ) -> &mut Self {
        self.stage(stage, |stage: &mut StateStage<T>| {
            stage.on_state_enter(state, system)
        })
    }

    pub fn on_state_update<T: Clone + Component>(
        &mut self,
        stage: &str,
        state: T,
        system: impl Into<SystemDescriptor>,
    ) -> &mut Self {
        self.stage(stage, |stage: &mut StateStage<T>| {
            stage.on_state_update(state, system)
        })
    }

    pub fn on_state_exit<T: Clone + Component>(
        &mut self,
        stage: &str,
        state: T,
        system: impl Into<SystemDescriptor>,
    ) -> &mut Self {
        self.stage(stage, |stage: &mut StateStage<T>| {
            stage.on_state_exit(state, system)
        })
    }

    pub fn add_default_stages(&mut self) -> &mut Self {
        self.add_stage(
            stage::STARTUP,
            Schedule::default()
                .with_run_criteria(RunOnce::default())
                .with_stage(startup_stage::PRE_STARTUP, SystemStage::parallel())
                .with_stage(startup_stage::STARTUP, SystemStage::parallel())
                .with_stage(startup_stage::POST_STARTUP, SystemStage::parallel()),
        )
        .add_stage(stage::FIRST, SystemStage::parallel())
        .add_stage(stage::PRE_EVENT, SystemStage::parallel())
        .add_stage(stage::EVENT, SystemStage::parallel())
        .add_stage(stage::PRE_UPDATE, SystemStage::parallel())
        .add_stage(stage::UPDATE, SystemStage::parallel())
        .add_stage(stage::POST_UPDATE, SystemStage::parallel())
        .add_stage(stage::LAST, SystemStage::parallel())
    }

    pub fn add_event<T>(&mut self) -> &mut Self
    where
        T: Component,
    {
        self.insert_resource(Events::<T>::default())
            .add_system_to_stage(stage::EVENT, Events::<T>::update_system.system())
    }

    /// Inserts a resource to the current [App] and overwrites any resource previously added of the same type.
    pub fn insert_resource<T>(&mut self, resource: T) -> &mut Self
    where
        T: Component,
    {
        self.app.world.insert_resource(resource);
        self
    }

    pub fn insert_non_send_resource<T>(&mut self, resource: T) -> &mut Self
    where
        T: 'static,
    {
        self.app.world.insert_non_send(resource);
        self
    }

    pub fn init_resource<R>(&mut self) -> &mut Self
    where
        R: FromWorld + Send + Sync + 'static,
    {
        // PERF: We could avoid double hashing here, since the `from_resources` call is guaranteed not to
        // modify the map. However, we would need to be borrowing resources both mutably and immutably,
        // so we would need to be extremely certain this is correct
        if !self.world_mut().contains_resource::<R>() {
            let resource = R::from_world(self.world_mut());
            self.insert_resource(resource);
        }
        self
    }

    pub fn init_non_send_resource<R>(&mut self) -> &mut Self
    where
        R: FromWorld + 'static,
    {
        // See perf comment in init_resource
        if self.app.world.get_non_send::<R>().is_none() {
            let resource = R::from_world(self.world_mut());
            self.app.world.insert_non_send(resource);
        }
        self
    }

    pub fn set_runner(&mut self, run_fn: impl Fn(App) + 'static) -> &mut Self {
        self.app.runner = Box::new(run_fn);
        self
    }

    pub fn add_plugin<T>(&mut self, plugin: T) -> &mut Self
    where
        T: Plugin,
    {
        debug!("added plugin: {}", plugin.name());
        plugin.build(self);
        self
    }

    pub fn add_plugins<T: PluginGroup>(&mut self, mut group: T) -> &mut Self {
        let mut plugin_group_builder = PluginGroupBuilder::default();
        group.build(&mut plugin_group_builder);
        plugin_group_builder.finish(self);
        self
    }

    pub fn add_plugins_with<T, F>(&mut self, mut group: T, func: F) -> &mut Self
    where
        T: PluginGroup,
        F: FnOnce(&mut PluginGroupBuilder) -> &mut PluginGroupBuilder,
    {
        let mut plugin_group_builder = PluginGroupBuilder::default();
        group.build(&mut plugin_group_builder);
        func(&mut plugin_group_builder);
        plugin_group_builder.finish(self);
        self
    }
}
