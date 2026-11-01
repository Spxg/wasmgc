use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::{
    Attribute, FnArg, ForeignItem, ForeignItemFn, Item, ItemFn, ItemForeignMod, ReturnType, Safety,
    Signature, Type, ext::IdentExt,
};

pub fn expand(arguments: TokenStream, input: Item) -> syn::Result<TokenStream> {
    if !arguments.is_empty() {
        return Err(syn::Error::new_spanned(
            arguments,
            "wasmgc does not take arguments on functions or extern blocks",
        ));
    }
    match input {
        Item::Fn(function) => export(function),
        Item::ForeignMod(block) => import(block),
        item => Err(syn::Error::new_spanned(
            item,
            "wasmgc requires a struct, function or extern block",
        )),
    }
}

struct Parameter<'a> {
    ty: &'a Type,
    value_type: &'a Type,
    borrowed: bool,
    ident: syn::Ident,
}

struct Function<'a> {
    name: &'a syn::Ident,
    parameters: Vec<Parameter<'a>>,
    result: Option<&'a Type>,
}

impl<'a> Function<'a> {
    fn parse(signature: &'a Signature) -> syn::Result<Self> {
        if !signature.generics.params.is_empty() || signature.generics.where_clause.is_some() {
            return Err(syn::Error::new_spanned(
                &signature.generics,
                "wasmgc requires a concrete, non-generic function",
            ));
        }
        if let Some(token) = &signature.constness {
            return Err(syn::Error::new_spanned(
                token,
                "wasmgc does not support const functions",
            ));
        }
        if let Some(token) = &signature.asyncness {
            return Err(syn::Error::new_spanned(
                token,
                "wasmgc does not support async functions",
            ));
        }
        if let Some(variadic) = &signature.variadic {
            return Err(syn::Error::new_spanned(
                variadic,
                "wasmgc does not support variadic functions",
            ));
        }

        let mut parameters = Vec::new();
        for (index, argument) in signature.inputs.iter().enumerate() {
            let FnArg::Typed(argument) = argument else {
                return Err(syn::Error::new_spanned(
                    argument,
                    "wasmgc supports free functions only",
                ));
            };
            if !argument.attrs.is_empty() {
                return Err(syn::Error::new_spanned(
                    argument,
                    "wasmgc parameters cannot have attributes",
                ));
            }
            let ty = &*argument.ty;
            let (value_type, borrowed) = match ungroup(ty) {
                Type::Reference(reference) => {
                    if let Some(mutability) = &reference.mutability {
                        return Err(syn::Error::new_spanned(
                            mutability,
                            "wasmgc parameters support shared borrows, not &mut",
                        ));
                    }
                    (&*reference.elem, true)
                }
                _ => (ty, false),
            };
            parameters.push(Parameter {
                ty,
                value_type,
                borrowed,
                ident: format_ident!(
                    "__wasmgc_{}_arg_{index}",
                    signature.ident.unraw(),
                    span = Span::mixed_site()
                ),
            });
        }

        let result = match &signature.output {
            ReturnType::Default => None,
            ReturnType::Type(_, ty) => match ungroup(ty) {
                Type::Tuple(tuple) if tuple.elems.is_empty() => None,
                Type::Reference(_) => {
                    return Err(syn::Error::new_spanned(
                        ty,
                        "wasmgc requires owned return values; return a cloned root instead of a borrow",
                    ));
                }
                Type::Never(_) => {
                    return Err(syn::Error::new_spanned(
                        ty,
                        "wasmgc does not support diverging return types",
                    ));
                }
                _ => Some(&**ty),
            },
        };
        Ok(Self {
            name: &signature.ident,
            parameters,
            result,
        })
    }

    fn identifier(&self, purpose: &str) -> syn::Ident {
        format_ident!(
            "__wasmgc_{}_{}",
            self.name.unraw(),
            purpose,
            span = Span::mixed_site()
        )
    }

