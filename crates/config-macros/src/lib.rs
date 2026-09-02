use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{
    parse_macro_input, Data, DeriveInput, Fields, GenericArgument, Ident, PathArguments, Type,
};

/// Generates typed `get_`/`set_`/`unset_` accessors on `Config` for every
/// `Option<T>` field of the annotated struct.
///
/// Each struct declares its own full path from the root `ConfigLayer` via
/// `#[config(path = "a.b.c")]`. The root itself omits the
/// attribute, which defaults to an empty path. Fields that aren't
/// `Option<T>` are assumed to be nested config structs deriving
/// `ConfigField` on their own, and are silently skipped.
#[proc_macro_derive(ConfigField, attributes(config))]
pub fn derive_config_field(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn expand(input: DeriveInput) -> syn::Result<TokenStream2> {
    let path = section_path(&input)?;

    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            &input,
            "`ConfigField` can only be derived for structs",
        ));
    };
    let Fields::Named(fields) = &data.fields else {
        return Err(syn::Error::new_spanned(
            &input,
            "`ConfigField` requires named fields",
        ));
    };

    let mut methods = Vec::new();
    for field in &fields.named {
        let field_ident = field.ident.as_ref().expect("named field");
        let Some(inner_ty) = option_inner_type(&field.ty) else {
            // Assumed to be a nested config struct deriving `ConfigField`
            // on its own, with its own `#[config(path = "...")]`.
            continue;
        };

        let mut segments = path.clone();
        segments.push(field_ident.clone());
        let method_suffix = segments
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("_");

        let getter = format_ident!("get_{method_suffix}");
        let setter = format_ident!("set_{method_suffix}");
        let unsetter = format_ident!("unset_{method_suffix}");
        let access = quote! { #(#segments).* };

        let get_doc = format!(
            "Retrieve the `{method_suffix}`.\nIt traverses all config layers until it finds a set value."
        );
        let set_doc = format!(
            "Set the `{method_suffix}` only for this layer.\nThe base config is not affected."
        );
        let unset_doc = format!(
            "Unset the `{method_suffix}` only for this layer.\nThe base config is not affected."
        );

        methods.push(quote! {
            #[doc = #get_doc]
            pub fn #getter(&self) -> #inner_ty {
                self.0.resolve(|layer| layer.#access.clone())
            }

            #[doc = #set_doc]
            pub fn #setter(&self, value: #inner_ty) {
                self.0.layer.write().#access = Some(value);
            }

            #[doc = #unset_doc]
            pub fn #unsetter(&self) {
                self.0.layer.write().#access = None;
            }
        });
    }

    Ok(quote! {
        impl Config {
            #(#methods)*
        }
    })
}

/// Reads the dot-separated path from `#[config(path = "a.b.c")]`.
/// Defaults to an empty path (the root) when the attribute is absent.
fn section_path(input: &DeriveInput) -> syn::Result<Vec<Ident>> {
    let mut path = None;

    for attr in &input.attrs {
        if !attr.path().is_ident("config") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("path") {
                let lit: syn::LitStr = meta.value()?.parse()?;
                path = Some(lit);
                Ok(())
            } else {
                Err(meta.error("unsupported `config` attribute, expected `path`"))
            }
        })?;
    }

    let Some(path) = path else {
        return Ok(Vec::new());
    };

    path.value()
        .split('.')
        .map(|segment| syn::parse_str::<Ident>(segment))
        .collect::<syn::Result<Vec<_>>>()
        .map_err(|err| syn::Error::new_spanned(&path, format!("invalid `path` segment: {err}")))
}

/// Extracts `T` out of `Option<T>`, or `None` if `ty` isn't `Option<...>`.
fn option_inner_type(ty: &Type) -> Option<&Type> {
    let Type::Path(type_path) = ty else {
        return None;
    };
    let segment = type_path.path.segments.last()?;
    if segment.ident != "Option" {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &segment.arguments else {
        return None;
    };
    args.args.iter().find_map(|arg| match arg {
        GenericArgument::Type(ty) => Some(ty),
        _ => None,
    })
}
