use proc_macro::TokenStream;
use quote::{quote, ToTokens};
use syn::Path;

pub fn type_path(bevy_reflect: &Path, mut path: Path) -> TokenStream {
    path.leading_colon = None;
    if path.segments.is_empty() {
        panic!("Must provide a type path!");
    }

    match path.segments.len() {
        0 => panic!("Must provide a type path!"),
        1 => {
            let segment = &path.segments[0];
            let type_name = segment.ident.to_string();
            let type_name_len = type_name.len();
            quote! {
                #bevy_reflect::TypePath::new(
                    #type_name,
                    #type_name,
                    (0, #type_name_len),
                    None,
                    None,
                )
            }
        }
        _ => {
            let mut type_path = String::new();
            let mut segments = path.segments.iter();
            // crate segment is guaranteed to exist
            let crate_segment = segments.next().unwrap();
            let crate_string = crate_segment.ident.to_string();
            type_path.push_str(&crate_string);
            type_path.push_str("::");

            while segments.len() > 1 {
                let mod_segment = segments.next().unwrap();
                let mod_string = mod_segment.ident.to_string();
                type_path.push_str(&mod_string);
                type_path.push_str("::");
            }

            let mod_end = type_path.len() - 2;
            let type_ident_start = type_path.len();

            let segment = segments.next().unwrap();
            let type_name = segment.ident.to_string();
            let full_generics = format!(
                "{}{}",
                type_name,
                segment.arguments.to_token_stream().to_string()
            );
            type_path.push_str(&full_generics);

            let type_ident_end = type_ident_start + type_name.len();
            let crate_end = crate_string.len();

            quote! {
                #bevy_reflect::TypePath::new(
                    #type_path,
                    #type_name, // TODO: implement short
                    (#type_ident_start, #type_ident_end),
                    Some((0, #crate_end)),
                    Some((0, #mod_end)),
                )
            }
        }
    }
    .into()
}
