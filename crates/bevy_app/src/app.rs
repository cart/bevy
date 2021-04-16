use crate::{
    CoreStage, Events, Plugin, PluginGroup, PluginGroupBuilder,
    StartupStage,
};
use bevy_ecs::{
    component::{Component, ComponentDescriptor},
    prelude::{FromWorld, IntoExclusiveSystem, IntoSystem},
    schedule::{
        RunOnce, Schedule, Stage, StageLabel, State, SystemDescriptor, SystemSet, SystemStage,
    },
    world::World,
};
use bevy_utils::tracing::debug;
#[cfg(feature = "trace")]
use bevy_utils::tracing::info_span;
use std::fmt::Debug;
use std::hash::Hash;

#[allow(clippy::needless_doctest_main)]
/// Containers of app logic and data
///
/// App store the ECS World, Resources, Schedule, and Executor. They also store the "run" function
/// of the App, which by default executes the App schedule once. Apps are constructed using the
/// builder pattern.
///
/// ## Example
/// Here is a simple "Hello World" Bevy app:
/// ```
/// # use bevy_app::prelude::*;
/// # use bevy_ecs::prelude::*;
///
/// fn main() {
///    App::new()
///        .add_system(hello_world_system.system())
///        .run();
/// }
///
/// fn hello_world_system() {
///    println!("hello world");
/// }
/// ```
pub struct App {
    pub world: World,
    pub runner: Box<dyn Fn(App)>,
    pub schedule: Schedule,
    pub sub_apps: Vec<App>,
}

impl Default for App {
    fn default() -> Self {
        let mut app = App::empty();
        #[cfg(feature = "bevy_reflect")]
        app.init_resource::<bevy_reflect::TypeRegistryArc>();

        app.add_default_stages()
            .add_event::<AppExit>()
            .add_system_to_stage(CoreStage::Last, World::clear_trackers.exclusive_system());

        #[cfg(feature = "bevy_ci_testing")]
        {
            crate::ci_testing::setup_app(&mut app);
        }

        app
    }
}

impl App {
    pub fn new() -> App {
        App::default()
    }

    pub fn empty() -> App {
        Self {
            world: Default::default(),
            schedule: Default::default(),
            runner: Box::new(run_once),
            sub_apps: Vec::new(),
        }
    }

    pub fn update(&mut self) {
        self.schedule.run(&mut self.world);
    }

    pub fn run(&mut self) {
        #[cfg(feature = "trace")]
        let bevy_app_run_span = info_span!("bevy_app");
        #[cfg(feature = "trace")]
        let _bevy_app_run_guard = bevy_app_run_span.enter();

        let mut app = std::mem::replace(self, App::empty());
        let runner = std::mem::replace(&mut app.runner, Box::new(run_once));
        (runner)(app);
    }

    pub fn add_stage<S: Stage>(&mut self, label: impl StageLabel, stage: S) -> &mut Self {
        self.schedule.add_stage(label, stage);
        self
    }

    pub fn add_stage_after<S: Stage>(
        &mut self,
        target: impl StageLabel,
        label: impl StageLabel,
        stage: S,
    ) -> &mut Self {
        self.schedule.add_stage_after(target, label, stage);
        self
    }

    pub fn add_stage_before<S: Stage>(
        &mut self,
        target: impl StageLabel,
        label: impl StageLabel,
        stage: S,
    ) -> &mut Self {
        self.schedule.add_stage_before(target, label, stage);
        self
    }

    pub fn add_startup_stage<S: Stage>(&mut self, label: impl StageLabel, stage: S) -> &mut Self {
        self.schedule
            .stage(CoreStage::Startup, |schedule: &mut Schedule| {
                schedule.add_stage(label, stage)
            });
        self
    }

    pub fn add_startup_stage_after<S: Stage>(
        &mut self,
        target: impl StageLabel,
        label: impl StageLabel,
        stage: S,
    ) -> &mut Self {
        self.schedule
            .stage(CoreStage::Startup, |schedule: &mut Schedule| {
                schedule.add_stage_after(target, label, stage)
            });
        self
    }

    pub fn add_startup_stage_before<S: Stage>(
        &mut self,
        target: impl StageLabel,
        label: impl StageLabel,
        stage: S,
    ) -> &mut Self {
        self.schedule
            .stage(CoreStage::Startup, |schedule: &mut Schedule| {
                schedule.add_stage_before(target, label, stage)
            });
        self
    }

    pub fn stage<T: Stage, F: FnOnce(&mut T) -> &mut T>(
        &mut self,
        label: impl StageLabel,
        func: F,
    ) -> &mut Self {
        self.schedule.stage(label, func);
        self
    }

    pub fn add_system(&mut self, system: impl Into<SystemDescriptor>) -> &mut Self {
        self.add_system_to_stage(CoreStage::Update, system)
    }

    pub fn add_system_set(&mut self, system_set: SystemSet) -> &mut Self {
        self.add_system_set_to_stage(CoreStage::Update, system_set)
    }

    pub fn add_system_to_stage(
        &mut self,
        stage_label: impl StageLabel,
        system: impl Into<SystemDescriptor>,
    ) -> &mut Self {
        self.schedule.add_system_to_stage(stage_label, system);
        self
    }

    pub fn add_system_set_to_stage(
        &mut self,
        stage_label: impl StageLabel,
        system_set: SystemSet,
    ) -> &mut Self {
        self.schedule
            .add_system_set_to_stage(stage_label, system_set);
        self
    }

