//! `#[derive(Schema)]` for Fynkel components and wire records.
//!
//! ```ignore
//! #[derive(Schema)]
//! #[schema(kind = "component", id = 1, name = "fynkel.Transform")]
//! pub struct Transform {
//!     /// Metres.
//!     pub position: Vec3,
//!     pub rotation: Quat,
//!     pub scale: Vec3,
//! }
//! ```
//!
//! Field types are mapped by their last path segment: `f32 u32 i32 u64 bool
//! Vec3 Quat Vec4 Entity String AssetRef`, `Option<Entity>`,
//! `Option<AssetRef>`; any other type must
//! implement `SchemaField`, which this derive also emits — so nested records
//! and enums are just other `#[derive(Schema)]` types. Doc comments become
//! the schema docs (and end up in JSON Schema / Dart docs). `authored =
//! false` marks an engine-managed component that scene documents may not
//! contain. `document = "material"` on a record makes it the top level of
//! that document kind: every field optional, `Default` when absent.
//!
//! On a unit-variant enum the derive emits `HasEnumSchema`: the wire carries
//! the declaration index, documents the lowerCamel variant name.
//!
//! ```ignore
//! #[derive(Schema)]
//! #[schema(name = "fynkel.Projection")]
//! pub enum Projection { Perspective, Orthographic }
//! ```

use proc_macro::TokenStream;
use quote::quote;
use syn::{
    parse_macro_input, Data, DeriveInput, Expr, ExprLit, Fields, GenericArgument, Lit, Meta,
    PathArguments, Type,
};

#[proc_macro_derive(Schema, attributes(schema))]
pub fn derive_schema(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand(input) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

struct Attrs {
    kind: String,
    id: u32,
    name: String,
    authored: bool,
    document: Option<String>,
}

fn parse_attrs(input: &DeriveInput) -> syn::Result<Attrs> {
    let mut kind = None;
    let mut id = 0u32;
    let mut name = None;
    let mut authored = true;
    let mut document = None;
    for attr in &input.attrs {
        if !attr.path().is_ident("schema") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            let value: Expr = meta.value()?.parse()?;
            let lit = match value {
                Expr::Lit(ExprLit { lit, .. }) => lit,
                _ => return Err(meta.error("expected literal")),
            };
            if meta.path.is_ident("kind") {
                if let Lit::Str(s) = lit {
                    kind = Some(s.value());
                    return Ok(());
                }
            } else if meta.path.is_ident("id") {
                if let Lit::Int(i) = lit {
                    id = i.base10_parse()?;
                    return Ok(());
                }
            } else if meta.path.is_ident("name") {
                if let Lit::Str(s) = lit {
                    name = Some(s.value());
                    return Ok(());
                }
            } else if meta.path.is_ident("authored") {
                if let Lit::Bool(b) = lit {
                    authored = b.value();
                    return Ok(());
                }
            } else if meta.path.is_ident("document") {
                if let Lit::Str(s) = lit {
                    document = Some(s.value());
                    return Ok(());
                }
            }
            Err(meta.error(
                "expected kind = \"...\", id = N, name = \"...\", authored = true|false, document = \"...\"",
            ))
        })?;
    }
    let kind = kind.unwrap_or_else(|| "record".to_string());
    if !["component", "command", "event", "record"].contains(&kind.as_str()) {
        return Err(syn::Error::new_spanned(
            input,
            "schema kind must be component | command | event | record",
        ));
    }
    if kind != "record" && id == 0 {
        return Err(syn::Error::new_spanned(
            input,
            "components, commands and events need a non-zero `id`",
        ));
    }
    if document.is_some() && kind != "record" {
        return Err(syn::Error::new_spanned(
            input,
            "`document = ...` is for plain records (a document's top level)",
        ));
    }
    let name = name.unwrap_or_else(|| format!("fynkel.{}", input.ident));
    Ok(Attrs {
        kind,
        id,
        name,
        authored,
        document,
    })
}

fn doc_of(attrs: &[syn::Attribute]) -> String {
    let mut lines = Vec::new();
    for a in attrs {
        if let Meta::NameValue(nv) = &a.meta {
            if nv.path.is_ident("doc") {
                if let Expr::Lit(ExprLit {
                    lit: Lit::Str(s), ..
                }) = &nv.value
                {
                    lines.push(s.value().trim().to_string());
                }
            }
        }
    }
    lines.join(" ")
}

