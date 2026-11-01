use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{Fields, ItemStruct, Meta, ext::IdentExt, parse::Parser};

pub fn expand(arguments: TokenStream, input: ItemStruct) -> syn::Result<TokenStream> {
    let mut constructor = false;
    syn::meta::parser(|meta| {
        if !meta.path.is_ident("constructor") {
            return Err(meta.error("expected constructor"));
        }
        constructor = true;
        Ok(())
    })
    .parse2(arguments)?;
    let constructor = constructor.then(|| quote!(#[constructor]));
    // A derive sees only fields retained by cfg, so its schema and accessor
    // indices also match when cfg_attr selects getter or setter attributes.
    Ok(quote! {
        #[derive(::wasmgc::__private::BuildGcStruct)]
        #constructor
        #input
    })
}

pub fn derive(input: ItemStruct) -> syn::Result<TokenStream> {
    let constructor = input
        .attrs
        .iter()
        .any(|attribute| attribute.path().is_ident("constructor"));
    if !input.generics.params.is_empty() || input.generics.where_clause.is_some() {
        return Err(syn::Error::new_spanned(
            input.generics,
            "wasmgc requires a concrete, non-generic struct",
        ));
    }
    let field_exports = input
        .fields
        .iter()
        .map(field_exports)
        .collect::<syn::Result<Vec<_>>>()?;
    let Fields::Named(fields) = &input.fields else {
        return Err(syn::Error::new_spanned(
            &input.fields,
            "wasmgc requires named fields",
        ));
    };
    let fields = &fields.named;
    let name = &input.ident;
    let visibility = &input.vis;
    let view = format_ident!("{}Ref", name.unraw());
    let names: Vec<_> = fields
        .iter()
        .map(|field| field.ident.as_ref().unwrap())
        .collect();
    let types: Vec<_> = fields.iter().map(|field| &field.ty).collect();
    let field_count = fields.len();
    let arguments: Vec<_> = (0..field_count)
        .map(|i| format_ident!("__wasmgc_field_{i}"))
        .collect();
    let field_names: Vec<_> = names.iter().map(|name| name.unraw().to_string()).collect();
    let getters: Vec<_> = field_names
        .iter()
        .map(|name| format_ident!("__get_{name}"))
        .collect();
    let setters: Vec<_> = field_names
        .iter()
        .map(|name| format_ident!("__set_{name}"))
        .collect();
    let abi_types: Vec<_> = types
        .iter()
        .map(|ty| quote!(<#ty as ::wasmgc::GcValue>::Abi))
        .collect();

    let mut method_names = std::collections::HashSet::new();
    for field in &field_names {
        if !method_names.insert(field.clone()) || !method_names.insert(format!("set_{field}")) {
            return Err(syn::Error::new_spanned(
                name,
                "field names conflict with generated set_<field> methods",
            ));
        }
    }

    let mut declarations = Vec::new();
    let mut methods = Vec::new();
    let mut exports = Vec::new();
    if constructor {
        exports.push(struct_export_metadata(
            name,
            format_ident!("StructNew"),
            0,
            "new",
        ));
    }
    for (i, field) in fields.iter().enumerate() {
        let ident = names[i];
        let field_name = &field_names[i];
        let ty = types[i];
        let abi = &abi_types[i];
        let getter = &getters[i];
        let setter = &setters[i];
        let setter_method = format_ident!("set_{field_name}");
        let vis = &field.vis;
        let field_index = i as u32;
        let (export_get, export_set) = field_exports[i];
        for (enabled, access, kind) in [
            (export_get, "get", format_ident!("StructGet")),
            (export_set, "set", format_ident!("StructSet")),
        ] {
            if !enabled {
                continue;
            }
            exports.push(struct_export_metadata(
                name,
                kind,
                field_index,
                &format!("{access}_{field_name}"),
            ));
        }
        declarations.push(quote! {
            #[inline(never)]
            unsafe extern "C" fn #getter(handle: u32) -> #abi {
                static OPERATION: ::wasmgc::__private::Operation =
                    ::wasmgc::__private::Operation::new(
                        <#name as ::wasmgc::GcStruct>::SCHEMA,
                        ::wasmgc::__private::OperationKind::StructGet, #field_index,
                        #getter as *const (),
                    );
                let arguments = (handle,);
                // SAFETY: The static descriptor uses this field's index and
                // ABI. The initialized argument tuple outlives the marker call.
                unsafe {
                    <#abi as ::wasmgc::__private::Abi>::marker(
                        (&raw const OPERATION).cast(), (&raw const arguments).cast(),
                    )
                }
            }

            #[inline(never)]
            unsafe extern "C" fn #setter(handle: u32, value: #abi) {
                static OPERATION: ::wasmgc::__private::Operation =
                    ::wasmgc::__private::Operation::new(
                        <#name as ::wasmgc::GcStruct>::SCHEMA,
                        ::wasmgc::__private::OperationKind::StructSet, #field_index,
                        #setter as *const (),
                    );
                let arguments = (handle, value);
                // SAFETY: The descriptor matches this setter's field and ABI;
                // the tuple keeps both initialized arguments live for the call.
                unsafe {
                    ::wasmgc::__private::marker_void(
                        (&raw const OPERATION).cast(), (&raw const arguments).cast(),
                    )
                }
            }
        });
        methods.push(quote! {
            #[doc = concat!("Returns the current `", stringify!(#ident), "` field.")]
            #[doc = ""]
            #[doc = "Scalar values are copied. Reference fields return a new Rust root to the same reference value."]
            #vis fn #ident(&self) -> #ty {
                // SAFETY: self keeps the struct rooted. The generated getter
                // returns this field's scalar ABI or a fresh owned root, as
                // required by its GcValue implementation.
                unsafe {
                    <#ty as ::wasmgc::GcValue>::from_abi(#getter(self.root.as_raw()))
                }
            }

            #[doc = concat!("Replaces the `", stringify!(#ident), "` field with `value`.")]
            #[doc = ""]
            #[doc = "A reference value is borrowed for the call; the GC object retains the stored reference independently of its Rust root."]
            #vis fn #setter_method(&self, value: <#ty as ::wasmgc::GcValue>::SetterValue<'_>) {
                let abi = <#ty as ::wasmgc::GcValue>::setter_abi(&value);
                // SAFETY: self keeps the struct rooted, and value keeps the
                // resources borrowed by setter_abi alive until the field write.
                unsafe { #setter(self.root.as_raw(), abi) }
            }
        });
    }

    Ok(quote! {
        #[doc = concat!("A borrowed field-access view of `Gc<", stringify!(#name), ">`.")]
        #[doc = ""]
        #[doc = "Access this view through the GC root's Deref implementation. Its methods read and write the existing GC object; they do not expose a Rust reference to the object's storage."]
        #[repr(transparent)]
        #visibility struct #view {
            root: ::wasmgc::Gc<#name>,
        }

        impl ::wasmgc::__private::MustNotImplDrop for #name {}

        const _: () = {
            static FIELDS: [::wasmgc::__private::ValueKind; #field_count] = [
                #(<#types as ::wasmgc::GcValue>::KIND),*
            ];

            static SCHEMA: ::wasmgc::__private::Schema =
                ::wasmgc::__private::Schema::new(&FIELDS);

            #(#exports)*

            #[inline(never)]
            unsafe extern "C" fn __new(#(#arguments: #abi_types),*) -> u32 {
                static OPERATION: ::wasmgc::__private::Operation =
                    ::wasmgc::__private::Operation::new(
                        <#name as ::wasmgc::GcStruct>::SCHEMA,
                        ::wasmgc::__private::OperationKind::StructNew, 0, __new as *const (),
                    );
                let arguments = (#(#arguments,)*);
                // SAFETY: The static descriptor matches the constructor's
                // field order and ABIs; its initialized tuple lives for the call.
                unsafe {
                    <u32 as ::wasmgc::__private::Abi>::marker(
                        (&raw const OPERATION).cast(), (&raw const arguments).cast(),
                    )
                }
            }

            #(#declarations)*

            // SAFETY: The schema and shims use each field's GcValue kind and
            // ABI. allocate retains field owners and returns a fresh root;
            // the transparent view preserves the lifetime of the borrowed root.
            unsafe impl ::wasmgc::GcStruct for #name {
                const SCHEMA: *const ::wasmgc::__private::Schema = &raw const SCHEMA;

                type View = #view;

                fn allocate(self) -> u32 {
                    let Self { #(#names: #arguments),* } = self;
                    // SAFETY: The destructured fields own all resources used
                    // by to_abi until __new has stored the corresponding values.
                    unsafe { __new(#(<#types as ::wasmgc::GcValue>::to_abi(&#arguments)),*) }
                }

                fn view(root: &::wasmgc::Gc<Self>) -> &Self::View {
                    // SAFETY: repr(transparent) gives the view the layout of its
                    // sole Gc<Self> field. The cast preserves the shared borrow.
                    unsafe { &*(root as *const ::wasmgc::Gc<Self>).cast::<Self::View>() }
                }
            }

            impl #view {
                #(#methods)*
            }
        };
    })
}

fn field_exports(field: &syn::Field) -> syn::Result<(bool, bool)> {
    let mut get = false;
    let mut set = false;
    for attribute in &field.attrs {
        if attribute.path().is_ident("wasmgc") {
            return Err(syn::Error::new_spanned(
                attribute,
                "use getter or setter on fields",
            ));
        }
        let getter = attribute.path().is_ident("getter");
        let setter = attribute.path().is_ident("setter");
        if !getter && !setter {
            continue;
        }
        if !matches!(attribute.meta, Meta::Path(_)) {
            return Err(syn::Error::new_spanned(
                attribute,
                "getter and setter do not take arguments",
            ));
        }
        get |= getter;
        set |= setter;
    }
    Ok((get, set))
}

fn struct_export_metadata(
    name: &syn::Ident,
    kind: syn::Ident,
    field: u32,
    method: &str,
) -> proc_macro2::TokenStream {
    let descriptor = format_ident!(
        "__STRUCT_EXPORT_{}_{field}",
        kind.to_string().to_uppercase()
    );
    let suffix = format!(".{method}");
    let export_name = format!("{}{suffix}", name.unraw());
    quote! {
        #[used]
        #[unsafe(export_name = concat!(
            "__wasmgc_struct_export_desc.", module_path!(), ".", stringify!(#name), #suffix
        ))]
        static #descriptor: ::wasmgc::__private::Struct =
            ::wasmgc::__private::Struct::new(
                <#name as ::wasmgc::GcStruct>::SCHEMA,
                ::wasmgc::__private::OperationKind::#kind,
                #field,
                #export_name,
            );
    }
}
