mod attributes;
mod type_data;

use bevy_macro_utils::{fq_std::FQOption, BevyManifest};
use proc_macro2::TokenStream;
use quote::{format_ident, quote, quote_spanned};
use syn::{
    parse_quote, spanned::Spanned, Data, DataEnum, DataStruct, DeriveInput, Fields, GenericParam,
    MacroDelimiter, Member, Meta, Path,
};

use crate::reflect::{attributes::ContainerAttributes, type_data::TypeDataRegistration};

const REFLECT_ATTRIBUTE_NAME: &str = "reflect";

pub(crate) fn derive_reflect(ast: DeriveInput) -> Result<TokenStream, syn::Error> {
    let bevy_reflect = BevyManifest::shared(|manifest| manifest.get_path("bevy_reflect2"));
    let mut container_attributes = ContainerAttributes::default();
    for attr in ast.attrs.iter() {
        match &attr.meta {
            Meta::List(meta_list) if meta_list.path.is_ident(REFLECT_ATTRIBUTE_NAME) => {
                if let MacroDelimiter::Paren(_) = meta_list.delimiter {
                    container_attributes.parse_meta_list(meta_list)?;
                } else {
                    return Err(syn::Error::new(
                        meta_list.delimiter.span().join(),
                        format_args!(
                            "`#[{REFLECT_ATTRIBUTE_NAME}(\"...\")]` must use parentheses `(` and `)`"
                        ),
                    ));
                }
            }
            _ => {}
        }
    }

    let (kind, kind_declarations) = kind(&ast, &bevy_reflect);

    let type_ident = &ast.ident;
    let type_ident_str = type_ident.to_string();
    let type_ident_len = type_ident_str.len();
    let type_path_setup = type_path_setup(&ast, &bevy_reflect, &type_ident_str);

    let (impl_generics, type_generics, where_clause) = &ast.generics.split_for_impl();

    let full_type: Path = parse_quote!(#type_ident #type_generics);
    let vtable = vtable(&container_attributes, &full_type, &bevy_reflect);
    let type_registration = type_registration(&ast, &bevy_reflect, &container_attributes.type_data);
    Ok(quote! {
        unsafe impl #impl_generics #bevy_reflect::Typed for #type_ident #type_generics #where_clause {
            const TYPE: &'static #bevy_reflect::Type = &{
                #type_path_setup
                let module_len = ::core::module_path!().len();
                let crate_len = match crate::find_ascii_char(::core::module_path!(), b':') {
                    Some(value) => value,
                    None => module_len,
                };
                #bevy_reflect::Type {
                    type_id: ::core::any::TypeId::of::<Self>(),
                    kind: #kind,
                    layout: Some(::core::alloc::Layout::new::<Self>()),
                    vtable: #vtable,
                    type_path: #bevy_reflect::TypePath::new(
                        full,
                        short,
                        (0, #type_ident_len),
                        Some((0, crate_len)),
                        Some((0, module_len)),
                    )
                }
            };
        }

        #type_registration
        #kind_declarations
    })
}

fn kind(ast: &DeriveInput, bevy_reflect: &Path) -> (TokenStream, TokenStream) {
    match &ast.data {
        Data::Struct(data_struct) => (struct_kind(data_struct, bevy_reflect), quote! {}),
        Data::Enum(data_enum) => enum_kind(ast, data_enum, bevy_reflect),
        Data::Union(_data_union) => todo!(),
    }
}