fn field_type(ty: &Type) -> syn::Result<proc_macro2::TokenStream> {
    let Type::Path(p) = ty else {
        return Err(syn::Error::new_spanned(ty, "unsupported field type"));
    };
    let seg = p.path.segments.last().unwrap();
    let ident = seg.ident.to_string();
    let c = quote!(::fynkel_contracts::schema::FieldType);
    Ok(match ident.as_str() {
        "f32" => quote!(#c::F32),
        "u32" => quote!(#c::U32),
        "i32" => quote!(#c::I32),
        "u64" => quote!(#c::U64),
        "bool" => quote!(#c::Bool),
        "Vec3" => quote!(#c::Vec3),
        "Quat" => quote!(#c::Quat),
        "Vec4" => quote!(#c::Vec4),
        "Entity" => quote!(#c::Entity),
        "String" => quote!(#c::Str),
        "AssetRef" => quote!(#c::AssetRef),
        "Option" => {
            let PathArguments::AngleBracketed(args) = &seg.arguments else {
                return Err(syn::Error::new_spanned(ty, "Option needs a type argument"));
            };
            match args.args.first() {
                Some(GenericArgument::Type(Type::Path(inner)))
                    if inner.path.segments.last().unwrap().ident == "Entity" =>
                {
                    quote!(#c::OptionalEntity)
                }
                Some(GenericArgument::Type(Type::Path(inner)))
                    if inner.path.segments.last().unwrap().ident == "AssetRef" =>
                {
                    quote!(#c::OptionalAssetRef)
                }
                _ => {
                    return Err(syn::Error::new_spanned(
                        ty,
                        "only Option<Entity> and Option<AssetRef> are supported",
                    ))
                }
            }
        }
        _ => quote!(<#ty as ::fynkel_contracts::schema::SchemaField>::TYPE),
    })
}

fn lower_camel(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(first) => first.to_lowercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

fn expand_enum(input: &DeriveInput, data: &syn::DataEnum) -> syn::Result<proc_macro2::TokenStream> {
    let attrs = parse_attrs(input)?;
    if attrs.kind != "record" || attrs.id != 0 {
        return Err(syn::Error::new_spanned(
            input,
            "enums take only `name = \"...\"` (they are field types, not records)",
        ));
    }
    let mut names = Vec::new();
    let mut idents = Vec::new();
    for v in &data.variants {
        if !matches!(v.fields, Fields::Unit) {
            return Err(syn::Error::new_spanned(
                v,
                "Schema enums need unit variants only",
            ));
        }
        if v.discriminant.is_some() {
            return Err(syn::Error::new_spanned(
                v,
                "Schema enums use declaration order; drop the explicit discriminant",
            ));
        }
        names.push(lower_camel(&v.ident.to_string()));
        idents.push(&v.ident);
    }
    if idents.is_empty() {
        return Err(syn::Error::new_spanned(
            input,
            "Schema enums need a variant",
        ));
    }
    let ident = &input.ident;
    let ident_str = ident.to_string();
    let name = attrs.name;
    let doc = doc_of(&input.attrs);
    let index = 0u32..;
    let from_arms = idents
        .iter()
        .zip(index.clone())
        .map(|(v, i)| quote!(#i => Some(Self::#v),));
    let to_arms = idents
        .iter()
        .zip(index)
        .map(|(v, i)| quote!(Self::#v => #i,));
    Ok(quote! {
        impl ::fynkel_contracts::schema::HasEnumSchema for #ident {
            const ENUM: ::fynkel_contracts::schema::EnumSchema =
                ::fynkel_contracts::schema::EnumSchema {
                    name: #name,
                    rust_path: concat!(module_path!(), "::", #ident_str),
                    doc: #doc,
                    variants: &[ #(#names),* ],
                };
            fn to_index(&self) -> u32 {
                match self { #(#to_arms)* }
            }
            fn from_index(index: u32) -> Option<Self> {
                match index { #(#from_arms)* _ => None }
            }
        }
        impl ::fynkel_contracts::schema::SchemaField for #ident {
            const TYPE: ::fynkel_contracts::schema::FieldType =
                ::fynkel_contracts::schema::FieldType::Enum(
                    &<#ident as ::fynkel_contracts::schema::HasEnumSchema>::ENUM,
                );
        }
    })
}

fn expand(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    if let Data::Enum(data) = &input.data {
        return expand_enum(&input, data);
    }
    let attrs = parse_attrs(&input)?;
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            &input,
            "Schema derives on structs and unit-variant enums only",
        ));
    };
    let fields: Vec<_> = match &data.fields {
        Fields::Named(named) => named.named.iter().collect(),
        Fields::Unit => Vec::new(),
        Fields::Unnamed(_) => {
            return Err(syn::Error::new_spanned(
                &input,
                "Schema needs named fields (or a unit struct)",
            ))
        }
    };
    let mut field_tokens = Vec::new();
    for f in fields {
        let name = f.ident.as_ref().unwrap().to_string();
        let ty = field_type(&f.ty)?;
        let doc = doc_of(&f.attrs);
        field_tokens.push(quote! {
            ::fynkel_contracts::schema::FieldSchema { name: #name, ty: #ty, doc: #doc }
        });
    }
    let ident = &input.ident;
    let ident_str = ident.to_string();
    let kind = match attrs.kind.as_str() {
        "component" => quote!(::fynkel_contracts::schema::SchemaKind::Component),
        "command" => quote!(::fynkel_contracts::schema::SchemaKind::Command),
        "event" => quote!(::fynkel_contracts::schema::SchemaKind::Event),
        _ => quote!(::fynkel_contracts::schema::SchemaKind::Record),
    };
    let id = attrs.id;
    let name = attrs.name;
    let authored = attrs.authored;
    let document = match &attrs.document {
        Some(d) => quote!(Some(#d)),
        None => quote!(None),
    };
    let doc = doc_of(&input.attrs);
    Ok(quote! {
        impl ::fynkel_contracts::schema::HasSchema for #ident {
            const SCHEMA: ::fynkel_contracts::schema::RecordSchema =
                ::fynkel_contracts::schema::RecordSchema {
                    kind: #kind,
                    id: #id,
                    name: #name,
                    rust_path: concat!(module_path!(), "::", #ident_str),
                    doc: #doc,
                    fields: &[ #(#field_tokens),* ],
                    authored: #authored,
                    document: #document,
                };
        }
        impl ::fynkel_contracts::schema::SchemaField for #ident {
            const TYPE: ::fynkel_contracts::schema::FieldType =
                ::fynkel_contracts::schema::FieldType::Nested(
                    &<#ident as ::fynkel_contracts::schema::HasSchema>::SCHEMA,
                );
        }
    })
}
