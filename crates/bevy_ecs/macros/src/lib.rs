extern crate proc_macro;

use find_crate::Manifest;
use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::{format_ident, quote};
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input,
    punctuated::Punctuated,
    token::Comma,
    Data, DataStruct, DeriveInput, Field, Fields, GenericParam, Ident, Index, Lifetime, LitInt,
    Path, Result, Token,
};

struct AllTuples {
    macro_ident: Ident,
    start: usize,
    end: usize,
    idents: Vec<Ident>,
}

impl Parse for AllTuples {
    fn parse(input: ParseStream) -> Result<Self> {
        let macro_ident = input.parse::<Ident>()?;
        input.parse::<Comma>()?;
        let start = input.parse::<LitInt>()?.base10_parse()?;
        input.parse::<Comma>()?;
        let end = input.parse::<LitInt>()?.base10_parse()?;
        input.parse::<Comma>()?;
        let mut idents = Vec::new();
        while let Ok(ident) = input.parse::<Ident>() {
            idents.push(ident);
            let _ = input.parse::<Comma>();
        }

        Ok(AllTuples {
            macro_ident,
            start,
            end,
            idents,
        })
    }
}

#[proc_macro]
pub fn all_tuples(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as AllTuples);
    let len = input.end - input.start;
    let mut ident_tuples = Vec::with_capacity(len);
    for i in input.start..input.end {
        let idents = input
            .idents
            .iter()
            .map(|ident| format_ident!("{}{}", ident, i));
        if input.idents.len() < 2 {
            ident_tuples.push(quote! {
                #(#idents)*
            });
        } else {
            ident_tuples.push(quote! {
                (#(#idents),*)
            });
        }
    }

    let macro_ident = &input.macro_ident;
    let invocations = (input.start..input.end).map(|i| {
        let ident_tuples = &ident_tuples[0..i];
        quote! {
            #macro_ident!(#(#ident_tuples),*);
        }
    });
    TokenStream::from(quote! {
        #(
            #invocations
        )*
    })
}

// static BUNDLE_ATTRIBUTE_NAME: &str = "bundle";

#[proc_macro_derive(Bundle, attributes(bundle))]
pub fn derive_bundle(input: TokenStream) -> TokenStream {
    let ast = parse_macro_input!(input as DeriveInput);
    let manifest = Manifest::new().unwrap();
    let path_str = if let Some(package) = manifest.find(|name| name == "bevy") {
        format!("{}::ecs", package.name)
    } else if let Some(package) = manifest.find(|name| name == "bevy_internal") {
        format!("{}::ecs", package.name)
    } else if let Some(package) = manifest.find(|name| name == "bevy_ecs") {
        package.name
    } else {
        "bevy_ecs".to_string()
    };
    let crate_name: Path = syn::parse(path_str.parse::<TokenStream>().unwrap()).unwrap();

    let named_fields = match &ast.data {
        Data::Struct(DataStruct {
            fields: Fields::Named(fields),
            ..
        }) => &fields.named,
        _ => panic!("Expected a struct with named fields."),
    };

    // let is_bundle = named_fields
    //     .iter()
    //     .map(|field| {
    //         field
    //             .attrs
    //             .iter()
    //             .any(|a| *a.path.get_ident().as_ref().unwrap() == BUNDLE_ATTRIBUTE_NAME)
    //     })
    //     .collect::<Vec<bool>>();
    let field = named_fields
        .iter()
        .map(|field| field.ident.as_ref().unwrap())
        .collect::<Vec<_>>();
    let field_type = named_fields
        .iter()
        .map(|field| &field.ty)
        .collect::<Vec<_>>();

    let generics = ast.generics;
    let (impl_generics, ty_generics, _where_clause) = generics.split_for_impl();
    let struct_name = &ast.ident;

    TokenStream::from(quote! {
        impl #impl_generics #crate_name::core::DynamicBundle for #struct_name#ty_generics {
            fn type_info(&self) -> Vec<#crate_name::core::TypeInfo> {
                Self::static_type_info()
            }

            #[allow(unused_variables, unused_mut)]
            unsafe fn put(self, mut func: impl FnMut(*mut u8)) {
                #(
                    let mut field = self.#field;
                    func((&mut field as *mut #field_type).cast::<u8>());
                    std::mem::forget(field);
                )*
            }
        }

        impl #impl_generics #crate_name::core::Bundle for #struct_name#ty_generics {
            fn static_type_info() -> Vec<#crate_name::core::TypeInfo> {
                vec![#(#crate_name::core::TypeInfo::of::<#field_type>()),*]
            }

            #[allow(unused_variables, unused_mut, non_snake_case)]
            unsafe fn get(mut func: impl FnMut() -> *mut u8) -> Self {
                Self {
                    #(#field: func().cast::<#field_type>().read(),)*
                }
            }
        }
    })
}

