use flurry::HashMap;
use lazy_static::lazy_static;
use std::{fmt::Debug, hash::Hash, ops::Deref};

use crate::{
    id::{FunctionId, TraitTypeId, ValueTypeId},
    id_factory::IdFactory,
    no_move_vec::NoMoveVec,
    NativeFunction, TraitType, ValueType,
};

lazy_static! {
    static ref FUNCTION_ID_FACTORY: IdFactory<FunctionId> = IdFactory::new();
    static ref FUNCTIONS_BY_NAME: HashMap<String, FunctionId> = HashMap::new();
    static ref FUNCTIONS_BY_VALUE: HashMap<&'static NativeFunction, FunctionId> = HashMap::new();
    static ref FUNCTIONS: NoMoveVec<(&'static NativeFunction, String)> = NoMoveVec::new();
}

lazy_static! {
    static ref VALUE_TYPE_ID_FACTORY: IdFactory<ValueTypeId> = IdFactory::new();
    static ref VALUE_TYPES_BY_NAME: HashMap<String, ValueTypeId> = HashMap::new();
    static ref VALUE_TYPES_BY_VALUE: HashMap<&'static ValueType, ValueTypeId> = HashMap::new();
    static ref VALUE_TYPES: NoMoveVec<(&'static ValueType, String)> = NoMoveVec::new();
}

lazy_static! {
    static ref TRAIT_TYPE_ID_FACTORY: IdFactory<TraitTypeId> = IdFactory::new();
    static ref TRAIT_TYPES_BY_NAME: HashMap<String, TraitTypeId> = HashMap::new();
    static ref TRAIT_TYPES_BY_VALUE: HashMap<&'static TraitType, TraitTypeId> = HashMap::new();
    static ref TRAIT_TYPES: NoMoveVec<(&'static TraitType, String)> = NoMoveVec::new();
}

fn register_thing<
    K: From<usize> + Deref<Target = usize> + Sync + Send + Copy,
    V: Clone + Hash + Ord + Eq + Sync + Send + Copy,
    const INITIAL_CAPACITY_BITS: u32,
>(
    global_name: &str,
    value: V,
    id_factory: &IdFactory<K>,
    store: &NoMoveVec<(V, String), INITIAL_CAPACITY_BITS>,
    map_by_name: &HashMap<String, K>,
    map_by_value: &HashMap<V, K>,
) {
    if map_by_value.pin().get(&value).is_none() {
        let new_id = id_factory.get();
        // SAFETY: this is a fresh id
        unsafe {
            store.insert(*new_id, (value, global_name.to_string()));
        }
        map_by_value.pin().insert(value, new_id);
        map_by_name.pin().insert(global_name.to_string(), new_id);
    }
}

fn get_thing_id<
    K: From<usize> + Deref<Target = usize> + Sync + Send + Copy,
    V: Clone + Hash + Ord + Eq + Debug + Sync + Send,
>(
    value: V,
    map_by_value: &HashMap<V, K>,
) -> K {
    if let Some(id) = map_by_value.pin().get(&value) {
        *id
    } else {
        panic!("Use of unregistered {:?}", value);
    }
}

pub fn register_function(global_name: &str, func: &'static NativeFunction) {
    register_thing(
        global_name,
        func,
        &FUNCTION_ID_FACTORY,
        &FUNCTIONS,
        &FUNCTIONS_BY_NAME,
        &FUNCTIONS_BY_VALUE,
    )
}

pub fn get_function_id(func: &'static NativeFunction) -> FunctionId {
    get_thing_id(func, &FUNCTIONS_BY_VALUE)
}

pub fn get_function_id_by_global_name(global_name: &str) -> Option<FunctionId> {
    FUNCTIONS_BY_NAME.pin().get(global_name).cloned()
}

pub fn get_function(id: FunctionId) -> &'static NativeFunction {
    FUNCTIONS.get(*id).unwrap().0
}

pub fn get_function_global_name(id: FunctionId) -> &'static str {
    &FUNCTIONS.get(*id).unwrap().1
}

pub fn register_value_type(global_name: &str, ty: &'static ValueType) {
    register_thing(
        global_name,
        ty,
        &VALUE_TYPE_ID_FACTORY,
        &VALUE_TYPES,
        &VALUE_TYPES_BY_NAME,
        &VALUE_TYPES_BY_VALUE,
    )
}

pub fn get_value_type_id(func: &'static ValueType) -> ValueTypeId {
    get_thing_id(func, &VALUE_TYPES_BY_VALUE)
}

pub fn get_value_type_id_by_global_name(global_name: &str) -> Option<ValueTypeId> {
    VALUE_TYPES_BY_NAME.pin().get(global_name).cloned()
}

pub fn get_value_type(id: ValueTypeId) -> &'static ValueType {
    VALUE_TYPES.get(*id).unwrap().0
}

pub fn get_value_type_global_name(id: ValueTypeId) -> &'static str {
    &VALUE_TYPES.get(*id).unwrap().1
}

pub fn register_trait_type(global_name: &str, ty: &'static TraitType) {
    register_thing(
        global_name,
        ty,
        &TRAIT_TYPE_ID_FACTORY,
        &TRAIT_TYPES,
        &TRAIT_TYPES_BY_NAME,
        &TRAIT_TYPES_BY_VALUE,
    )
}

pub fn get_trait_type_id(func: &'static TraitType) -> TraitTypeId {
    get_thing_id(func, &TRAIT_TYPES_BY_VALUE)
}

pub fn get_trait_type_id_by_global_name(global_name: &str) -> Option<TraitTypeId> {
    TRAIT_TYPES_BY_NAME.pin().get(global_name).cloned()
}

pub fn get_trait(id: TraitTypeId) -> &'static TraitType {
    TRAIT_TYPES.get(*id).unwrap().0
}

pub fn get_trait_type_global_name(id: TraitTypeId) -> &'static str {
    &TRAIT_TYPES.get(*id).unwrap().1
}