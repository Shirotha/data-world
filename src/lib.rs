//! This crate provides a mechanism for storing data as entities in designated [data worlds](DataWorlds).
use bevy_ecs::{prelude::*, system::RunSystemOnce};
use bevy_log::prelude::*;
use bevy_reflect::{std_traits::ReflectDefault, Reflect};
use bevy_scene::{ron::Error as RonError, DynamicScene, DynamicSceneBundle};
// TODO: rename worlds into static, persistent, transient
/// Mutable data retrieved from a [DataWorld](data worlds) resource.
pub enum DataMut<'a> {
    /// Data does not exist.
    Missing,
    /// Data was found.
    Found(EntityWorldMut<'a>),
    /// Data was in the static world and was moved to the dynamic world.
    Moved(EntityWorldMut<'a>, DataRef),
}

/// Data storage separated into its own [world](World).
/// Data will be separated into two world:
/// - Static data is immutable
/// - Dynamic data can be mutable
///
/// Trying to access static data as mutable will first clone the data into the dynamic world.
#[derive(Debug, Resource)]
pub struct DataWorlds {
    static_world: World,
    dynamic_world: World,
}
impl DataWorlds {
    /// Creates a `DataWorlds` resource from optional scene data.
    /// `type_registry` should have registered all components that will be stored in the data worlds.
    #[inline]
    pub fn from_scenes(
        type_registry: &AppTypeRegistry,
        static_scene: Option<DynamicSceneBundle>,
        dynamic_scene: Option<DynamicSceneBundle>,
    ) -> Self {
        let span_static = trace_span!("create_static_data_world").entered();
        let mut static_world = World::new();
        static_world.insert_resource(type_registry.clone());
        if let Some(static_scene) = static_scene {
            static_world.spawn(static_scene);
        }
        span_static.exit();
        let span_dynamic = trace_span!("create_dynamic_data_world").entered();
        let mut dynamic_world = World::new();
        dynamic_world.insert_resource(type_registry.clone());
        if let Some(dynamic_scene) = dynamic_scene {
            dynamic_world.spawn(dynamic_scene);
        }
        span_dynamic.exit();
        Self {
            static_world,
            dynamic_world,
        }
    }
    /// Use a one-time system to modify static data.
    ///
    /// This should only be used for initial setup as data in the static world should be immutable during runtime.
    #[inline(always)]
    pub fn modify_static_data<Out, Marker>(
        &mut self,
        system: impl IntoSystem<(), Out, Marker>,
    ) -> Out {
        self.static_world.run_system_once(system)
    }
    /// Reload only the dynamic data from a scene.
    /// All changes made since the last load will be lost.
    #[inline]
    pub fn reload_dynamic_data(&mut self, dynamic_scene: DynamicSceneBundle) {
        let span = trace_span!("create_dynamic_data_world").entered();
        let mut dynamic_world = World::new();
        dynamic_world.insert_resource(
            self.dynamic_world
                .remove_resource::<AppTypeRegistry>()
                .expect("Resource should have been added in constructor"),
        );
        dynamic_world.spawn(dynamic_scene);
        span.exit();
        self.dynamic_world = dynamic_world;
    }
    /// Serialized static data into RON format.
    /// This should only be nessesary for first time setup, as static data is immutable.
    #[inline]
    pub fn serialize_static_ron(&self) -> Result<String, RonError> {
        let span = trace_span!("serialize_static_data_world").entered();
        let scene = DynamicScene::from_world(&self.static_world);
        let type_registry = self.static_world.resource::<AppTypeRegistry>();
        let result = scene.serialize_ron(type_registry);
        span.exit();
        result
    }
    /// Serialized dynamic data into RON format.
    #[inline]
    pub fn serialize_dynamic_ron(&self) -> Result<String, RonError> {
        let span = trace_span!("serialize_dynamic_data_world").entered();
        let scene = DynamicScene::from_world(&self.dynamic_world);
        let type_registry = self.dynamic_world.resource::<AppTypeRegistry>();
        let result = scene.serialize_ron(type_registry);
        span.exit();
        result
    }
    /// Returns a reference to the data pointed to by `ptr`, returns [`None`] when the reference is [`Null`](DataRef::Null) or the entity does not exist.
    #[inline]
    pub fn get(&self, ptr: DataRef) -> Option<EntityRef> {
        match ptr {
            DataRef::Static(entity) => self.static_world.get_entity(entity),
            DataRef::Dynamic(entity) => self.dynamic_world.get_entity(entity),
            DataRef::Null => None,
        }
    }
    /// Returns a reference to the data pointed to by `ptr`.
    ///
    /// # Panics
    /// This will panic if the reference is [`Null`](DataRef::Null) or the entity does not exits.
    #[inline]
    pub fn entity(&self, ptr: DataRef) -> EntityRef {
        match ptr {
            DataRef::Static(entity) => self.static_world.entity(entity),
            DataRef::Dynamic(entity) => self.dynamic_world.entity(entity),
            DataRef::Null => panic!("Tried to access null reference"),
        }
    }
    /// Returns a mutable reference to the data pointed to by `ptr`, returns [`None`] when the reference is [`Null`](DataRef::Null) or the entity does not exist.
    /// Static data will be cloned into the dynamic world
    #[inline]
    pub fn get_mut(&mut self, ptr: DataRef) -> DataMut {
        match ptr {
            DataRef::Static(entity) => {
                let Some(entity) = self.transfer(entity) else {
                    return DataMut::Missing;
                };
                let Some(ptr) = self.dynamic_world.get_entity_mut(entity) else {
                    return DataMut::Missing;
                };
                DataMut::Moved(ptr, DataRef::Dynamic(entity))
            }
            DataRef::Dynamic(entity) => {
                let Some(ptr) = self.dynamic_world.get_entity_mut(entity) else {
                    return DataMut::Missing;
                };
                DataMut::Found(ptr)
            }
            DataRef::Null => DataMut::Missing,
        }
    }
    /// Returns a mutable reference to the data pointed to by `ptr`.
    ///
    /// # Panics
    /// This will panic if the reference is [`Null`](DataRef::Null) or the entity does not exits.
    #[inline]
    pub fn entity_mut(&mut self, ptr: DataRef) -> DataMut {
        match ptr {
            DataRef::Static(entity) => {
                let Some(entity) = self.transfer(entity) else {
                    return DataMut::Missing;
                };
                let Some(ptr) = self.dynamic_world.get_entity_mut(entity) else {
                    return DataMut::Missing;
                };
                DataMut::Moved(ptr, DataRef::Dynamic(entity))
            }
            DataRef::Dynamic(entity) => {
                let Some(ptr) = self.dynamic_world.get_entity_mut(entity) else {
                    return DataMut::Missing;
                };
                DataMut::Found(ptr)
            }
            DataRef::Null => panic!("Tried to access null reference"),
        }
    }
    #[inline]
    fn transfer(&mut self, entity: Entity) -> Option<Entity> {
        trace!("transfer entity to dynamic world");
        let source_ref = self.static_world.get_entity(entity)?;
        let target = self.dynamic_world.spawn_empty().id();
        let components = self.static_world.components();
        // SAFETY: constructor guaranties that a `AppTypeRegistry` is added.
        let registry = self.static_world.resource::<AppTypeRegistry>();
        let registry_guard = registry.read();
        for component_id in source_ref.archetype().components() {
            let type_id = components
                .get_info(component_id)
                .expect("Component should implement Reflect")
                .type_id()
                .expect("Component should be a rust type");
            registry_guard
                .get(type_id)
                .expect("type should be registered")
                .data::<ReflectComponent>()
                .expect("Data should be added by Reflect derive")
                .copy(
                    &self.static_world,
                    &mut self.dynamic_world,
                    entity,
                    target,
                    &registry_guard,
                );
        }
        Some(target)
    }
}

/// Reference to data stored in [DataWorlds].
///
/// # Safety
/// For data that is static but might be mutabe at a later point all cross references should be `DataRef` instead of plain [Entity] fields,
/// as those would get invalidated when the data gets transfered to the dynamic world.
#[derive(Debug, Reflect, Default, Clone, Copy, PartialEq, Eq)]
#[reflect(Default, PartialEq)]
pub enum DataRef {
    /// Null pointer.
    #[default]
    Null,
    /// Data located in the static world.
    Static(Entity),
    /// Data located in the dynamic world.
    Dynamic(Entity),
}

#[cfg(test)]
mod test {
    use super::*;

    #[derive(Debug, Clone, Copy, Reflect, Component)]
    #[reflect(Component)]
    struct SomeCompoennt {
        data: i32,
    }
    #[derive(Debug, Clone, Copy, Reflect, Component)]
    #[reflect(Component)]
    struct SomeRef {
        entity: DataRef,
    }
    fn setup_data(world: &mut World) -> DataRef {
        let a = world.spawn(SomeCompoennt { data: 42 }).id();
        let b = world
            .spawn((
                SomeCompoennt { data: 21 },
                SomeRef {
                    entity: DataRef::Static(a),
                },
            ))
            .id();
        DataRef::Static(b)
    }

    #[test]
    fn test() {
        let type_registry = AppTypeRegistry::default();
        {
            let mut guard = type_registry.write();
            guard.register::<SomeCompoennt>();
            guard.register::<SomeRef>();
        }
        let mut data = DataWorlds::from_scenes(&type_registry, None, None);
        let root = data.modify_static_data(setup_data);
        let mut world = World::new();
        world.insert_resource(type_registry);
        world.insert_resource(data);
        {
            let data = world.resource_ref::<DataWorlds>();
            let b = data.entity(root);
            assert_eq!(b.get::<SomeCompoennt>().unwrap().data, 21);
            let a = data.entity(b.get::<SomeRef>().unwrap().entity);
            assert_eq!(a.get::<SomeCompoennt>().unwrap().data, 42);
        }
        {
            let mut data = world.resource_mut::<DataWorlds>();
            let DataMut::Moved(mut b, entity) = data.entity_mut(root) else {
                panic!()
            };
            b.get_mut::<SomeCompoennt>().unwrap().data = 42;
            let b = data.entity(entity);
            assert_eq!(b.get::<SomeCompoennt>().unwrap().data, 42);
        }
        // FIXME: ReflectSerialize should be defined by Entity, but it isn't for some reason
        let data = {
            let data = world.resource::<DataWorlds>();
            let static_ron = data.serialize_static_ron().unwrap();
            let dynamic_ron = data.serialize_dynamic_ron().unwrap();
            (static_ron, dynamic_ron)
        };
        world.remove_resource::<DataWorlds>();
        // TODO: save ron to file and test loading
    }
}