fn get_idents(fmt_string: fn(usize) -> String, count: usize) -> Vec<Ident> {
    (0..count)
        .map(|i| Ident::new(&fmt_string(i), Span::call_site()))
        .collect::<Vec<Ident>>()
}

fn get_lifetimes(fmt_string: fn(usize) -> String, count: usize) -> Vec<Lifetime> {
    (0..count)
        .map(|i| Lifetime::new(&fmt_string(i), Span::call_site()))
        .collect::<Vec<Lifetime>>()
}

#[proc_macro]
pub fn impl_query_set(_input: TokenStream) -> TokenStream {
    let mut tokens = TokenStream::new();
    let max_queries = 4;
    let queries = get_idents(|i| format!("Q{}", i), max_queries);
    let filters = get_idents(|i| format!("F{}", i), max_queries);
    let lifetimes = get_lifetimes(|i| format!("'q{}", i), max_queries);
    let mut query_fns = Vec::new();
    let mut query_fn_muts = Vec::new();
    for i in 0..max_queries {
        let query = &queries[i];
        let filter = &filters[i];
        let lifetime = &lifetimes[i];
        let fn_name = Ident::new(&format!("q{}", i), Span::call_site());
        let fn_name_mut = Ident::new(&format!("q{}_mut", i), Span::call_site());
        let index = Index::from(i);
        query_fns.push(quote! {
            pub fn #fn_name(&self) -> &Query<#lifetime, #query, #filter> {
                &self.0.#index
            }
        });
        query_fn_muts.push(quote! {
            pub fn #fn_name_mut(&mut self) -> &mut Query<#lifetime, #query, #filter> {
                &mut self.0.#index
            }
        });
    }

    for query_count in 1..=max_queries {
        let query = &queries[0..query_count];
        let filter = &filters[0..query_count];
        let lifetime = &lifetimes[0..query_count];
        let query_fn = &query_fns[0..query_count];
        let query_fn_mut = &query_fn_muts[0..query_count];
        tokens.extend(TokenStream::from(quote! {
            impl<#(#lifetime,)*  #(#query: WorldQuery + 'static,)* #(#filter: QueryFilter + 'static,)*> SystemParam for QuerySet<(#(Query<#lifetime, #query, #filter>,)*)> {
                type Fetch = QuerySetState<(#(QueryState<#query, #filter>,)*)>;
            }

            // SAFE: Relevant query ComponentId and ArchetypeComponentId access is applied to SystemState. If any QueryState conflicts
            // with any prior access, a panic will occur.
            unsafe impl<#(#query: WorldQuery + 'static,)* #(#filter: QueryFilter + 'static,)*> SystemParamState for QuerySetState<(#(QueryState<#query, #filter>,)*)> {
                fn init(world: &mut World, system_state: &mut SystemState) -> Self {
                    #(
                        let mut #query = QueryState::<#query, #filter>::new(world);
                        assert_component_access_compatibility(
                            &system_state.name,
                            std::any::type_name::<#query>(),
                            std::any::type_name::<#filter>(),
                            &#query.component_access,
                            &system_state.component_access,
                            world,
                        );
                    )*
                    #(
                        system_state
                            .component_access
                            .extend(&#query.component_access);
                        system_state
                            .archetype_component_access
                            .extend(&#query.archetype_component_access);
                    )*
                    QuerySetState((#(#query,)*))
                }

                fn update(&mut self, world: &World, system_state: &mut SystemState) {
                    let (#(#query,)*) = &mut self.0;
                    #(#query.update(world, system_state);)*
                }
            }

            impl<'a, #(#query: WorldQuery + 'static,)* #(#filter: QueryFilter + 'static,)*> SystemParamFetch<'a> for QuerySetState<(#(QueryState<#query, #filter>,)*)> {
                type Item = QuerySet<(#(Query<'a, #query, #filter>,)*)>;

                #[inline]
                unsafe fn get_param(
                    state: &'a mut Self,
                    _system_state: &'a SystemState,
                    world: &'a World,
                ) -> Option<Self::Item> {
                    let (#(#query,)*) = &state.0;
                    Some(QuerySet((#(Query::new(world, #query),)*)))
                }
            }

            impl<#(#lifetime,)* #(#query: WorldQuery,)* #(#filter: QueryFilter,)*> QuerySet<(#(Query<#lifetime, #query, #filter>,)*)> {
                #(#query_fn)*
                #(#query_fn_mut)*
            }
        }));
    }

    tokens
}

#[derive(Default)]
struct SystemParamFieldAttributes {
    pub ignore: bool,
}

static SYSTEM_PARAM_ATTRIBUTE_NAME: &str = "system_param";

#[proc_macro_derive(SystemParam, attributes(system_param))]
pub fn derive_system_param(input: TokenStream) -> TokenStream {
    let ast = parse_macro_input!(input as DeriveInput);
    let fields = match &ast.data {
        Data::Struct(DataStruct {
            fields: Fields::Named(fields),
            ..
        }) => &fields.named,
        _ => panic!("Expected a struct with named fields."),
    };

    let manifest = Manifest::new().unwrap();
    let path_str = if let Some(package) = manifest.find(|name| name == "bevy") {
        format!("{}::ecs", package.name)
    } else {
        "bevy_ecs".to_string()
    };
    let path: Path = syn::parse(path_str.parse::<TokenStream>().unwrap()).unwrap();

    let field_attributes = fields
        .iter()
        .map(|field| {
            (
                field,
                field
                    .attrs
                    .iter()
                    .find(|a| *a.path.get_ident().as_ref().unwrap() == SYSTEM_PARAM_ATTRIBUTE_NAME)
                    .map_or_else(SystemParamFieldAttributes::default, |a| {
                        syn::custom_keyword!(ignore);
                        let mut attributes = SystemParamFieldAttributes::default();
                        a.parse_args_with(|input: ParseStream| {
                            if input.parse::<Option<ignore>>()?.is_some() {
                                attributes.ignore = true;
                            }
                            Ok(())
                        })
                        .expect("Invalid 'render_resources' attribute format.");

                        attributes
                    }),
            )
        })
        .collect::<Vec<(&Field, SystemParamFieldAttributes)>>();
    let mut fields = Vec::new();
    let mut field_indices = Vec::new();
    let mut field_types = Vec::new();
    let mut ignored_fields = Vec::new();
    let mut ignored_field_types = Vec::new();
    for (i, (field, attrs)) in field_attributes.iter().enumerate() {
        if attrs.ignore {
            ignored_fields.push(field.ident.as_ref().unwrap());
            ignored_field_types.push(&field.ty);
        } else {
            fields.push(field.ident.as_ref().unwrap());
            field_types.push(&field.ty);
            field_indices.push(Index::from(i));
        }
    }

    let generics = ast.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let lifetimeless_generics: Vec<_> = generics
        .params
        .iter()
        .filter(|g| matches!(g, GenericParam::Type(_)))
        .collect();

    let mut punctuated_generics = Punctuated::<_, Token![,]>::new();
    punctuated_generics.extend(lifetimeless_generics.iter());

    let mut punctuated_generic_idents = Punctuated::<_, Token![,]>::new();
    punctuated_generic_idents.extend(lifetimeless_generics.iter().map(|g| match g {
        GenericParam::Type(g) => &g.ident,
        _ => panic!(),
    }));

    let struct_name = &ast.ident;
    let fetch_struct_name = Ident::new(&format!("{}State", struct_name), Span::call_site());

    TokenStream::from(quote! {
        impl #impl_generics #path::system::SystemParam for #struct_name#ty_generics #where_clause {
            type Fetch = #fetch_struct_name <(#(<#field_types as SystemParam>::Fetch,)*), #punctuated_generic_idents>;
        }

        pub struct #fetch_struct_name<TSystemParamState, #punctuated_generic_idents> {
            state: TSystemParamState,
            marker: std::marker::PhantomData<(#punctuated_generic_idents)>
        }

        unsafe impl<TSystemParamState: #path::system::SystemParamState, #punctuated_generics> #path::system::SystemParamState for #fetch_struct_name<TSystemParamState, #punctuated_generic_idents> {
            fn init(world: &mut #path::core::World, system_state: &mut #path::system::SystemState) -> Self {
                Self {
                    state: TSystemParamState::init(world, system_state),
                    marker: std::marker::PhantomData,
                }
            }

            fn update(&mut self, world: &#path::core::World, system_state: &mut #path::system::SystemState) {
                self.state.update(world, system_state)
            }
        }

        impl #impl_generics #path::system::SystemParamFetch<'a> for #fetch_struct_name <(#(<#field_types as SystemParam>::Fetch,)*), #punctuated_generic_idents> {
            type Item = #struct_name#ty_generics;
            unsafe fn get_param(
                state: &'a mut Self,
                system_state: &'a #path::system::SystemState,
                world: &'a #path::core::World,
            ) -> Option<Self::Item> {
                Some(#struct_name {
                    #(#fields: <<#field_types as SystemParam>::Fetch as #path::system::SystemParamFetch>::get_param(&mut state.state.#field_indices, system_state, world)?,)*
                    #(#ignored_fields: <#ignored_field_types>::default(),)*
                })
            }
        }
    })
}