fn struct_kind(data_struct: &DataStruct, bevy_reflect: &Path) -> TokenStream {
    let field_kind = match &data_struct.fields {
        Fields::Named(_) => {
            quote! { #bevy_reflect::FieldKind::Named }
        }
        Fields::Unnamed(_) => {
            quote! { #bevy_reflect::FieldKind::Unnamed }
        }
        Fields::Unit => {
            quote! { #bevy_reflect::FieldKind::Unit }
        }
    };
    let mut fields = Vec::with_capacity(data_struct.fields.len());
    for (index, field) in data_struct.fields.iter().enumerate() {
        let (name, ident) = if let Some(ident) = &field.ident {
            (ident.to_string(), Member::Named(ident.clone()))
        } else {
            (index.to_string(), Member::Unnamed(index.into()))
        };
        let field_ty = &field.ty;
        fields.push(quote! {
            #bevy_reflect::Field {
                name: #name,
                offset: ::core::mem::offset_of!(Self, #ident),
                ty: #bevy_reflect::TypeRef(#bevy_reflect::Type::of::<#field_ty>),
            }
        })
    }
    quote! {
        #bevy_reflect::Kind::Struct(#bevy_reflect::Struct {
            kind: #field_kind,
            fields: &[
                #(#fields),*
            ],
        })
    }
}

fn enum_kind(
    ast: &DeriveInput,
    data_enum: &DataEnum,
    bevy_reflect: &Path,
) -> (TokenStream, TokenStream) {
    let type_ident = &ast.ident;
    let mut variants = Vec::with_capacity(data_enum.variants.len());
    let mut variant_field_accesses = Vec::with_capacity(data_enum.variants.len());
    let mut variant_indices = Vec::with_capacity(data_enum.variants.len());
    for (index, variant) in data_enum.variants.iter().enumerate() {
        let variant_ident = &variant.ident;

        let mut fields = Vec::with_capacity(variant.fields.len());
        let mut field_names = Vec::with_capacity(variant.fields.len());
        let mut field_index_cases = Vec::with_capacity(variant.fields.len());
        for (field_index, field) in variant.fields.iter().enumerate() {
            let name = if let Some(ident) = &field.ident {
                ident.to_string()
            } else {
                index.to_string()
            };
            let field_ty = &field.ty;
            fields.push(quote! {
                #bevy_reflect::EnumField {
                    name: #name,
                    ty: #bevy_reflect::TypeRef(#bevy_reflect::Type::of::<#field_ty>),
                }
            });
            let field_name = field
                .ident
                .clone()
                .unwrap_or_else(|| format_ident!("f{}", field_index));
            field_index_cases.push(quote! {
                #field_index => #FQOption::Some(#field_name)
            });
            field_names.push(field_name);
        }
        let (variant_kind, field_access, variant_index) = match variant.fields {
            Fields::Named(_) => (
                quote! {#bevy_reflect::FieldKind::Named},
                quote! {
                    #type_ident::#variant_ident { #(#field_names),* } => match index {
                        #(#field_index_cases,)*
                        _ => #FQOption::None,
                    }
                },
                quote! {
                    #type_ident::#variant_ident { .. } => #index
                },
            ),
            Fields::Unnamed(_) => (
                quote! {#bevy_reflect::FieldKind::Unnamed},
                quote! {
                    #type_ident::#variant_ident(#(#field_names),*) => match index {
                        #(#field_index_cases,)*
                        _ => #FQOption::None,
                    }
                },
                quote! {
                    #type_ident::#variant_ident( .. ) => #index
                },
            ),
            Fields::Unit => (
                quote! {#bevy_reflect::FieldKind::Unit},
                quote! {
                    #type_ident::#variant_ident => None
                },
                quote! {
                    #type_ident::#variant_ident => #index
                },
            ),
        };
        let name = variant.ident.to_string();
        variants.push(quote! {
            #bevy_reflect::Variant {
                name: #name,
                kind: #variant_kind,
                fields: &[
                    #(#fields),*
                ],
            }
        });
        variant_field_accesses.push(field_access);
        variant_indices.push(variant_index);
    }
    let kind = quote! {
        #bevy_reflect::Kind::Enum(#bevy_reflect::Enum {
            cast_enum_access: #bevy_reflect::cast_enum_access::<Self>,
            variants: &[
                #(#variants),*
            ],
        })
    };
    let (impl_generics, type_generics, where_clause) = &ast.generics.split_for_impl();
    let kind_declarations = quote! {
        unsafe impl #impl_generics #bevy_reflect::EnumAccess for #type_ident #type_generics #where_clause {
            fn field(&self, index: usize) -> #FQOption<&dyn #bevy_reflect::Reflect> {
                match self {
                    #(#variant_field_accesses,)*
                }
            }

            fn field_mut(&mut self, index: usize) -> #FQOption<&mut dyn #bevy_reflect::Reflect> {
                match self {
                    #(#variant_field_accesses,)*
                }
            }

            fn variant(&self) -> usize {
                match self {
                    #(#variant_indices,)*
                }
            }
        }
    };

    (kind, kind_declarations)
}

fn vtable(
    container_attributes: &ContainerAttributes,
    type_name: &Path,
    bevy_reflect: &Path,
) -> TokenStream {
    let default = container_attributes.default_impl(type_name, bevy_reflect);
    quote! {
        #bevy_reflect::TypeVTable {
            cast_reflect: #bevy_reflect::cast_reflect::<Self>,
            set_default: #default
        }
    }
}

fn type_registration(
    ast: &DeriveInput,
    bevy_reflect: &Path,
    type_data: &[TypeDataRegistration],
) -> TokenStream {
    let type_data = type_data.iter().map(|data| {
        let reflect_path = data.reflect_path();
        let args = data.args();

        let args = if args.is_empty() {
            // Set the span so that we get pointed to the correct identifier even when there are no type data arguments
            let span = reflect_path.span();
            quote_spanned!(span => ())
        } else {
            quote!((#args))
        };

        quote! {
            registration.register_type_data_with::<#reflect_path, Self, _>(#args);
        }
    });

    let type_ident = &ast.ident;
    let (impl_generics, type_generics, where_clause) = &ast.generics.split_for_impl();
    quote! {
        impl #impl_generics #bevy_reflect::GetTypeRegistration for #type_ident #type_generics #where_clause {
            fn get_type_registration() -> #bevy_reflect::TypeRegistration {
                let mut registration = #bevy_reflect::TypeRegistration::of::<Self>();
                #(#type_data)*
                registration
            }
        }
    }
}

fn type_path_setup(ast: &DeriveInput, bevy_reflect: &Path, type_ident_str: &str) -> TokenStream {
    let mut generic_type_paths = Vec::new();
    let mut generic_short_type_paths = Vec::new();
    for param in ast.generics.params.iter() {
        match param {
            GenericParam::Lifetime(_) => {}
            GenericParam::Const(_) => {}
            GenericParam::Type(type_param) => {
                let ident = &type_param.ident;
                generic_type_paths
                    .push(quote! {<#ident as #bevy_reflect::Typed>::TYPE.type_path().full()});
                generic_short_type_paths
                    .push(quote! {<#ident as #bevy_reflect::Typed>::TYPE.type_path().short()});
            }
        }
    }
    if generic_type_paths.is_empty() {
        quote! {
            let full = #bevy_reflect::concat!(::core::module_path!(), "::", #type_ident_str);
            let short = #type_ident_str;
        }
    } else {
        quote! {
            let full = #bevy_reflect::concat!(::core::module_path!(), "::", #type_ident_str, "<", #(#generic_type_paths,)* ">");
            let short = #bevy_reflect::concat!(#type_ident_str, "<", #(#generic_short_type_paths,)* ">");
        }
    }
}
