// Copyright 2019 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

// modified by Bevy contributors

/// Imagine macro parameters, but more like those Russian dolls.
///
/// Calls m!(A, B, C), m!(A, B), m!(B), and m!() for i.e. (m, A, B, C)
/// where m is any macro, for any number of parameters.
#[macro_export]
macro_rules! smaller_tuples_too {
    ($m: ident, $ty: ident) => {
        $m!{$ty}
        $m!{}
    };
    ($m: ident, $ty: ident, $($tt: ident),*) => {
        $m!{$ty, $($tt),*}
        smaller_tuples_too!{$m, $($tt),*}
    };
}

mod access;
mod archetype;
mod blob_vec;
mod borrow;
mod bundle;
mod component;
mod entities;
mod entity_builder;
mod entity_map;
mod filter;
mod query;
pub mod query2;
mod serde;
mod sparse_set;
mod world;
mod world_builder;

pub use access::*;
pub use archetype::*;
pub use borrow::*;
pub use bundle::*;
pub use component::*;
pub use entities::*;
// pub use entity_builder::{BuiltEntity, EntityBuilder};
pub use entity_map::*;
pub use filter::*;
pub use query::*;
pub use sparse_set::*;
pub use world::*;
pub use world_builder::*;

// Unstable implementation details needed by the macros
#[doc(hidden)]
pub use bevy_utils;
#[doc(hidden)]
pub use query::Fetch;