    fn metadata(
        &self,
        kind: &str,
        module: &str,
        name: &str,
        attributes: TokenStream,
    ) -> TokenStream {
        let kind = syn::Ident::new(kind, Span::call_site());
        let descriptor = self.identifier("descriptor");
        let parameters = self.identifier("parameters");
        let result_name = self.identifier("result");
        let value_types = self.parameters.iter().map(|parameter| parameter.value_type);
        let count = self.parameters.len();
        let (result_static, result) = match self.result {
            Some(ty) => (
                quote! {
                    static #result_name: ::wasmgc::__private::ValueKind =
                        <#ty as ::wasmgc::GcValue>::KIND;
                },
                quote!(Some(&#result_name)),
            ),
            None => (TokenStream::new(), quote!(None)),
        };
        quote! {
            static #parameters: [::wasmgc::__private::ValueKind; #count] = [
                #(<#value_types as ::wasmgc::GcValue>::KIND),*
            ];

            #result_static

            #attributes
            static #descriptor: ::wasmgc::__private::Function =
                ::wasmgc::__private::Function::new(
                    ::wasmgc::__private::FunctionKind::#kind,
                    &#parameters,
                    #result,
                    #module,
                    #name,
                );
        }
    }

    fn abi_parameters(&self) -> Vec<TokenStream> {
        self.parameters
            .iter()
            .map(|parameter| {
                let Parameter {
                    ident, value_type, ..
                } = parameter;
                quote!(#ident: <#value_type as ::wasmgc::GcValue>::Abi)
            })
            .collect()
    }

    fn abi_result(&self) -> TokenStream {
        match self.result {
            Some(ty) => quote!(-> <#ty as ::wasmgc::GcValue>::Abi),
            None => TokenStream::new(),
        }
    }
}

fn ungroup(mut ty: &Type) -> &Type {
    loop {
        ty = match ty {
            Type::Paren(paren) => &paren.elem,
            Type::Group(group) => &group.elem,
            _ => return ty,
        };
    }
}

fn export(item: ItemFn) -> syn::Result<TokenStream> {
    item.modifiers.require_empty()?;
    let export_name = item.sig.ident.unraw().to_string();
    let function = Function::parse(&item.sig)?;
    let name = &item.sig.ident;
    let metadata = function.metadata(
        "Export",
        "",
        &export_name,
        quote! {
            #[used]
            #[unsafe(export_name = concat!(
                "__wasmgc_function_desc.", stringify!(#name)
            ))]
        },
    );
    let abi_parameters = function.abi_parameters();
    let abi_result = function.abi_result();
    let bridge = function.identifier("bridge");
    let locals = function.parameters.iter().map(|parameter| {
        let Parameter {
            ident, value_type, ..
        } = parameter;
        quote! {
            // SAFETY: The generated export adapter supplies the declared scalar
            // ABI or transfers a fresh root for this reference parameter.
            let #ident = unsafe { <#value_type as ::wasmgc::GcValue>::from_abi(#ident) };
        }
    });
    let arguments = function.parameters.iter().map(|parameter| {
        let ident = &parameter.ident;
        if parameter.borrowed {
            quote!(&#ident)
        } else {
            quote!(#ident)
        }
    });
    let call = quote!(#name(#(#arguments),*));
    let call = if matches!(item.sig.safety, Safety::Unsafe(_)) {
        quote! {
            // SAFETY: Exporting an unsafe function preserves its preconditions
            // for host callers; the adapter only changes value representation.
            unsafe { #call }
        }
    } else {
        call
    };
    let call = match function.result {
        Some(ty) => quote!(<#ty as ::wasmgc::GcValue>::into_abi(#call)),
        None => call,
    };

    // Rust calls must retain the original signature. Only the extra bridge is
    // exported for replacement with a native reference adapter.
    Ok(quote! {
        #item

        const _: () = {
            #metadata

            #[inline(never)]
            #[unsafe(export_name = concat!(
                "__wasmgc_function_bridge.", stringify!(#name)
            ))]
            unsafe extern "C" fn #bridge(#(#abi_parameters),*) #abi_result {
                #(#locals)*
                #call
            }
        };
    })
}

fn import(block: ItemForeignMod) -> syn::Result<TokenStream> {
    if block
        .abi
        .name
        .as_ref()
        .is_none_or(|name| name.value() != "wasmgc")
    {
        return Err(syn::Error::new_spanned(
            &block.abi,
            "wasmgc imports require an extern \"wasmgc\" block",
        ));
    }
    let mut functions = Vec::new();
    for item in block.items {
        let item = match item {
            ForeignItem::Fn(item) => item,
            other => {
                return Err(syn::Error::new_spanned(
                    other,
                    "wasmgc extern blocks support functions only",
                ));
            }
        };
        functions.push(import_function(item, &block.attrs)?);
    }
    Ok(quote!(#(#functions)*))
}

fn import_function(
    item: ForeignItemFn,
    block_attributes: &[Attribute],
) -> syn::Result<TokenStream> {
    item.modifiers.require_empty()?;
    let import_name = item.sig.ident.unraw().to_string();
    let function = Function::parse(&item.sig)?;
    let name = &item.sig.ident;
    let visibility = &item.vis;
    let attributes = &item.attrs;
    let unsafety = if matches!(item.sig.safety, Safety::Safe(_)) {
        None
    } else {
        Some(quote!(unsafe))
    };
    let parameters = function.parameters.iter().map(|parameter| {
        let Parameter { ident, ty, .. } = parameter;
        quote!(#ident: #ty)
    });
    let output = &item.sig.output;
    let metadata = function.metadata("Import", "env", &import_name, TokenStream::new());
    let abi_parameters = function.abi_parameters();
    let abi_result = function.abi_result();
    let bridge = function.identifier("bridge");
    let descriptor = function.identifier("descriptor");
    let arguments_tuple = function.identifier("arguments");
    let result = function.identifier("return_value");
    let marker = match function.result {
        Some(ty) => quote!(<<#ty as ::wasmgc::GcValue>::Abi as ::wasmgc::__private::Abi>::marker),
        None => quote!(::wasmgc::__private::marker_void),
    };
    let abi_arguments = function.parameters.iter().map(|parameter| &parameter.ident);
    let arguments = function.parameters.iter().map(|parameter| {
        let Parameter {
            ident,
            value_type,
            borrowed,
            ..
        } = parameter;
        if *borrowed {
            quote!(<#value_type as ::wasmgc::GcValue>::to_abi(#ident))
        } else {
            quote!(<#value_type as ::wasmgc::GcValue>::to_abi(&#ident))
        }
    });
    let call = quote! {
        // SAFETY: Parameter owners keep all borrowed ABI resources live through
        // the import. The wrapper preserves the declared host-call preconditions.
        unsafe { #bridge(#(#arguments),*) }
    };
    let call = match function.result {
        Some(ty) => quote! {
            let #result = #call;
            // SAFETY: The import adapter lowers the result using its declared
            // scalar ABI or a newly allocated root owned by this return value.
            unsafe { <#ty as ::wasmgc::GcValue>::from_abi(#result) }
        },
        None => call,
    };

    Ok(quote! {
        #(#block_attributes)*
        #(#attributes)*
        #visibility #unsafety fn #name(#(#parameters),*) #output {
            #metadata

            #[inline(never)]
            unsafe extern "C" fn #bridge(#(#abi_parameters),*) #abi_result {
                let #arguments_tuple = (#(#abi_arguments,)* #bridge as *const ());
                // SAFETY: The static descriptor matches this import's signature;
                // the initialized argument tuple remains live for the marker.
                unsafe {
                    #marker(
                        (&raw const #descriptor).cast(),
                        (&raw const #arguments_tuple).cast(),
                    )
                }
            }

            #call
        }
    })
}
