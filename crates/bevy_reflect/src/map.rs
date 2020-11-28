use std::{any::Any, collections::hash_map::Entry};

use bevy_utils::HashMap;

use crate::{serde::Serializable, DiffError, Reflect, ReflectMut, ReflectRef};

/// An ordered ReflectValue->ReflectValue mapping. ReflectValue Keys are assumed to return a non-None hash.  
/// Ideally the ordering is stable across runs, but this is not required.
/// This corresponds to types like [std::collections::HashMap].
pub trait Map: Reflect {
    fn get(&self, key: &dyn Reflect) -> Option<&dyn Reflect>;
    fn get_mut(&mut self, key: &dyn Reflect) -> Option<&mut dyn Reflect>;
    fn get_at(&self, index: usize) -> Option<(&dyn Reflect, &dyn Reflect)>;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn iter(&self) -> MapIter;
    fn clone_dynamic(&self) -> DynamicMap;
}

const HASH_ERROR: &str = "the given key does not support hashing";

#[derive(Default)]
pub struct DynamicMap {
    pub values: Vec<(Box<dyn Reflect>, Box<dyn Reflect>)>,
    pub indices: HashMap<u64, usize>,
}

impl DynamicMap {
    pub fn insert<K: Reflect, V: Reflect>(&mut self, key: K, value: V) {
        self.insert_boxed(Box::new(key), Box::new(value));
    }

    pub fn insert_boxed(&mut self, key: Box<dyn Reflect>, value: Box<dyn Reflect>) {
        match self.indices.entry(key.hash().expect(HASH_ERROR)) {
            Entry::Occupied(entry) => {
                self.values[*entry.get()] = (key, value);
            }
            Entry::Vacant(entry) => {
                entry.insert(self.values.len());
                self.values.push((key, value));
            }
        }
    }
}

impl Map for DynamicMap {
    fn get(&self, key: &dyn Reflect) -> Option<&dyn Reflect> {
        self.indices
            .get(&key.hash().expect(HASH_ERROR))
            .map(|index| &*self.values.get(*index).unwrap().1)
    }

    fn get_mut(&mut self, key: &dyn Reflect) -> Option<&mut dyn Reflect> {
        self.indices
            .get(&key.hash().expect(HASH_ERROR))
            .cloned()
            .map(move |index| &mut *self.values.get_mut(index).unwrap().1)
    }

    fn len(&self) -> usize {
        self.values.len()
    }

    fn clone_dynamic(&self) -> DynamicMap {
        DynamicMap {
            values: self
                .values
                .iter()
                .map(|(key, value)| (key.clone_value(), value.clone_value()))
                .collect(),
            indices: self.indices.clone(),
        }
    }

    fn iter(&self) -> MapIter {
        MapIter {
            map: self,
            index: 0,
        }
    }

    fn get_at(&self, index: usize) -> Option<(&dyn Reflect, &dyn Reflect)> {
        self.values
            .get(index)
            .map(|(key, value)| (&**key, &**value))
    }
}

impl Reflect for DynamicMap {
    fn type_name(&self) -> &str {
        std::any::type_name::<Self>()
    }

    fn any(&self) -> &dyn Any {
        self
    }

    fn any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn apply(&mut self, value: &dyn Reflect) {
        if let ReflectRef::Map(map_value) = value.reflect_ref() {
            for (key, value) in map_value.iter() {
                if let Some(v) = self.get_mut(key) {
                    v.apply(value)
                }
            }
        } else {
            panic!("attempted to apply a non-map type to a map type");
        }
    }

    fn set(&mut self, value: Box<dyn Reflect>) -> Result<(), Box<dyn Reflect>> {
        *self = value.take()?;
        Ok(())
    }

    fn reflect_ref(&self) -> ReflectRef {
        ReflectRef::Map(self)
    }

    fn reflect_mut(&mut self) -> ReflectMut {
        ReflectMut::Map(self)
    }

    fn clone_value(&self) -> Box<dyn Reflect> {
        Box::new(self.clone_dynamic())
    }

    fn hash(&self) -> Option<u64> {
        None
    }

    fn partial_eq(&self, value: &dyn Reflect) -> Option<bool> {
        map_partial_eq(self, value)
    }

    fn serializable(&self) -> Option<Serializable> {
        None
    }

    fn diff<'a>(
        &'a self,
        value: &'a dyn Reflect,
    ) -> Result<Option<Box<dyn Reflect>>, DiffError<'a>> {
        map_diff(self, value)
    }
}

pub struct MapIter<'a> {
    pub(crate) map: &'a dyn Map,
    pub(crate) index: usize,
}

impl<'a> Iterator for MapIter<'a> {
    type Item = (&'a dyn Reflect, &'a dyn Reflect);

    fn next(&mut self) -> Option<Self::Item> {
        let value = self.map.get_at(self.index);
        self.index += 1;
        value
    }
}

pub fn map_partial_eq<M: Map>(a: &M, b: &dyn Reflect) -> Option<bool> {
    let map = if let ReflectRef::Map(map) = b.reflect_ref() {
        map
    } else {
        return Some(false);
    };

    if a.len() != map.len() {
        return Some(false);
    }

    for (key, value) in a.iter() {
        if let Some(map_value) = map.get(key) {
            if let Some(false) | None = value.partial_eq(map_value) {
                return Some(false);
            }
        } else {
            return Some(false);
        }
    }

    Some(true)
}

pub fn map_diff<'a, M: Map>(
    a: &'a M,
    b: &'a dyn Reflect,
) -> Result<Option<Box<dyn Reflect>>, DiffError<'a>> {
    let b_map = if let ReflectRef::Map(b_map) = b.reflect_ref() {
        b_map
    } else {
        return Err(DiffError::TypeMismatch {
            a_type_name: a.type_name(),
            b_type_name: b.type_name(),
        });
    };

    if a.type_name() != b.type_name() {
        return Err(DiffError::TypeMismatch {
            a_type_name: a.type_name(),
            b_type_name: b.type_name(),
        });
    }

    if let Some(equal) = a.partial_eq(b) {
        if equal {
            Ok(None)
        } else {
            Ok(Some(b_map.clone_value()))
        }
    } else {
        Err(DiffError::TypeDoesNotSupportPartialEq {
            type_name: a.type_name(),
        })
    }
}