    pub fn add_startup_system(&mut self, system: impl Into<SystemDescriptor>) -> &mut Self {
        self.add_startup_system_to_stage(StartupStage::Startup, system)
    }

    pub fn add_startup_system_to_stage(
        &mut self,
        stage_label: impl StageLabel,
        system: impl Into<SystemDescriptor>,
    ) -> &mut Self {
        self.schedule
            .stage(CoreStage::Startup, |schedule: &mut Schedule| {
                schedule.add_system_to_stage(stage_label, system)
            });
        self
    }

    /// Adds a new [State] with the given `initial` value.
    /// This inserts a new `State<T>` resource and adds a new "driver" to [CoreStage::Update].
    /// Each stage that uses `State<T>` for system run criteria needs a driver. If you need to use your state in a
    /// different stage, consider using [Self::add_state_to_stage] or manually adding [State::get_driver] to additional stages
    /// you need it in.
    pub fn add_state<T>(&mut self, initial: T) -> &mut Self
    where
        T: Component + Debug + Clone + Eq + Hash,
    {
        self.add_state_to_stage(CoreStage::Update, initial)
    }

    /// Adds a new [State] with the given `initial` value.
    /// This inserts a new `State<T>` resource and adds a new "driver" to the given stage.
    /// Each stage that uses `State<T>` for system run criteria needs a driver. If you need to use your state in
    /// more than one stage, consider manually adding [State::get_driver] to the stages
    /// you need it in.
    pub fn add_state_to_stage<T>(&mut self, stage: impl StageLabel, initial: T) -> &mut Self
    where
        T: Component + Debug + Clone + Eq + Hash,
    {
        self.insert_resource(State::new(initial))
            .add_system_set_to_stage(stage, State::<T>::get_driver())
    }

    pub fn add_default_stages(&mut self) -> &mut Self {
        self.add_stage(CoreStage::First, SystemStage::parallel())
            .add_stage(
                CoreStage::Startup,
                Schedule::default()
                    .with_run_criteria(RunOnce::default())
                    .with_stage(StartupStage::PreStartup, SystemStage::parallel())
                    .with_stage(StartupStage::Startup, SystemStage::parallel())
                    .with_stage(StartupStage::PostStartup, SystemStage::parallel()),
            )
            .add_stage(CoreStage::PreUpdate, SystemStage::parallel())
            .add_stage(CoreStage::Update, SystemStage::parallel())
            .add_stage(CoreStage::PostUpdate, SystemStage::parallel())
            .add_stage(CoreStage::Last, SystemStage::parallel())
    }

    /// Setup the application to manage events of type `T`.
    ///
    /// This is done by adding a `Resource` of type `Events::<T>`,
    /// and inserting a `Events::<T>::update_system` system into `CoreStage::First`.
    pub fn add_event<T>(&mut self) -> &mut Self
    where
        T: Component,
    {
        self.insert_resource(Events::<T>::default())
            .add_system_to_stage(CoreStage::First, Events::<T>::update_system.system())
    }

    /// Inserts a resource to the current [App] and overwrites any resource previously added of the
    /// same type.
    pub fn insert_resource<T>(&mut self, resource: T) -> &mut Self
    where
        T: Component,
    {
        self.world.insert_resource(resource);
        self
    }

    pub fn insert_non_send_resource<T>(&mut self, resource: T) -> &mut Self
    where
        T: 'static,
    {
        self.world.insert_non_send(resource);
        self
    }

    pub fn init_resource<R>(&mut self) -> &mut Self
    where
        R: FromWorld + Send + Sync + 'static,
    {
        // PERF: We could avoid double hashing here, since the `from_resources` call is guaranteed
        // not to modify the map. However, we would need to be borrowing resources both
        // mutably and immutably, so we would need to be extremely certain this is correct
        if !self.world.contains_resource::<R>() {
            let resource = R::from_world(&mut self.world);
            self.insert_resource(resource);
        }
        self
    }

    pub fn init_non_send_resource<R>(&mut self) -> &mut Self
    where
        R: FromWorld + 'static,
    {
        // See perf comment in init_resource
        if self.world.get_non_send_resource::<R>().is_none() {
            let resource = R::from_world(&mut self.world);
            self.world.insert_non_send(resource);
        }
        self
    }

    pub fn set_runner(&mut self, run_fn: impl Fn(App) + 'static) -> &mut Self {
        self.runner = Box::new(run_fn);
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

    /// Registers a new component using the given [ComponentDescriptor]. Components do not need to
    /// be manually registered. This just provides a way to override default configuration.
    /// Attempting to register a component with a type that has already been used by [World]
    /// will result in an error.
    ///
    /// See [World::register_component]
    pub fn register_component(&mut self, descriptor: ComponentDescriptor) -> &mut Self {
        self.world.register_component(descriptor).unwrap();
        self
    }

    #[cfg(feature = "bevy_reflect")]
    pub fn register_type<T: bevy_reflect::GetTypeRegistration>(&mut self) -> &mut Self {
        {
            let registry = self
                .world
                .get_resource_mut::<bevy_reflect::TypeRegistryArc>()
                .unwrap();
            registry.write().register::<T>();
        }
        self
    }
}

fn run_once(mut app: App) {
    app.update();
}

/// An event that indicates the app should exit. This will fully exit the app process.
#[derive(Debug, Clone)]
pub struct AppExit;
