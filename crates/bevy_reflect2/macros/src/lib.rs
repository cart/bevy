#![expect(missing_docs, reason = "Work in progress")]

mod reflect;
pub(crate) mod type_path;

use bevy_macro_utils::BevyManifest;
use proc_macro::TokenStream;
use syn::{parse_macro_input, DeriveInput, Path};

#[proc_macro_derive(Reflect, attributes(reflect))]
pub fn derive_reflect(input: TokenStream) -> TokenStream {
    let ast = parse_macro_input!(input as DeriveInput);
    match reflect::derive_reflect(ast) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.into_compile_error().into(),
    }
}

/// Creates a type path from a standard rust-style type path (as defined in code). The first item in
/// the path will be the crate name, the last item in the path will be the short name. Generics are
/// supported.
#[proc_macro]
pub fn type_path(input: TokenStream) -> TokenStream {
    let bevy_reflect = BevyManifest::shared(|manifest| manifest.get_path("bevy_reflect2"));
    let path = parse_macro_input!(input as Path);
    type_path::type_path(&bevy_reflect, path)
}
