use bevy_macro_utils::{fq_std::FQOption, terminated_parser};
use proc_macro2::{Span, TokenStream};
use quote::{quote, quote_spanned};
use syn::{
    ext::IdentExt, parenthesized, parse::ParseStream, spanned::Spanned, token::Paren, Ident,
    MetaList, Path, Result, Token,
};

use crate::reflect::type_data::TypeDataRegistration;

#[derive(Default)]
pub(crate) struct ContainerAttributes {
    pub(crate) default: TraitImpl,
    // clone: TraitImpl,
    // debug: TraitImpl,
    // hash: TraitImpl,
    // partial_ord: TraitImpl,
    // partial_eq: TraitImpl,
    // custom_attributes: CustomAttributes,
    pub(crate) type_data: Vec<TypeDataRegistration>,
}

impl ContainerAttributes {
    /// Parse a comma-separated list of container attributes.
    ///
    /// # Example
    /// - `Hash, Debug(custom_debug), MyTrait`
    pub fn parse_terminated(&mut self, input: ParseStream) -> Result<()> {
        terminated_parser(Token![,], |stream| self.parse_container_attribute(stream))(input)?;

        Ok(())
    }

    /// Parse the contents of a `#[reflect(...)]` attribute into a [`ContainerAttributes`] instance.
    ///
    /// # Example
    /// - `#[reflect(Hash, Debug(custom_debug), MyTrait)]`
    /// - `#[reflect(no_field_bounds)]`
    pub fn parse_meta_list(&mut self, meta: &MetaList) -> Result<()> {
        meta.parse_args_with(|stream: ParseStream| self.parse_terminated(stream))
    }

    /// Parse a single container attribute.
    fn parse_container_attribute(&mut self, input: ParseStream) -> Result<()> {
        let lookahead = input.lookahead1();
        if lookahead.peek(kw::Default) {
            self.parse_default(input)
        } else if lookahead.peek(Ident::peek_any) {
            self.parse_type_data(input)
        } else {
            Err(lookahead.error())
        }
    }
    ///
    /// Parse a type data registration.
    ///
    /// Examples:
    /// - `#[reflect(Default)]`
    /// - `#[reflect(Hash(custom_hash_fn))]`
    fn parse_type_data(&mut self, input: ParseStream) -> Result<()> {
        let type_data = input.parse::<TypeDataRegistration>()?;

        if self
            .type_data
            .iter()
            .any(|existing| existing.path() == type_data.path())
        {
            return Err(syn::Error::new(
                type_data.path().span(),
                CONFLICTING_TYPE_DATA_MESSAGE,
            ));
        }

        self.type_data.push(type_data);

        Ok(())
    }

    /// Parse `default` attribute.
    ///
    /// Examples:
    /// - `#[reflect(Default)]`
    /// - `#[reflect(Default(custom_default_fn))]`
    fn parse_default(&mut self, input: ParseStream) -> Result<()> {
        let ident = input.parse::<kw::Default>()?;

        if input.peek(Paren) {
            let content;
            parenthesized!(content in input);
            let path = content.parse::<Path>()?;
            self.default.merge(TraitImpl::Custom(path, ident.span))?;
        } else {
            self.default = TraitImpl::Implemented(ident.span);
        }

        Ok(())
    }

    /// Returns the implementation of `PartialReflect::reflect_hash` as a `TokenStream`.
    ///
    /// If `Hash` was not registered, returns `None`.
    pub fn default_impl(&self, type_name: &Path, bevy_reflect: &Path) -> TokenStream {
        match &self.default {
            &TraitImpl::Implemented(span) => quote_spanned! {span=>
                #FQOption::Some(#bevy_reflect::set_default::<Self>)
            },
            &TraitImpl::Custom(ref impl_fn, span) => quote_spanned! {span=>
                #FQOption::Some({
                    unsafe fn custom_default(ptr: *mut u8) {
                        // SAFETY: caller verifies that ptr is a valid allocation of type T, and it corresponds
                        // to a unique, mutable reference
                        unsafe {
                            *(ptr as *mut #type_name) = #impl_fn();
                        }
                    }
                    custom_default
                })
            },
            TraitImpl::NotImplemented => {
                quote! {None}
            }
        }
    }
}

// The error message to show when a trait/type is specified multiple times
const CONFLICTING_TYPE_DATA_MESSAGE: &str = "conflicting type data registration";

/// A marker for trait implementations registered via the `Reflect` derive macro.
#[derive(Clone, Default)]
pub(crate) enum TraitImpl {
    /// The trait is not registered as implemented.
    #[default]
    NotImplemented,

    /// The trait is registered as implemented.
    Implemented(Span),

    /// The trait is registered with a custom function rather than an actual implementation.
    Custom(Path, Span),
}

impl TraitImpl {
    /// Merges this [`TraitImpl`] with another.
    ///
    /// Update `self` with whichever value is not [`TraitImpl::NotImplemented`].
    /// If `other` is [`TraitImpl::NotImplemented`], then `self` is not modified.
    /// An error is returned if neither value is [`TraitImpl::NotImplemented`].
    pub fn merge(&mut self, other: TraitImpl) -> Result<()> {
        match (&self, other) {
            (TraitImpl::NotImplemented, value) => {
                *self = value;
                Ok(())
            }
            (_, TraitImpl::NotImplemented) => Ok(()),
            (_, TraitImpl::Implemented(span) | TraitImpl::Custom(_, span)) => {
                Err(syn::Error::new(span, CONFLICTING_TYPE_DATA_MESSAGE))
            }
        }
    }
}

mod kw {
    // syn::custom_keyword!(Debug);
    // syn::custom_keyword!(PartialEq);
    // syn::custom_keyword!(PartialOrd);
    // syn::custom_keyword!(Hash);
    // syn::custom_keyword!(Clone);
    syn::custom_keyword!(Default);
}
