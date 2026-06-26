#![cfg_attr(docsrs, feature(doc_cfg))]

//! Macros for deriving asset traits.

use bevy_ecs_macro_logic::component::{
    DeriveComponent, HookAttributeKind, StorageAttribute, StorageTy,
};
use bevy_macro_utils::{as_member, BevyManifest};
use proc_macro::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::{parse_macro_input, parse_quote, Data, DataStruct, DeriveInput, Path};

const DEPENDENCY_ATTRIBUTE: &str = "dependency";

/// Implement the `Asset` trait.
#[proc_macro_derive(Asset, attributes(dependency))]
pub fn derive_asset(input: TokenStream) -> TokenStream {
    let mut ast = parse_macro_input!(input as DeriveInput);
    let (bevy_ecs, bevy_asset) = BevyManifest::shared(|manifest| {
        (
            manifest.get_path("bevy_ecs"),
            manifest.get_path("bevy_asset"),
        )
    });

    let mut derive_component = match DeriveComponent::parse(&ast, StorageAttribute::Allowed) {
        Ok(value) => value,
        Err(e) => return e.into_compile_error().into(),
    };

    derive_component.on_add.push(HookAttributeKind::Path(
        parse_quote!(#bevy_asset::hooks::on_add::<Self>),
    ));
    derive_component.on_remove.push(HookAttributeKind::Path(
        parse_quote!(#bevy_asset::hooks::on_remove::<Self>),
    ));
    derive_component.on_despawn.push(HookAttributeKind::Path(
        parse_quote!(#bevy_asset::hooks::on_despawn::<Self>),
    ));

    let component_impl =
        match derive_component.impl_component(&mut ast, &bevy_ecs, StorageTy::Table) {
            Ok(value) => value,
            Err(err) => return err.into_compile_error().into(),
        };

    let struct_name = &ast.ident;
    let (impl_generics, type_generics, where_clause) = &ast.generics.split_for_impl();
    let dependency_visitor = match derive_dependency_visitor_internal(&ast, &bevy_asset, &bevy_ecs)
    {
        Ok(dependency_visitor) => dependency_visitor,
        Err(err) => return err.into_compile_error().into(),
    };

    TokenStream::from(quote! {
        impl #impl_generics #bevy_asset::Asset for #struct_name #type_generics #where_clause { }
        #dependency_visitor
        #component_impl
    })
}

/// Implement the `VisitAssetDependencies` trait.
#[proc_macro_derive(VisitAssetDependencies, attributes(dependency))]
pub fn derive_asset_dependency_visitor(input: TokenStream) -> TokenStream {
    let ast = parse_macro_input!(input as DeriveInput);
    let (bevy_ecs, bevy_asset) = BevyManifest::shared(|manifest| {
        (
            manifest.get_path("bevy_ecs"),
            manifest.get_path("bevy_asset"),
        )
    });
    match derive_dependency_visitor_internal(&ast, &bevy_asset, &bevy_ecs) {
        Ok(dependency_visitor) => TokenStream::from(dependency_visitor),
        Err(err) => err.into_compile_error().into(),
    }
}

fn derive_dependency_visitor_internal(
    ast: &DeriveInput,
    bevy_asset_path: &Path,
    bevy_ecs: &Path,
) -> Result<proc_macro2::TokenStream, syn::Error> {
    let struct_name = &ast.ident;
    let (impl_generics, type_generics, where_clause) = &ast.generics.split_for_impl();

    let visit_dep = |to_read| quote!(#bevy_asset_path::VisitAssetDependencies::visit_dependencies(#to_read, visit););
    let is_dep_attribute = |a: &syn::Attribute| a.path().is_ident(DEPENDENCY_ATTRIBUTE);
    let field_has_dep = |f: &syn::Field| f.attrs.iter().any(is_dep_attribute);

    let body = match &ast.data {
        Data::Struct(DataStruct { fields, .. }) => {
            let field_visitors = fields
                .iter()
                .enumerate()
                .filter(|(_, f)| field_has_dep(f))
                .map(|(i, field)| as_member(field.ident.as_ref(), i))
                .map(|member| visit_dep(quote!(&self.#member)));
            Some(quote!(#(#field_visitors)*))
        }
        Data::Enum(data_enum) => {
            let variant_has_dep = |v: &syn::Variant| v.fields.iter().any(field_has_dep);
            let any_case_required = data_enum.variants.iter().any(variant_has_dep);
            let cases = data_enum.variants.iter().filter(|v| variant_has_dep(v));
            let cases = cases.map(|variant| {
                let ident = &variant.ident;
                let field_members = variant
                    .fields
                    .iter()
                    .enumerate()
                    .filter(|(_, f)| field_has_dep(f))
                    .map(|(i, field)| as_member(field.ident.as_ref(), i));
                let field_locals = field_members.clone().map(|m| format_ident!("__self_{}", m));
                let field_visitors = field_locals.clone().map(|i| visit_dep(quote!(#i)));
                quote!(Self::#ident {#(#field_members: #field_locals,)* ..} => {
                    #(#field_visitors)*
                })
            });

            any_case_required.then(|| quote!(match self { #(#cases)*, _ => {} }))
        }
        Data::Union(_) => {
            return Err(syn::Error::new(
                Span::call_site().into(),
                "Asset derive currently doesn't work on unions",
            ));
        }
    };

    // prevent unused variable warning in case there are no dependencies
    let visit = if body.is_none() {
        quote! { _visit }
    } else {
        quote! { visit }
    };

    Ok(quote! {
        impl #impl_generics #bevy_asset_path::VisitAssetDependencies for #struct_name #type_generics #where_clause {
            fn visit_dependencies(&self, #visit: &mut impl ::core::ops::FnMut(#bevy_ecs::entity::Entity)) {
                #body
            }
        }
    })
}
