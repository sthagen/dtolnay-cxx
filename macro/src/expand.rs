use crate::syntax::atom::Atom::*;
use crate::syntax::attrs::{self, OtherAttrs};
use crate::syntax::cfg::CfgExpr;
use crate::syntax::file::Module;
use crate::syntax::instantiate::{ImplKey, NamedImplKey};
use crate::syntax::qualified::QualifiedName;
use crate::syntax::report::Errors;
use crate::syntax::symbol::Symbol;
use crate::syntax::{
    self, check, mangle, Api, Doc, Enum, ExternFn, ExternType, Impl, Lang, Lifetimes, Pair,
    Signature, Struct, Trait, Type, TypeAlias, Types,
};
use crate::type_id::Crate;
use crate::{derive, generics};
use proc_macro2::{Ident, Span, TokenStream};
use quote::{format_ident, quote, quote_spanned, ToTokens};
use std::mem;
use syn::{parse_quote, punctuated, Generics, Lifetime, Result, Token};

pub(crate) fn bridge(mut ffi: Module) -> Result<TokenStream> {
    let ref mut errors = Errors::new();

    let mut cfg = CfgExpr::Unconditional;
    let mut doc = Doc::new();
    let attrs = attrs::parse(
        errors,
        mem::take(&mut ffi.attrs),
        attrs::Parser {
            cfg: Some(&mut cfg),
            doc: Some(&mut doc),
            ..Default::default()
        },
    );

    let content = mem::take(&mut ffi.content);
    let trusted = ffi.unsafety.is_some();
    let namespace = &ffi.namespace;
    let ref mut apis = syntax::parse_items(errors, content, trusted, namespace);
    #[cfg(feature = "experimental-enum-variants-from-header")]
    crate::load::load(errors, apis);
    let ref types = Types::collect(errors, apis);
    errors.propagate()?;

    let generator = check::Generator::Macro;
    check::typecheck(errors, apis, types, generator);
    errors.propagate()?;

    Ok(expand(ffi, doc, attrs, apis, types))
}

fn expand(ffi: Module, doc: Doc, attrs: OtherAttrs, apis: &[Api], types: &Types) -> TokenStream {
    let mut expanded = TokenStream::new();
    let mut hidden = TokenStream::new();
    let mut forbid = TokenStream::new();

    for api in apis {
        if let Api::RustType(ety) = api {
            expanded.extend(expand_rust_type_import(ety));
            hidden.extend(expand_rust_type_assert_unpin(ety, types));
        }
    }

    for api in apis {
        match api {
            Api::Include(_) | Api::Impl(_) => {}
            Api::Struct(strct) => {
                expanded.extend(expand_struct(strct));
                hidden.extend(expand_struct_operators(strct));
                forbid.extend(expand_struct_forbid_drop(strct));
            }
            Api::Enum(enm) => expanded.extend(expand_enum(enm)),
            Api::CxxType(ety) => {
                let ident = &ety.name.rust;
                if !types.structs.contains_key(ident) && !types.enums.contains_key(ident) {
                    expanded.extend(expand_cxx_type(ety));
                    hidden.extend(expand_cxx_type_assert_pinned(ety, types));
                }
            }
            Api::CxxFunction(efn) => {
                expanded.extend(expand_cxx_function_shim(efn, types));
            }
            Api::RustType(ety) => {
                expanded.extend(expand_rust_type_impl(ety));
                hidden.extend(expand_rust_type_layout(ety, types));
            }
            Api::RustFunction(efn) => hidden.extend(expand_rust_function_shim(efn, types)),
            Api::TypeAlias(alias) => {
                expanded.extend(expand_type_alias(alias));
                hidden.extend(expand_type_alias_verify(alias, types));
            }
        }
    }

    for (impl_key, &explicit_impl) in &types.impls {
        match impl_key {
            ImplKey::RustBox(ident) => {
                hidden.extend(expand_rust_box(ident, types, explicit_impl));
            }
            ImplKey::RustVec(ident) => {
                hidden.extend(expand_rust_vec(ident, types, explicit_impl));
            }
            ImplKey::UniquePtr(ident) => {
                expanded.extend(expand_unique_ptr(ident, types, explicit_impl));
            }
            ImplKey::SharedPtr(ident) => {
                expanded.extend(expand_shared_ptr(ident, types, explicit_impl));
            }
            ImplKey::WeakPtr(ident) => {
                expanded.extend(expand_weak_ptr(ident, types, explicit_impl));
            }
            ImplKey::CxxVector(ident) => {
                expanded.extend(expand_cxx_vector(ident, explicit_impl, types));
            }
        }
    }

    if !forbid.is_empty() {
        hidden.extend(expand_forbid(forbid));
    }

    // Work around https://github.com/rust-lang/rust/issues/67851.
    if !hidden.is_empty() {
        expanded.extend(quote! {
            #[doc(hidden)]
            const _: () = {
                #hidden
            };
        });
    }

    let vis = &ffi.vis;
    let mod_token = &ffi.mod_token;
    let ident = &ffi.ident;
    let span = ffi.brace_token.span;
    let expanded = quote_spanned!(span=> {#expanded});

    quote! {
        #doc
        #attrs
        #[deny(improper_ctypes, improper_ctypes_definitions)]
        #[allow(clippy::unknown_lints)]
        #[allow(
            non_camel_case_types,
            non_snake_case,
            clippy::extra_unused_type_parameters,
            clippy::items_after_statements,
            clippy::no_effect_underscore_binding,
            clippy::ptr_as_ptr,
            clippy::ref_as_ptr,
            clippy::upper_case_acronyms,
            clippy::use_self,
        )]
        #vis #mod_token #ident #expanded
    }
}

fn expand_struct(strct: &Struct) -> TokenStream {
    let ident = &strct.name.rust;
    let doc = &strct.doc;
    let attrs = &strct.attrs;
    let generics = &strct.generics;
    let type_id = type_id(&strct.name);
    let fields = strct.fields.iter().map(|field| {
        let doc = &field.doc;
        let attrs = &field.attrs;
        // This span on the pub makes "private type in public interface" errors
        // appear in the right place.
        let vis = field.visibility;
        quote!(#doc #attrs #vis #field)
    });
    let mut derives = None;
    let derived_traits = derive::expand_struct(strct, &mut derives);

    let span = ident.span();
    let visibility = strct.visibility;
    let struct_token = strct.struct_token;
    let struct_def = quote_spanned! {span=>
        #visibility #struct_token #ident #generics {
            #(#fields,)*
        }
    };

    quote! {
        #doc
        #derives
        #attrs
        #[repr(C)]
        #struct_def

        #[automatically_derived]
        unsafe impl #generics ::cxx::ExternType for #ident #generics {
            #[allow(unused_attributes)] // incorrect lint
            #[doc(hidden)]
            type Id = #type_id;
            type Kind = ::cxx::kind::Trivial;
        }

        #derived_traits
    }
}

fn expand_struct_operators(strct: &Struct) -> TokenStream {
    let ident = &strct.name.rust;
    let generics = &strct.generics;
    let mut operators = TokenStream::new();

    for derive in &strct.derives {
        let span = derive.span;
        match derive.what {
            Trait::PartialEq => {
                let link_name = mangle::operator(&strct.name, "eq");
                let local_name = format_ident!("__operator_eq_{}", strct.name.rust);
                let prevent_unwind_label = format!("::{} as PartialEq>::eq", strct.name.rust);
                operators.extend(quote_spanned! {span=>
                    #[doc(hidden)]
                    #[#UnsafeAttr(#ExportNameAttr = #link_name)]
                    extern "C" fn #local_name #generics(lhs: &#ident #generics, rhs: &#ident #generics) -> bool {
                        let __fn = concat!("<", module_path!(), #prevent_unwind_label);
                        ::cxx::private::prevent_unwind(__fn, || *lhs == *rhs)
                    }
                });

                if !derive::contains(&strct.derives, Trait::Eq) {
                    let link_name = mangle::operator(&strct.name, "ne");
                    let local_name = format_ident!("__operator_ne_{}", strct.name.rust);
                    let prevent_unwind_label = format!("::{} as PartialEq>::ne", strct.name.rust);
                    operators.extend(quote_spanned! {span=>
                        #[doc(hidden)]
                        #[#UnsafeAttr(#ExportNameAttr = #link_name)]
                        extern "C" fn #local_name #generics(lhs: &#ident #generics, rhs: &#ident #generics) -> bool {
                            let __fn = concat!("<", module_path!(), #prevent_unwind_label);
                            ::cxx::private::prevent_unwind(__fn, || *lhs != *rhs)
                        }
                    });
                }
            }
            Trait::PartialOrd => {
                let link_name = mangle::operator(&strct.name, "lt");
                let local_name = format_ident!("__operator_lt_{}", strct.name.rust);
                let prevent_unwind_label = format!("::{} as PartialOrd>::lt", strct.name.rust);
                operators.extend(quote_spanned! {span=>
                    #[doc(hidden)]
                    #[#UnsafeAttr(#ExportNameAttr = #link_name)]
                    extern "C" fn #local_name #generics(lhs: &#ident #generics, rhs: &#ident #generics) -> bool {
                        let __fn = concat!("<", module_path!(), #prevent_unwind_label);
                        ::cxx::private::prevent_unwind(__fn, || *lhs < *rhs)
                    }
                });

                let link_name = mangle::operator(&strct.name, "le");
                let local_name = format_ident!("__operator_le_{}", strct.name.rust);
                let prevent_unwind_label = format!("::{} as PartialOrd>::le", strct.name.rust);
                operators.extend(quote_spanned! {span=>
                    #[doc(hidden)]
                    #[#UnsafeAttr(#ExportNameAttr = #link_name)]
                    extern "C" fn #local_name #generics(lhs: &#ident #generics, rhs: &#ident #generics) -> bool {
                        let __fn = concat!("<", module_path!(), #prevent_unwind_label);
                        ::cxx::private::prevent_unwind(__fn, || *lhs <= *rhs)
                    }
                });

                if !derive::contains(&strct.derives, Trait::Ord) {
                    let link_name = mangle::operator(&strct.name, "gt");
                    let local_name = format_ident!("__operator_gt_{}", strct.name.rust);
                    let prevent_unwind_label = format!("::{} as PartialOrd>::gt", strct.name.rust);
                    operators.extend(quote_spanned! {span=>
                        #[doc(hidden)]
                        #[#UnsafeAttr(#ExportNameAttr = #link_name)]
                        extern "C" fn #local_name #generics(lhs: &#ident #generics, rhs: &#ident #generics) -> bool {
                            let __fn = concat!("<", module_path!(), #prevent_unwind_label);
                            ::cxx::private::prevent_unwind(__fn, || *lhs > *rhs)
                        }
                    });

                    let link_name = mangle::operator(&strct.name, "ge");
                    let local_name = format_ident!("__operator_ge_{}", strct.name.rust);
                    let prevent_unwind_label = format!("::{} as PartialOrd>::ge", strct.name.rust);
                    operators.extend(quote_spanned! {span=>
                        #[doc(hidden)]
                        #[#UnsafeAttr(#ExportNameAttr = #link_name)]
                        extern "C" fn #local_name #generics(lhs: &#ident #generics, rhs: &#ident #generics) -> bool {
                            let __fn = concat!("<", module_path!(), #prevent_unwind_label);
                            ::cxx::private::prevent_unwind(__fn, || *lhs >= *rhs)
                        }
                    });
                }
            }
            Trait::Hash => {
                let link_name = mangle::operator(&strct.name, "hash");
                let local_name = format_ident!("__operator_hash_{}", strct.name.rust);
                let prevent_unwind_label = format!("::{} as Hash>::hash", strct.name.rust);
                operators.extend(quote_spanned! {span=>
                    #[doc(hidden)]
                    #[#UnsafeAttr(#ExportNameAttr = #link_name)]
                    #[allow(clippy::cast_possible_truncation)]
                    extern "C" fn #local_name #generics(this: &#ident #generics) -> usize {
                        let __fn = concat!("<", module_path!(), #prevent_unwind_label);
                        ::cxx::private::prevent_unwind(__fn, || ::cxx::private::hash(this))
                    }
                });
            }
            _ => {}
        }
    }

    operators
}

fn expand_struct_forbid_drop(strct: &Struct) -> TokenStream {
    let ident = &strct.name.rust;
    let generics = &strct.generics;
    let span = ident.span();
    let impl_token = Token![impl](strct.visibility.span);

    quote_spanned! {span=>
        #[automatically_derived]
        #impl_token #generics self::Drop for super::#ident #generics {}
    }
}

fn expand_enum(enm: &Enum) -> TokenStream {
    let ident = &enm.name.rust;
    let doc = &enm.doc;
    let attrs = &enm.attrs;
    let repr = &enm.repr;
    let type_id = type_id(&enm.name);
    let variants = enm.variants.iter().map(|variant| {
        let doc = &variant.doc;
        let attrs = &variant.attrs;
        let variant_ident = &variant.name.rust;
        let discriminant = &variant.discriminant;
        let span = variant_ident.span();
        Some(quote_spanned! {span=>
            #doc
            #attrs
            #[allow(dead_code)]
            pub const #variant_ident: Self = #ident { repr: #discriminant };
        })
    });
    let mut derives = None;
    let derived_traits = derive::expand_enum(enm, &mut derives);

    let span = ident.span();
    let visibility = enm.visibility;
    let struct_token = Token![struct](enm.enum_token.span);
    let enum_repr = quote! {
        #[allow(missing_docs)]
        pub repr: #repr,
    };
    let enum_def = quote_spanned! {span=>
        #visibility #struct_token #ident {
            #enum_repr
        }
    };

    quote! {
        #doc
        #derives
        #attrs
        #[repr(transparent)]
        #enum_def

        #[allow(non_upper_case_globals)]
        impl #ident {
            #(#variants)*
        }

        #[automatically_derived]
        unsafe impl ::cxx::ExternType for #ident {
            #[allow(unused_attributes)] // incorrect lint
            #[doc(hidden)]
            type Id = #type_id;
            type Kind = ::cxx::kind::Trivial;
        }

        #derived_traits
    }
}

fn expand_cxx_type(ety: &ExternType) -> TokenStream {
    let ident = &ety.name.rust;
    let doc = &ety.doc;
    let attrs = &ety.attrs;
    let generics = &ety.generics;
    let type_id = type_id(&ety.name);

    let lifetime_fields = ety.generics.lifetimes.iter().map(|lifetime| {
        let field = format_ident!("_lifetime_{}", lifetime.ident);
        quote!(#field: ::cxx::core::marker::PhantomData<&#lifetime ()>)
    });
    let repr_fields = quote! {
        _private: ::cxx::private::Opaque,
        #(#lifetime_fields,)*
    };

    let span = ident.span();
    let visibility = &ety.visibility;
    let struct_token = Token![struct](ety.type_token.span);
    let extern_type_def = quote_spanned! {span=>
        #visibility #struct_token #ident #generics {
            #repr_fields
        }
    };

    quote! {
        #doc
        #attrs
        #[repr(C)]
        #extern_type_def

        #[automatically_derived]
        unsafe impl #generics ::cxx::ExternType for #ident #generics {
            #[allow(unused_attributes)] // incorrect lint
            #[doc(hidden)]
            type Id = #type_id;
            type Kind = ::cxx::kind::Opaque;
        }
    }
}

fn expand_cxx_type_assert_pinned(ety: &ExternType, types: &Types) -> TokenStream {
    let ident = &ety.name.rust;
    let infer = Token![_](ident.span());

    let resolve = types.resolve(ident);
    let lifetimes = resolve.generics.to_underscore_lifetimes();

    quote! {
        let _: fn() = {
            // Derived from https://github.com/nvzqz/static-assertions-rs.
            trait __AmbiguousIfImpl<A> {
                fn infer() {}
            }

            #[automatically_derived]
            impl<T> __AmbiguousIfImpl<()> for T
            where
                T: ?::cxx::core::marker::Sized
            {}

            #[allow(dead_code)]
            struct __Invalid;

            #[automatically_derived]
            impl<T> __AmbiguousIfImpl<__Invalid> for T
            where
                T: ?::cxx::core::marker::Sized + ::cxx::core::marker::Unpin,
            {}

            // If there is only one specialized trait impl, type inference with
            // `_` can be resolved and this can compile. Fails to compile if
            // user has added a manual Unpin impl for their opaque C++ type as
            // then `__AmbiguousIfImpl<__Invalid>` also exists.
            <#ident #lifetimes as __AmbiguousIfImpl<#infer>>::infer
        };
    }
}

fn expand_cxx_function_decl(efn: &ExternFn, types: &Types) -> TokenStream {
    let generics = &efn.generics;
    let receiver = efn.receiver.iter().map(|receiver| {
        let receiver_type = receiver.ty();
        quote!(_: #receiver_type)
    });
    let args = efn.args.iter().map(|arg| {
        let var = &arg.name.rust;
        let colon = arg.colon_token;
        let ty = expand_extern_type(&arg.ty, types, true);
        if arg.ty == RustString {
            quote!(#var #colon *const #ty)
        } else if let Type::RustVec(_) = arg.ty {
            quote!(#var #colon *const #ty)
        } else if let Type::Fn(_) = arg.ty {
            quote!(#var #colon ::cxx::private::FatFunction)
        } else if types.needs_indirect_abi(&arg.ty) {
            quote!(#var #colon *mut #ty)
        } else {
            quote!(#var #colon #ty)
        }
    });
    let all_args = receiver.chain(args);
    let ret = if efn.throws {
        quote!(-> ::cxx::private::Result)
    } else {
        expand_extern_return_type(&efn.ret, types, true)
    };
    let mut outparam = None;
    if indirect_return(efn, types) {
        let ret = expand_extern_type(efn.ret.as_ref().unwrap(), types, true);
        outparam = Some(quote!(__return: *mut #ret));
    }
    let link_name = mangle::extern_fn(efn, types);
    let local_name = format_ident!("__{}", efn.name.rust);
    quote! {
        #[link_name = #link_name]
        fn #local_name #generics(#(#all_args,)* #outparam) #ret;
    }
}

fn expand_cxx_function_shim(efn: &ExternFn, types: &Types) -> TokenStream {
    let doc = &efn.doc;
    let attrs = &efn.attrs;
    let decl = expand_cxx_function_decl(efn, types);
    let receiver = efn.receiver.iter().map(|receiver| {
        let var = receiver.var;
        if receiver.pinned {
            let colon = receiver.colon_token;
            let ty = receiver.ty_self();
            quote!(#var #colon #ty)
        } else {
            let ampersand = receiver.ampersand;
            let lifetime = &receiver.lifetime;
            let mutability = receiver.mutability;
            quote!(#ampersand #lifetime #mutability #var)
        }
    });
    let args = efn.args.iter().map(|arg| quote!(#arg));
    let all_args = receiver.chain(args);
    let ret = if efn.throws {
        let ok = match &efn.ret {
            Some(ret) => quote!(#ret),
            None => quote!(()),
        };
        quote!(-> ::cxx::core::result::Result<#ok, ::cxx::Exception>)
    } else {
        expand_return_type(&efn.ret)
    };
    let indirect_return = indirect_return(efn, types);
    let receiver_var = efn
        .receiver
        .iter()
        .map(|receiver| receiver.var.to_token_stream());
    let arg_vars = efn.args.iter().map(|arg| {
        let var = &arg.name.rust;
        let span = var.span();
        match &arg.ty {
            Type::Ident(ident) if ident.rust == RustString => {
                quote_spanned!(span=> #var.as_mut_ptr() as *const ::cxx::private::RustString)
            }
            Type::RustBox(ty) => {
                if types.is_considered_improper_ctype(&ty.inner) {
                    quote_spanned!(span=> ::cxx::alloc::boxed::Box::into_raw(#var).cast())
                } else {
                    quote_spanned!(span=> ::cxx::alloc::boxed::Box::into_raw(#var))
                }
            }
            Type::UniquePtr(ty) => {
                if types.is_considered_improper_ctype(&ty.inner) {
                    quote_spanned!(span=> ::cxx::UniquePtr::into_raw(#var).cast())
                } else {
                    quote_spanned!(span=> ::cxx::UniquePtr::into_raw(#var))
                }
            }
            Type::RustVec(_) => quote_spanned!(span=> #var.as_mut_ptr() as *const ::cxx::private::RustVec<_>),
            Type::Ref(ty) => match &ty.inner {
                Type::Ident(ident) if ident.rust == RustString => match ty.mutable {
                    false => quote_spanned!(span=> ::cxx::private::RustString::from_ref(#var)),
                    true => quote_spanned!(span=> ::cxx::private::RustString::from_mut(#var)),
                },
                Type::RustVec(vec) if vec.inner == RustString => match ty.mutable {
                    false => quote_spanned!(span=> ::cxx::private::RustVec::from_ref_vec_string(#var)),
                    true => quote_spanned!(span=> ::cxx::private::RustVec::from_mut_vec_string(#var)),
                },
                Type::RustVec(_) => match ty.mutable {
                    false => quote_spanned!(span=> ::cxx::private::RustVec::from_ref(#var)),
                    true => quote_spanned!(span=> ::cxx::private::RustVec::from_mut(#var)),
                },
                inner if types.is_considered_improper_ctype(inner) => {
                    let var = match ty.pinned {
                        false => quote!(#var),
                        true => quote_spanned!(span=> ::cxx::core::pin::Pin::into_inner_unchecked(#var)),
                    };
                    match ty.mutable {
                        false => {
                            quote_spanned!(span=> #var as *const #inner as *const ::cxx::core::ffi::c_void)
                        }
                        true => quote_spanned!(span=> #var as *mut #inner as *mut ::cxx::core::ffi::c_void),
                    }
                }
                _ => quote!(#var),
            },
            Type::Ptr(ty) => {
                if types.is_considered_improper_ctype(&ty.inner) {
                    quote_spanned!(span=> #var.cast())
                } else {
                    quote!(#var)
                }
            }
            Type::Str(_) => quote_spanned!(span=> ::cxx::private::RustStr::from(#var)),
            Type::SliceRef(ty) => match ty.mutable {
                false => quote_spanned!(span=> ::cxx::private::RustSlice::from_ref(#var)),
                true => quote_spanned!(span=> ::cxx::private::RustSlice::from_mut(#var)),
            },
            ty if types.needs_indirect_abi(ty) => quote_spanned!(span=> #var.as_mut_ptr()),
            _ => quote!(#var),
        }
    });
    let vars = receiver_var.chain(arg_vars);
    let trampolines = efn
        .args
        .iter()
        .filter_map(|arg| {
            if let Type::Fn(f) = &arg.ty {
                let var = &arg.name;
                Some(expand_function_pointer_trampoline(efn, var, f, types))
            } else {
                None
            }
        })
        .collect::<TokenStream>();
    let mut setup = efn
        .args
        .iter()
        .filter(|arg| types.needs_indirect_abi(&arg.ty))
        .map(|arg| {
            let var = &arg.name.rust;
            let span = var.span();
            // These are arguments for which C++ has taken ownership of the data
            // behind the mut reference it received.
            quote_spanned! {span=>
                let mut #var = ::cxx::core::mem::MaybeUninit::new(#var);
            }
        })
        .collect::<TokenStream>();
    let local_name = format_ident!("__{}", efn.name.rust);
    let span = efn.semi_token.span;
    let call = if indirect_return {
        let ret = expand_extern_type(efn.ret.as_ref().unwrap(), types, true);
        setup.extend(quote_spanned! {span=>
            let mut __return = ::cxx::core::mem::MaybeUninit::<#ret>::uninit();
        });
        setup.extend(if efn.throws {
            quote_spanned! {span=>
                #local_name(#(#vars,)* __return.as_mut_ptr()).exception()?;
            }
        } else {
            quote_spanned! {span=>
                #local_name(#(#vars,)* __return.as_mut_ptr());
            }
        });
        quote_spanned!(span=> __return.assume_init())
    } else if efn.throws {
        quote_spanned! {span=>
            #local_name(#(#vars),*).exception()
        }
    } else {
        quote_spanned! {span=>
            #local_name(#(#vars),*)
        }
    };
    let mut expr;
    if let Some(ret) = &efn.ret {
        expr = match ret {
            Type::Ident(ident) if ident.rust == RustString => {
                quote_spanned!(span=> #call.into_string())
            }
            Type::RustBox(ty) => {
                if types.is_considered_improper_ctype(&ty.inner) {
                    quote_spanned!(span=> ::cxx::alloc::boxed::Box::from_raw(#call.cast()))
                } else {
                    quote_spanned!(span=> ::cxx::alloc::boxed::Box::from_raw(#call))
                }
            }
            Type::RustVec(vec) => {
                if vec.inner == RustString {
                    quote_spanned!(span=> #call.into_vec_string())
                } else {
                    quote_spanned!(span=> #call.into_vec())
                }
            }
            Type::UniquePtr(ty) => {
                if types.is_considered_improper_ctype(&ty.inner) {
                    quote_spanned!(span=> ::cxx::UniquePtr::from_raw(#call.cast()))
                } else {
                    quote_spanned!(span=> ::cxx::UniquePtr::from_raw(#call))
                }
            }
            Type::Ref(ty) => match &ty.inner {
                Type::Ident(ident) if ident.rust == RustString => match ty.mutable {
                    false => quote_spanned!(span=> #call.as_string()),
                    true => quote_spanned!(span=> #call.as_mut_string()),
                },
                Type::RustVec(vec) if vec.inner == RustString => match ty.mutable {
                    false => quote_spanned!(span=> #call.as_vec_string()),
                    true => quote_spanned!(span=> #call.as_mut_vec_string()),
                },
                Type::RustVec(_) => match ty.mutable {
                    false => quote_spanned!(span=> #call.as_vec()),
                    true => quote_spanned!(span=> #call.as_mut_vec()),
                },
                inner if types.is_considered_improper_ctype(inner) => {
                    let mutability = ty.mutability;
                    let deref_mut = quote_spanned!(span=> &#mutability *#call.cast());
                    match ty.pinned {
                        false => deref_mut,
                        true => {
                            quote_spanned!(span=> ::cxx::core::pin::Pin::new_unchecked(#deref_mut))
                        }
                    }
                }
                _ => call,
            },
            Type::Ptr(ty) => {
                if types.is_considered_improper_ctype(&ty.inner) {
                    quote_spanned!(span=> #call.cast())
                } else {
                    call
                }
            }
            Type::Str(_) => quote_spanned!(span=> #call.as_str()),
            Type::SliceRef(slice) => {
                let inner = &slice.inner;
                match slice.mutable {
                    false => quote_spanned!(span=> #call.as_slice::<#inner>()),
                    true => quote_spanned!(span=> #call.as_mut_slice::<#inner>()),
                }
            }
            _ => call,
        };
        if efn.throws {
            expr = quote_spanned!(span=> ::cxx::core::result::Result::Ok(#expr));
        }
    } else if efn.throws {
        expr = call;
    } else {
        expr = quote! { #call; };
    }
    let dispatch = quote_spanned!(span=> unsafe { #setup #expr });
    let visibility = efn.visibility;
    let unsafety = &efn.unsafety;
    let fn_token = efn.fn_token;
    let ident = &efn.name.rust;
    let generics = &efn.generics;
    let arg_list = quote_spanned!(efn.paren_token.span=> (#(#all_args,)*));
    let calling_conv = match efn.lang {
        Lang::Cxx => quote_spanned!(span=> "C"),
        Lang::CxxUnwind => quote_spanned!(span=> "C-unwind"),
        Lang::Rust => unreachable!(),
    };
    let fn_body = quote_spanned!(span=> {
        #UnsafeExtern extern #calling_conv {
            #decl
        }
        #trampolines
        #dispatch
    });
    match &efn.receiver {
        None => {
            quote! {
                #doc
                #attrs
                #visibility #unsafety #fn_token #ident #generics #arg_list #ret #fn_body
            }
        }
        Some(receiver) => {
            let elided_generics;
            let receiver_ident = &receiver.ty.rust;
            let resolve = types.resolve(&receiver.ty);
            let receiver_generics = if receiver.ty.generics.lt_token.is_some() {
                &receiver.ty.generics
            } else {
                elided_generics = Lifetimes {
                    lt_token: resolve.generics.lt_token,
                    lifetimes: resolve
                        .generics
                        .lifetimes
                        .pairs()
                        .map(|pair| {
                            let lifetime = Lifetime::new("'_", pair.value().apostrophe);
                            let punct = pair.punct().map(|&&comma| comma);
                            punctuated::Pair::new(lifetime, punct)
                        })
                        .collect(),
                    gt_token: resolve.generics.gt_token,
                };
                &elided_generics
            };
            quote_spanned! {ident.span()=>
                impl #generics #receiver_ident #receiver_generics {
                    #doc
                    #attrs
                    #visibility #unsafety #fn_token #ident #arg_list #ret #fn_body
                }
            }
        }
    }
}

fn expand_function_pointer_trampoline(
    efn: &ExternFn,
    var: &Pair,
    sig: &Signature,
    types: &Types,
) -> TokenStream {
    let c_trampoline = mangle::c_trampoline(efn, var, types);
    let r_trampoline = mangle::r_trampoline(efn, var, types);
    let local_name = parse_quote!(__);
    let prevent_unwind_label = format!("::{}::{}", efn.name.rust, var.rust);
    let body_span = efn.semi_token.span;
    let shim = expand_rust_function_shim_impl(
        sig,
        types,
        &r_trampoline,
        local_name,
        prevent_unwind_label,
        None,
        Some(&efn.generics),
        &efn.attrs,
        body_span,
    );
    let calling_conv = match efn.lang {
        Lang::Cxx => "C",
        Lang::CxxUnwind => "C-unwind",
        Lang::Rust => unreachable!(),
    };
    let var = &var.rust;

    quote! {
        let #var = ::cxx::private::FatFunction {
            trampoline: {
                #UnsafeExtern extern #calling_conv {
                    #[link_name = #c_trampoline]
                    fn trampoline();
                }
                #shim
                trampoline as usize as *const ::cxx::core::ffi::c_void
            },
            ptr: #var as usize as *const ::cxx::core::ffi::c_void,
        };
    }
}

fn expand_rust_type_import(ety: &ExternType) -> TokenStream {
    let ident = &ety.name.rust;
    let span = ident.span();

    quote_spanned! {span=>
        use super::#ident;
    }
}

fn expand_rust_type_impl(ety: &ExternType) -> TokenStream {
    let ident = &ety.name.rust;
    let generics = &ety.generics;
    let span = ident.span();
    let unsafe_impl = quote_spanned!(ety.type_token.span=> unsafe impl);

    let mut impls = quote_spanned! {span=>
        #[automatically_derived]
        #[doc(hidden)]
        #unsafe_impl #generics ::cxx::private::RustType for #ident #generics {}
    };

    for derive in &ety.derives {
        if derive.what == Trait::ExternType {
            let type_id = type_id(&ety.name);
            let span = derive.span;
            impls.extend(quote_spanned! {span=>
                #[automatically_derived]
                unsafe impl #generics ::cxx::ExternType for #ident #generics {
                    #[allow(unused_attributes)] // incorrect lint
                    #[doc(hidden)]
                    type Id = #type_id;
                    type Kind = ::cxx::kind::Opaque;
                }
            });
        }
    }

    impls
}

fn expand_rust_type_assert_unpin(ety: &ExternType, types: &Types) -> TokenStream {
    let ident = &ety.name.rust;
    let begin_span = Token![::](ety.type_token.span);
    let unpin = quote_spanned! {ety.semi_token.span=>
        #begin_span cxx::core::marker::Unpin
    };

    let resolve = types.resolve(ident);
    let lifetimes = resolve.generics.to_underscore_lifetimes();

    quote_spanned! {ident.span()=>
        let _ = {
            fn __AssertUnpin<T: ?::cxx::core::marker::Sized + #unpin>() {}
            __AssertUnpin::<#ident #lifetimes>
        };
    }
}

fn expand_rust_type_layout(ety: &ExternType, types: &Types) -> TokenStream {
    // Rustc will render as follows if not sized:
    //
    //     type TheirType;
    //     -----^^^^^^^^^-
    //     |    |
    //     |    doesn't have a size known at compile-time
    //     required by this bound in `__AssertSized`

    let ident = &ety.name.rust;
    let begin_span = Token![::](ety.type_token.span);
    let sized = quote_spanned! {ety.semi_token.span=>
        #begin_span cxx::core::marker::Sized
    };

    let link_sizeof = mangle::operator(&ety.name, "sizeof");
    let link_alignof = mangle::operator(&ety.name, "alignof");

    let local_sizeof = format_ident!("__sizeof_{}", ety.name.rust);
    let local_alignof = format_ident!("__alignof_{}", ety.name.rust);

    let resolve = types.resolve(ident);
    let lifetimes = resolve.generics.to_underscore_lifetimes();

    quote_spanned! {ident.span()=>
        {
            #[doc(hidden)]
            #[allow(clippy::needless_maybe_sized)]
            fn __AssertSized<T: ?#sized + #sized>() -> ::cxx::core::alloc::Layout {
                ::cxx::core::alloc::Layout::new::<T>()
            }
            #[doc(hidden)]
            #[#UnsafeAttr(#ExportNameAttr = #link_sizeof)]
            extern "C" fn #local_sizeof() -> usize {
                __AssertSized::<#ident #lifetimes>().size()
            }
            #[doc(hidden)]
            #[#UnsafeAttr(#ExportNameAttr = #link_alignof)]
            extern "C" fn #local_alignof() -> usize {
                __AssertSized::<#ident #lifetimes>().align()
            }
        }
    }
}

fn expand_forbid(impls: TokenStream) -> TokenStream {
    quote! {
        mod forbid {
            pub trait Drop {}
            #[automatically_derived]
            #[allow(drop_bounds)]
            impl<T: ?::cxx::core::marker::Sized + ::cxx::core::ops::Drop> self::Drop for T {}
            #impls
        }
    }
}

fn expand_rust_function_shim(efn: &ExternFn, types: &Types) -> TokenStream {
    let link_name = mangle::extern_fn(efn, types);
    let local_name = match &efn.receiver {
        None => format_ident!("__{}", efn.name.rust),
        Some(receiver) => format_ident!("__{}__{}", receiver.ty.rust, efn.name.rust),
    };
    let prevent_unwind_label = match &efn.receiver {
        None => format!("::{}", efn.name.rust),
        Some(receiver) => format!("::{}::{}", receiver.ty.rust, efn.name.rust),
    };
    let invoke = Some(&efn.name.rust);
    let body_span = efn.semi_token.span;
    expand_rust_function_shim_impl(
        efn,
        types,
        &link_name,
        local_name,
        prevent_unwind_label,
        invoke,
        None,
        &efn.attrs,
        body_span,
    )
}

fn expand_rust_function_shim_impl(
    sig: &Signature,
    types: &Types,
    link_name: &Symbol,
    local_name: Ident,
    prevent_unwind_label: String,
    invoke: Option<&Ident>,
    outer_generics: Option<&Generics>,
    attrs: &OtherAttrs,
    body_span: Span,
) -> TokenStream {
    let generics = outer_generics.unwrap_or(&sig.generics);
    let receiver_var = sig
        .receiver
        .as_ref()
        .map(|receiver| quote_spanned!(receiver.var.span=> __self));
    let receiver = sig.receiver.as_ref().map(|receiver| {
        let colon = receiver.colon_token;
        let receiver_type = receiver.ty();
        quote!(#receiver_var #colon #receiver_type)
    });
    let args = sig.args.iter().map(|arg| {
        let var = &arg.name.rust;
        let colon = arg.colon_token;
        let ty = expand_extern_type(&arg.ty, types, false);
        if types.needs_indirect_abi(&arg.ty) {
            quote!(#var #colon *mut #ty)
        } else {
            quote!(#var #colon #ty)
        }
    });
    let all_args = receiver.into_iter().chain(args);

    let mut requires_unsafe = false;
    let arg_vars = sig.args.iter().map(|arg| {
        let var = &arg.name.rust;
        let span = var.span();
        match &arg.ty {
            Type::Ident(i) if i.rust == RustString => {
                requires_unsafe = true;
                quote_spanned!(span=> ::cxx::core::mem::take((*#var).as_mut_string()))
            }
            Type::RustBox(_) => {
                requires_unsafe = true;
                quote_spanned!(span=> ::cxx::alloc::boxed::Box::from_raw(#var))
            }
            Type::RustVec(vec) => {
                requires_unsafe = true;
                if vec.inner == RustString {
                    quote_spanned!(span=> ::cxx::core::mem::take((*#var).as_mut_vec_string()))
                } else {
                    quote_spanned!(span=> ::cxx::core::mem::take((*#var).as_mut_vec()))
                }
            }
            Type::UniquePtr(_) => {
                requires_unsafe = true;
                quote_spanned!(span=> ::cxx::UniquePtr::from_raw(#var))
            }
            Type::Ref(ty) => match &ty.inner {
                Type::Ident(i) if i.rust == RustString => match ty.mutable {
                    false => quote_spanned!(span=> #var.as_string()),
                    true => quote_spanned!(span=> #var.as_mut_string()),
                },
                Type::RustVec(vec) if vec.inner == RustString => match ty.mutable {
                    false => quote_spanned!(span=> #var.as_vec_string()),
                    true => quote_spanned!(span=> #var.as_mut_vec_string()),
                },
                Type::RustVec(_) => match ty.mutable {
                    false => quote_spanned!(span=> #var.as_vec()),
                    true => quote_spanned!(span=> #var.as_mut_vec()),
                },
                _ => quote!(#var),
            },
            Type::Str(_) => {
                requires_unsafe = true;
                quote_spanned!(span=> #var.as_str())
            }
            Type::SliceRef(slice) => {
                requires_unsafe = true;
                let inner = &slice.inner;
                match slice.mutable {
                    false => quote_spanned!(span=> #var.as_slice::<#inner>()),
                    true => quote_spanned!(span=> #var.as_mut_slice::<#inner>()),
                }
            }
            ty if types.needs_indirect_abi(ty) => {
                requires_unsafe = true;
                quote_spanned!(span=> ::cxx::core::ptr::read(#var))
            }
            _ => quote!(#var),
        }
    });
    let vars: Vec<_> = receiver_var.into_iter().chain(arg_vars).collect();

    let wrap_super = invoke.map(|invoke| expand_rust_function_shim_super(sig, &local_name, invoke));

    let mut requires_closure;
    let mut call = match invoke {
        Some(_) => {
            requires_closure = false;
            quote!(#local_name)
        }
        None => {
            requires_closure = true;
            requires_unsafe = true;
            quote!(::cxx::core::mem::transmute::<*const (), #sig>(__extern))
        }
    };
    requires_closure |= !vars.is_empty();
    call.extend(quote! { (#(#vars),*) });

    let span = body_span;
    let conversion = sig.ret.as_ref().and_then(|ret| match ret {
        Type::Ident(ident) if ident.rust == RustString => {
            Some(quote_spanned!(span=> ::cxx::private::RustString::from))
        }
        Type::RustBox(_) => Some(quote_spanned!(span=> ::cxx::alloc::boxed::Box::into_raw)),
        Type::RustVec(vec) => {
            if vec.inner == RustString {
                Some(quote_spanned!(span=> ::cxx::private::RustVec::from_vec_string))
            } else {
                Some(quote_spanned!(span=> ::cxx::private::RustVec::from))
            }
        }
        Type::UniquePtr(_) => Some(quote_spanned!(span=> ::cxx::UniquePtr::into_raw)),
        Type::Ref(ty) => match &ty.inner {
            Type::Ident(ident) if ident.rust == RustString => match ty.mutable {
                false => Some(quote_spanned!(span=> ::cxx::private::RustString::from_ref)),
                true => Some(quote_spanned!(span=> ::cxx::private::RustString::from_mut)),
            },
            Type::RustVec(vec) if vec.inner == RustString => match ty.mutable {
                false => Some(quote_spanned!(span=> ::cxx::private::RustVec::from_ref_vec_string)),
                true => Some(quote_spanned!(span=> ::cxx::private::RustVec::from_mut_vec_string)),
            },
            Type::RustVec(_) => match ty.mutable {
                false => Some(quote_spanned!(span=> ::cxx::private::RustVec::from_ref)),
                true => Some(quote_spanned!(span=> ::cxx::private::RustVec::from_mut)),
            },
            _ => None,
        },
        Type::Str(_) => Some(quote_spanned!(span=> ::cxx::private::RustStr::from)),
        Type::SliceRef(ty) => match ty.mutable {
            false => Some(quote_spanned!(span=> ::cxx::private::RustSlice::from_ref)),
            true => Some(quote_spanned!(span=> ::cxx::private::RustSlice::from_mut)),
        },
        _ => None,
    });

    let mut expr = match conversion {
        None => call,
        Some(conversion) if !sig.throws => {
            requires_closure = true;
            quote_spanned!(span=> #conversion(#call))
        }
        Some(conversion) => {
            requires_closure = true;
            quote_spanned!(span=> ::cxx::core::result::Result::map(#call, #conversion))
        }
    };

    let mut outparam = None;
    let indirect_return = indirect_return(sig, types);
    if indirect_return {
        let ret = expand_extern_type(sig.ret.as_ref().unwrap(), types, false);
        outparam = Some(quote_spanned!(span=> __return: *mut #ret,));
    }
    if sig.throws {
        let out = match sig.ret {
            Some(_) => quote_spanned!(span=> __return),
            None => quote_spanned!(span=> &mut ()),
        };
        requires_closure = true;
        requires_unsafe = true;
        expr = quote_spanned!(span=> ::cxx::private::r#try(#out, #expr));
    } else if indirect_return {
        requires_closure = true;
        requires_unsafe = true;
        expr = quote_spanned!(span=> ::cxx::core::ptr::write(__return, #expr));
    }

    if requires_unsafe {
        expr = quote_spanned!(span=> unsafe { #expr });
    }

    let closure = if requires_closure {
        quote_spanned!(span=> move || #expr)
    } else {
        quote!(#local_name)
    };

    expr = quote_spanned!(span=> ::cxx::private::prevent_unwind(__fn, #closure));

    let ret = if sig.throws {
        quote!(-> ::cxx::private::Result)
    } else {
        expand_extern_return_type(&sig.ret, types, false)
    };

    let pointer = match invoke {
        None => Some(quote_spanned!(span=> __extern: *const ())),
        Some(_) => None,
    };

    quote_spanned! {span=>
        #attrs
        #[doc(hidden)]
        #[#UnsafeAttr(#ExportNameAttr = #link_name)]
        unsafe extern "C" fn #local_name #generics(#(#all_args,)* #outparam #pointer) #ret {
            let __fn = ::cxx::private::concat!(::cxx::private::module_path!(), #prevent_unwind_label);
            #wrap_super
            #expr
        }
    }
}

// A wrapper like `fn f(x: Arg) { super::f(x) }` just to ensure we have the
// accurate unsafety declaration and no problematic elided lifetimes.
fn expand_rust_function_shim_super(
    sig: &Signature,
    local_name: &Ident,
    invoke: &Ident,
) -> TokenStream {
    let unsafety = sig.unsafety;
    let generics = &sig.generics;

    let receiver_var = sig
        .receiver
        .as_ref()
        .map(|receiver| Ident::new("__self", receiver.var.span));
    let receiver = sig.receiver.iter().map(|receiver| {
        let receiver_type = receiver.ty();
        quote!(#receiver_var: #receiver_type)
    });
    let args = sig.args.iter().map(|arg| quote!(#arg));
    let all_args = receiver.chain(args);

    let ret = if let Some((result, _langle, rangle)) = sig.throws_tokens {
        let ok = match &sig.ret {
            Some(ret) => quote!(#ret),
            None => quote!(()),
        };
        // Set spans that result in the `Result<...>` written by the user being
        // highlighted as the cause if their error type has no Display impl.
        let result_begin = quote_spanned!(result.span=> ::cxx::core::result::Result<#ok, impl);
        let result_end = if rustversion::cfg!(since(1.82)) {
            // https://blog.rust-lang.org/2024/10/17/Rust-1.82.0.html#precise-capturing-use-syntax
            quote_spanned!(rangle.span=> ::cxx::core::fmt::Display + use<>>)
        } else {
            quote_spanned!(rangle.span=> ::cxx::core::fmt::Display>)
        };
        quote!(-> #result_begin #result_end)
    } else {
        expand_return_type(&sig.ret)
    };

    let arg_vars = sig.args.iter().map(|arg| &arg.name.rust);
    let vars = receiver_var.iter().chain(arg_vars);

    let span = invoke.span();
    let call = match &sig.receiver {
        None => quote_spanned!(span=> super::#invoke),
        Some(receiver) => {
            let receiver_type = &receiver.ty.rust;
            quote_spanned!(span=> #receiver_type::#invoke)
        }
    };

    let mut body = quote_spanned!(span=> #call(#(#vars,)*));
    let mut allow_unused_unsafe = None;
    if unsafety.is_some() {
        body = quote_spanned!(span=> unsafe { #body });
        allow_unused_unsafe = Some(quote_spanned!(span=> #[allow(unused_unsafe)]));
    }

    quote_spanned! {span=>
        #allow_unused_unsafe
        #unsafety fn #local_name #generics(#(#all_args,)*) #ret {
            #body
        }
    }
}

fn expand_type_alias(alias: &TypeAlias) -> TokenStream {
    let doc = &alias.doc;
    let attrs = &alias.attrs;
    let visibility = alias.visibility;
    let type_token = alias.type_token;
    let ident = &alias.name.rust;
    let generics = &alias.generics;
    let eq_token = alias.eq_token;
    let ty = &alias.ty;
    let semi_token = alias.semi_token;

    quote! {
        #doc
        #attrs
        #visibility #type_token #ident #generics #eq_token #ty #semi_token
    }
}

fn expand_type_alias_verify(alias: &TypeAlias, types: &Types) -> TokenStream {
    let attrs = &alias.attrs;
    let ident = &alias.name.rust;
    let type_id = type_id(&alias.name);
    let begin_span = alias.type_token.span;
    let end_span = alias.semi_token.span;
    let begin = quote_spanned!(begin_span=> ::cxx::private::verify_extern_type::<);
    let end = quote_spanned!(end_span=> >);

    let mut verify = quote! {
        #attrs
        const _: fn() = #begin #ident, #type_id #end;
    };

    if types.required_trivial.contains_key(&alias.name.rust) {
        let begin = quote_spanned!(begin_span=> ::cxx::private::verify_extern_kind::<);
        verify.extend(quote! {
            #attrs
            const _: fn() = #begin #ident, ::cxx::kind::Trivial #end;
        });
    }

    verify
}

fn type_id(name: &Pair) -> TokenStream {
    let namespace_segments = name.namespace.iter();
    let mut segments = Vec::with_capacity(namespace_segments.len() + 1);
    segments.extend(namespace_segments.cloned());
    segments.push(Ident::new(&name.cxx.to_string(), Span::call_site()));
    let qualified = QualifiedName { segments };
    crate::type_id::expand(Crate::Cxx, qualified)
}

fn expand_rust_box(key: &NamedImplKey, types: &Types, explicit_impl: Option<&Impl>) -> TokenStream {
    let ident = key.rust;
    let resolve = types.resolve(ident);
    let link_prefix = format!("cxxbridge1$box${}$", resolve.name.to_symbol());
    let link_alloc = format!("{}alloc", link_prefix);
    let link_dealloc = format!("{}dealloc", link_prefix);
    let link_drop = format!("{}drop", link_prefix);

    let local_prefix = format_ident!("{}__box_", ident);
    let local_alloc = format_ident!("{}alloc", local_prefix);
    let local_dealloc = format_ident!("{}dealloc", local_prefix);
    let local_drop = format_ident!("{}drop", local_prefix);

    let (impl_generics, ty_generics) = generics::split_for_impl(key, explicit_impl, resolve);

    let begin_span = explicit_impl.map_or(key.begin_span, |explicit| explicit.impl_token.span);
    let end_span = explicit_impl.map_or(key.end_span, |explicit| explicit.brace_token.span.join());
    let unsafe_token = format_ident!("unsafe", span = begin_span);
    let prevent_unwind_drop_label = format!("::{} as Drop>::drop", ident);

    quote_spanned! {end_span=>
        #[automatically_derived]
        #[doc(hidden)]
        #unsafe_token impl #impl_generics ::cxx::private::ImplBox for #ident #ty_generics {}
        #[doc(hidden)]
        #[#UnsafeAttr(#ExportNameAttr = #link_alloc)]
        unsafe extern "C" fn #local_alloc #impl_generics() -> *mut ::cxx::core::mem::MaybeUninit<#ident #ty_generics> {
            // No prevent_unwind: the global allocator is not allowed to panic.
            //
            // TODO: replace with Box::new_uninit when stable.
            // https://doc.rust-lang.org/std/boxed/struct.Box.html#method.new_uninit
            // https://github.com/rust-lang/rust/issues/63291
            ::cxx::alloc::boxed::Box::into_raw(::cxx::alloc::boxed::Box::new(::cxx::core::mem::MaybeUninit::uninit()))
        }
        #[doc(hidden)]
        #[#UnsafeAttr(#ExportNameAttr = #link_dealloc)]
        unsafe extern "C" fn #local_dealloc #impl_generics(ptr: *mut ::cxx::core::mem::MaybeUninit<#ident #ty_generics>) {
            // No prevent_unwind: the global allocator is not allowed to panic.
            let _ = unsafe { ::cxx::alloc::boxed::Box::from_raw(ptr) };
        }
        #[doc(hidden)]
        #[#UnsafeAttr(#ExportNameAttr = #link_drop)]
        unsafe extern "C" fn #local_drop #impl_generics(this: *mut ::cxx::alloc::boxed::Box<#ident #ty_generics>) {
            let __fn = concat!("<", module_path!(), #prevent_unwind_drop_label);
            ::cxx::private::prevent_unwind(__fn, || unsafe { ::cxx::core::ptr::drop_in_place(this) });
        }
    }
}

fn expand_rust_vec(key: &NamedImplKey, types: &Types, explicit_impl: Option<&Impl>) -> TokenStream {
    let elem = key.rust;
    let resolve = types.resolve(elem);
    let link_prefix = format!("cxxbridge1$rust_vec${}$", resolve.name.to_symbol());
    let link_new = format!("{}new", link_prefix);
    let link_drop = format!("{}drop", link_prefix);
    let link_len = format!("{}len", link_prefix);
    let link_capacity = format!("{}capacity", link_prefix);
    let link_data = format!("{}data", link_prefix);
    let link_reserve_total = format!("{}reserve_total", link_prefix);
    let link_set_len = format!("{}set_len", link_prefix);
    let link_truncate = format!("{}truncate", link_prefix);

    let local_prefix = format_ident!("{}__vec_", elem);
    let local_new = format_ident!("{}new", local_prefix);
    let local_drop = format_ident!("{}drop", local_prefix);
    let local_len = format_ident!("{}len", local_prefix);
    let local_capacity = format_ident!("{}capacity", local_prefix);
    let local_data = format_ident!("{}data", local_prefix);
    let local_reserve_total = format_ident!("{}reserve_total", local_prefix);
    let local_set_len = format_ident!("{}set_len", local_prefix);
    let local_truncate = format_ident!("{}truncate", local_prefix);

    let (impl_generics, ty_generics) = generics::split_for_impl(key, explicit_impl, resolve);

    let begin_span = explicit_impl.map_or(key.begin_span, |explicit| explicit.impl_token.span);
    let end_span = explicit_impl.map_or(key.end_span, |explicit| explicit.brace_token.span.join());
    let unsafe_token = format_ident!("unsafe", span = begin_span);
    let prevent_unwind_drop_label = format!("::{} as Drop>::drop", elem);

    quote_spanned! {end_span=>
        #[automatically_derived]
        #[doc(hidden)]
        #unsafe_token impl #impl_generics ::cxx::private::ImplVec for #elem #ty_generics {}
        #[doc(hidden)]
        #[#UnsafeAttr(#ExportNameAttr = #link_new)]
        unsafe extern "C" fn #local_new #impl_generics(this: *mut ::cxx::private::RustVec<#elem #ty_generics>) {
            // No prevent_unwind: cannot panic.
            unsafe {
                ::cxx::core::ptr::write(this, ::cxx::private::RustVec::new());
            }
        }
        #[doc(hidden)]
        #[#UnsafeAttr(#ExportNameAttr = #link_drop)]
        unsafe extern "C" fn #local_drop #impl_generics(this: *mut ::cxx::private::RustVec<#elem #ty_generics>) {
            let __fn = concat!("<", module_path!(), #prevent_unwind_drop_label);
            ::cxx::private::prevent_unwind(
                __fn,
                || unsafe { ::cxx::core::ptr::drop_in_place(this) },
            );
        }
        #[doc(hidden)]
        #[#UnsafeAttr(#ExportNameAttr = #link_len)]
        unsafe extern "C" fn #local_len #impl_generics(this: *const ::cxx::private::RustVec<#elem #ty_generics>) -> usize {
            // No prevent_unwind: cannot panic.
            unsafe { (*this).len() }
        }
        #[doc(hidden)]
        #[#UnsafeAttr(#ExportNameAttr = #link_capacity)]
        unsafe extern "C" fn #local_capacity #impl_generics(this: *const ::cxx::private::RustVec<#elem #ty_generics>) -> usize {
            // No prevent_unwind: cannot panic.
            unsafe { (*this).capacity() }
        }
        #[doc(hidden)]
        #[#UnsafeAttr(#ExportNameAttr = #link_data)]
        unsafe extern "C" fn #local_data #impl_generics(this: *const ::cxx::private::RustVec<#elem #ty_generics>) -> *const #elem #ty_generics {
            // No prevent_unwind: cannot panic.
            unsafe { (*this).as_ptr() }
        }
        #[doc(hidden)]
        #[#UnsafeAttr(#ExportNameAttr = #link_reserve_total)]
        unsafe extern "C" fn #local_reserve_total #impl_generics(this: *mut ::cxx::private::RustVec<#elem #ty_generics>, new_cap: usize) {
            // No prevent_unwind: the global allocator is not allowed to panic.
            unsafe {
                (*this).reserve_total(new_cap);
            }
        }
        #[doc(hidden)]
        #[#UnsafeAttr(#ExportNameAttr = #link_set_len)]
        unsafe extern "C" fn #local_set_len #impl_generics(this: *mut ::cxx::private::RustVec<#elem #ty_generics>, len: usize) {
            // No prevent_unwind: cannot panic.
            unsafe {
                (*this).set_len(len);
            }
        }
        #[doc(hidden)]
        #[#UnsafeAttr(#ExportNameAttr = #link_truncate)]
        unsafe extern "C" fn #local_truncate #impl_generics(this: *mut ::cxx::private::RustVec<#elem #ty_generics>, len: usize) {
            let __fn = concat!("<", module_path!(), #prevent_unwind_drop_label);
            ::cxx::private::prevent_unwind(
                __fn,
                || unsafe { (*this).truncate(len) },
            );
        }
    }
}

fn expand_unique_ptr(
    key: &NamedImplKey,
    types: &Types,
    explicit_impl: Option<&Impl>,
) -> TokenStream {
    let ident = key.rust;
    let name = ident.to_string();
    let resolve = types.resolve(ident);
    let prefix = format!("cxxbridge1$unique_ptr${}$", resolve.name.to_symbol());
    let link_null = format!("{}null", prefix);
    let link_uninit = format!("{}uninit", prefix);
    let link_raw = format!("{}raw", prefix);
    let link_get = format!("{}get", prefix);
    let link_release = format!("{}release", prefix);
    let link_drop = format!("{}drop", prefix);

    let (impl_generics, ty_generics) = generics::split_for_impl(key, explicit_impl, resolve);

    let can_construct_from_value = types.is_maybe_trivial(ident);
    let new_method = if can_construct_from_value {
        let raw_mut = if rustversion::cfg!(since(1.82)) {
            quote!(&raw mut)
        } else {
            quote!(&mut)
        };
        Some(quote! {
            fn __new(value: Self) -> ::cxx::core::mem::MaybeUninit<*mut ::cxx::core::ffi::c_void> {
                #UnsafeExtern extern "C" {
                    #[link_name = #link_uninit]
                    fn __uninit(this: *mut ::cxx::core::mem::MaybeUninit<*mut ::cxx::core::ffi::c_void>) -> *mut ::cxx::core::ffi::c_void;
                }
                let mut repr = ::cxx::core::mem::MaybeUninit::uninit();
                unsafe {
                    __uninit(#raw_mut repr).cast::<#ident #ty_generics>().write(value);
                }
                repr
            }
        })
    } else {
        None
    };

    let begin_span = explicit_impl.map_or(key.begin_span, |explicit| explicit.impl_token.span);
    let end_span = explicit_impl.map_or(key.end_span, |explicit| explicit.brace_token.span.join());
    let unsafe_token = format_ident!("unsafe", span = begin_span);
    let raw_const = if rustversion::cfg!(since(1.82)) {
        quote_spanned!(end_span=> &raw const)
    } else {
        quote_spanned!(end_span=> &)
    };
    let raw_mut = if rustversion::cfg!(since(1.82)) {
        quote_spanned!(end_span=> &raw mut)
    } else {
        quote_spanned!(end_span=> &mut)
    };

    quote_spanned! {end_span=>
        #[automatically_derived]
        #unsafe_token impl #impl_generics ::cxx::private::UniquePtrTarget for #ident #ty_generics {
            fn __typename(f: &mut ::cxx::core::fmt::Formatter<'_>) -> ::cxx::core::fmt::Result {
                f.write_str(#name)
            }
            fn __null() -> ::cxx::core::mem::MaybeUninit<*mut ::cxx::core::ffi::c_void> {
                #UnsafeExtern extern "C" {
                    #[link_name = #link_null]
                    fn __null(this: *mut ::cxx::core::mem::MaybeUninit<*mut ::cxx::core::ffi::c_void>);
                }
                let mut repr = ::cxx::core::mem::MaybeUninit::uninit();
                unsafe {
                    __null(#raw_mut repr);
                }
                repr
            }
            #new_method
            unsafe fn __raw(raw: *mut Self) -> ::cxx::core::mem::MaybeUninit<*mut ::cxx::core::ffi::c_void> {
                #UnsafeExtern extern "C" {
                    #[link_name = #link_raw]
                    fn __raw(this: *mut ::cxx::core::mem::MaybeUninit<*mut ::cxx::core::ffi::c_void>, raw: *mut ::cxx::core::ffi::c_void);
                }
                let mut repr = ::cxx::core::mem::MaybeUninit::uninit();
                unsafe {
                    __raw(#raw_mut repr, raw.cast());
                }
                repr
            }
            unsafe fn __get(repr: ::cxx::core::mem::MaybeUninit<*mut ::cxx::core::ffi::c_void>) -> *const Self {
                #UnsafeExtern extern "C" {
                    #[link_name = #link_get]
                    fn __get(this: *const ::cxx::core::mem::MaybeUninit<*mut ::cxx::core::ffi::c_void>) -> *const ::cxx::core::ffi::c_void;
                }
                unsafe { __get(#raw_const repr).cast() }
            }
            unsafe fn __release(mut repr: ::cxx::core::mem::MaybeUninit<*mut ::cxx::core::ffi::c_void>) -> *mut Self {
                #UnsafeExtern extern "C" {
                    #[link_name = #link_release]
                    fn __release(this: *mut ::cxx::core::mem::MaybeUninit<*mut ::cxx::core::ffi::c_void>) -> *mut ::cxx::core::ffi::c_void;
                }
                unsafe { __release(#raw_mut repr).cast() }
            }
            unsafe fn __drop(mut repr: ::cxx::core::mem::MaybeUninit<*mut ::cxx::core::ffi::c_void>) {
                #UnsafeExtern extern "C" {
                    #[link_name = #link_drop]
                    fn __drop(this: *mut ::cxx::core::mem::MaybeUninit<*mut ::cxx::core::ffi::c_void>);
                }
                unsafe {
                    __drop(#raw_mut repr);
                }
            }
        }
    }
}

fn expand_shared_ptr(
    key: &NamedImplKey,
    types: &Types,
    explicit_impl: Option<&Impl>,
) -> TokenStream {
    let ident = key.rust;
    let name = ident.to_string();
    let resolve = types.resolve(ident);
    let prefix = format!("cxxbridge1$shared_ptr${}$", resolve.name.to_symbol());
    let link_null = format!("{}null", prefix);
    let link_uninit = format!("{}uninit", prefix);
    let link_clone = format!("{}clone", prefix);
    let link_get = format!("{}get", prefix);
    let link_drop = format!("{}drop", prefix);

    let (impl_generics, ty_generics) = generics::split_for_impl(key, explicit_impl, resolve);

    let can_construct_from_value = types.is_maybe_trivial(ident);
    let new_method = if can_construct_from_value {
        Some(quote! {
            unsafe fn __new(value: Self, new: *mut ::cxx::core::ffi::c_void) {
                #UnsafeExtern extern "C" {
                    #[link_name = #link_uninit]
                    fn __uninit(new: *mut ::cxx::core::ffi::c_void) -> *mut ::cxx::core::ffi::c_void;
                }
                unsafe {
                    __uninit(new).cast::<#ident #ty_generics>().write(value);
                }
            }
        })
    } else {
        None
    };

    let begin_span = explicit_impl.map_or(key.begin_span, |explicit| explicit.impl_token.span);
    let end_span = explicit_impl.map_or(key.end_span, |explicit| explicit.brace_token.span.join());
    let unsafe_token = format_ident!("unsafe", span = begin_span);

    quote_spanned! {end_span=>
        #[automatically_derived]
        #unsafe_token impl #impl_generics ::cxx::private::SharedPtrTarget for #ident #ty_generics {
            fn __typename(f: &mut ::cxx::core::fmt::Formatter<'_>) -> ::cxx::core::fmt::Result {
                f.write_str(#name)
            }
            unsafe fn __null(new: *mut ::cxx::core::ffi::c_void) {
                #UnsafeExtern extern "C" {
                    #[link_name = #link_null]
                    fn __null(new: *mut ::cxx::core::ffi::c_void);
                }
                unsafe {
                    __null(new);
                }
            }
            #new_method
            unsafe fn __clone(this: *const ::cxx::core::ffi::c_void, new: *mut ::cxx::core::ffi::c_void) {
                #UnsafeExtern extern "C" {
                    #[link_name = #link_clone]
                    fn __clone(this: *const ::cxx::core::ffi::c_void, new: *mut ::cxx::core::ffi::c_void);
                }
                unsafe {
                    __clone(this, new);
                }
            }
            unsafe fn __get(this: *const ::cxx::core::ffi::c_void) -> *const Self {
                #UnsafeExtern extern "C" {
                    #[link_name = #link_get]
                    fn __get(this: *const ::cxx::core::ffi::c_void) -> *const ::cxx::core::ffi::c_void;
                }
                unsafe { __get(this).cast() }
            }
            unsafe fn __drop(this: *mut ::cxx::core::ffi::c_void) {
                #UnsafeExtern extern "C" {
                    #[link_name = #link_drop]
                    fn __drop(this: *mut ::cxx::core::ffi::c_void);
                }
                unsafe {
                    __drop(this);
                }
            }
        }
    }
}

fn expand_weak_ptr(key: &NamedImplKey, types: &Types, explicit_impl: Option<&Impl>) -> TokenStream {
    let ident = key.rust;
    let name = ident.to_string();
    let resolve = types.resolve(ident);
    let prefix = format!("cxxbridge1$weak_ptr${}$", resolve.name.to_symbol());
    let link_null = format!("{}null", prefix);
    let link_clone = format!("{}clone", prefix);
    let link_downgrade = format!("{}downgrade", prefix);
    let link_upgrade = format!("{}upgrade", prefix);
    let link_drop = format!("{}drop", prefix);

    let (impl_generics, ty_generics) = generics::split_for_impl(key, explicit_impl, resolve);

    let begin_span = explicit_impl.map_or(key.begin_span, |explicit| explicit.impl_token.span);
    let end_span = explicit_impl.map_or(key.end_span, |explicit| explicit.brace_token.span.join());
    let unsafe_token = format_ident!("unsafe", span = begin_span);

    quote_spanned! {end_span=>
        #[automatically_derived]
        #unsafe_token impl #impl_generics ::cxx::private::WeakPtrTarget for #ident #ty_generics {
            fn __typename(f: &mut ::cxx::core::fmt::Formatter<'_>) -> ::cxx::core::fmt::Result {
                f.write_str(#name)
            }
            unsafe fn __null(new: *mut ::cxx::core::ffi::c_void) {
                #UnsafeExtern extern "C" {
                    #[link_name = #link_null]
                    fn __null(new: *mut ::cxx::core::ffi::c_void);
                }
                unsafe {
                    __null(new);
                }
            }
            unsafe fn __clone(this: *const ::cxx::core::ffi::c_void, new: *mut ::cxx::core::ffi::c_void) {
                #UnsafeExtern extern "C" {
                    #[link_name = #link_clone]
                    fn __clone(this: *const ::cxx::core::ffi::c_void, new: *mut ::cxx::core::ffi::c_void);
                }
                unsafe {
                    __clone(this, new);
                }
            }
            unsafe fn __downgrade(shared: *const ::cxx::core::ffi::c_void, weak: *mut ::cxx::core::ffi::c_void) {
                #UnsafeExtern extern "C" {
                    #[link_name = #link_downgrade]
                    fn __downgrade(shared: *const ::cxx::core::ffi::c_void, weak: *mut ::cxx::core::ffi::c_void);
                }
                unsafe {
                    __downgrade(shared, weak);
                }
            }
            unsafe fn __upgrade(weak: *const ::cxx::core::ffi::c_void, shared: *mut ::cxx::core::ffi::c_void) {
                #UnsafeExtern extern "C" {
                    #[link_name = #link_upgrade]
                    fn __upgrade(weak: *const ::cxx::core::ffi::c_void, shared: *mut ::cxx::core::ffi::c_void);
                }
                unsafe {
                    __upgrade(weak, shared);
                }
            }
            unsafe fn __drop(this: *mut ::cxx::core::ffi::c_void) {
                #UnsafeExtern extern "C" {
                    #[link_name = #link_drop]
                    fn __drop(this: *mut ::cxx::core::ffi::c_void);
                }
                unsafe {
                    __drop(this);
                }
            }
        }
    }
}

fn expand_cxx_vector(
    key: &NamedImplKey,
    explicit_impl: Option<&Impl>,
    types: &Types,
) -> TokenStream {
    let elem = key.rust;
    let name = elem.to_string();
    let resolve = types.resolve(elem);
    let prefix = format!("cxxbridge1$std$vector${}$", resolve.name.to_symbol());
    let link_new = format!("{}new", prefix);
    let link_size = format!("{}size", prefix);
    let link_get_unchecked = format!("{}get_unchecked", prefix);
    let link_push_back = format!("{}push_back", prefix);
    let link_pop_back = format!("{}pop_back", prefix);
    let unique_ptr_prefix = format!(
        "cxxbridge1$unique_ptr$std$vector${}$",
        resolve.name.to_symbol(),
    );
    let link_unique_ptr_null = format!("{}null", unique_ptr_prefix);
    let link_unique_ptr_raw = format!("{}raw", unique_ptr_prefix);
    let link_unique_ptr_get = format!("{}get", unique_ptr_prefix);
    let link_unique_ptr_release = format!("{}release", unique_ptr_prefix);
    let link_unique_ptr_drop = format!("{}drop", unique_ptr_prefix);

    let (impl_generics, ty_generics) = generics::split_for_impl(key, explicit_impl, resolve);

    let begin_span = explicit_impl.map_or(key.begin_span, |explicit| explicit.impl_token.span);
    let end_span = explicit_impl.map_or(key.end_span, |explicit| explicit.brace_token.span.join());
    let unsafe_token = format_ident!("unsafe", span = begin_span);

    let can_pass_element_by_value = types.is_maybe_trivial(elem);
    let by_value_methods = if can_pass_element_by_value {
        Some(quote_spanned! {end_span=>
            unsafe fn __push_back(
                this: ::cxx::core::pin::Pin<&mut ::cxx::CxxVector<Self>>,
                value: &mut ::cxx::core::mem::ManuallyDrop<Self>,
            ) {
                #UnsafeExtern extern "C" {
                    #[link_name = #link_push_back]
                    fn __push_back #impl_generics(
                        this: ::cxx::core::pin::Pin<&mut ::cxx::CxxVector<#elem #ty_generics>>,
                        value: *mut ::cxx::core::ffi::c_void,
                    );
                }
                unsafe {
                    __push_back(
                        this,
                        value as *mut ::cxx::core::mem::ManuallyDrop<Self> as *mut ::cxx::core::ffi::c_void,
                    );
                }
            }
            unsafe fn __pop_back(
                this: ::cxx::core::pin::Pin<&mut ::cxx::CxxVector<Self>>,
                out: &mut ::cxx::core::mem::MaybeUninit<Self>,
            ) {
                #UnsafeExtern extern "C" {
                    #[link_name = #link_pop_back]
                    fn __pop_back #impl_generics(
                        this: ::cxx::core::pin::Pin<&mut ::cxx::CxxVector<#elem #ty_generics>>,
                        out: *mut ::cxx::core::ffi::c_void,
                    );
                }
                unsafe {
                    __pop_back(
                        this,
                        out as *mut ::cxx::core::mem::MaybeUninit<Self> as *mut ::cxx::core::ffi::c_void,
                    );
                }
            }
        })
    } else {
        None
    };

    let raw_const = if rustversion::cfg!(since(1.82)) {
        quote_spanned!(end_span=> &raw const)
    } else {
        quote_spanned!(end_span=> &)
    };
    let raw_mut = if rustversion::cfg!(since(1.82)) {
        quote_spanned!(end_span=> &raw mut)
    } else {
        quote_spanned!(end_span=> &mut)
    };

    quote_spanned! {end_span=>
        #[automatically_derived]
        #unsafe_token impl #impl_generics ::cxx::private::VectorElement for #elem #ty_generics {
            fn __typename(f: &mut ::cxx::core::fmt::Formatter<'_>) -> ::cxx::core::fmt::Result {
                f.write_str(#name)
            }
            fn __vector_new() -> *mut ::cxx::CxxVector<Self> {
                #UnsafeExtern extern "C" {
                    #[link_name = #link_new]
                    fn __vector_new #impl_generics() -> *mut ::cxx::CxxVector<#elem #ty_generics>;
                }
                unsafe { __vector_new() }
            }
            fn __vector_size(v: &::cxx::CxxVector<Self>) -> usize {
                #UnsafeExtern extern "C" {
                    #[link_name = #link_size]
                    fn __vector_size #impl_generics(_: &::cxx::CxxVector<#elem #ty_generics>) -> usize;
                }
                unsafe { __vector_size(v) }
            }
            unsafe fn __get_unchecked(v: *mut ::cxx::CxxVector<Self>, pos: usize) -> *mut Self {
                #UnsafeExtern extern "C" {
                    #[link_name = #link_get_unchecked]
                    fn __get_unchecked #impl_generics(
                        v: *mut ::cxx::CxxVector<#elem #ty_generics>,
                        pos: usize,
                    ) -> *mut ::cxx::core::ffi::c_void;
                }
                unsafe { __get_unchecked(v, pos) as *mut Self }
            }
            #by_value_methods
            fn __unique_ptr_null() -> ::cxx::core::mem::MaybeUninit<*mut ::cxx::core::ffi::c_void> {
                #UnsafeExtern extern "C" {
                    #[link_name = #link_unique_ptr_null]
                    fn __unique_ptr_null(this: *mut ::cxx::core::mem::MaybeUninit<*mut ::cxx::core::ffi::c_void>);
                }
                let mut repr = ::cxx::core::mem::MaybeUninit::uninit();
                unsafe {
                    __unique_ptr_null(#raw_mut repr);
                }
                repr
            }
            unsafe fn __unique_ptr_raw(raw: *mut ::cxx::CxxVector<Self>) -> ::cxx::core::mem::MaybeUninit<*mut ::cxx::core::ffi::c_void> {
                #UnsafeExtern extern "C" {
                    #[link_name = #link_unique_ptr_raw]
                    fn __unique_ptr_raw #impl_generics(this: *mut ::cxx::core::mem::MaybeUninit<*mut ::cxx::core::ffi::c_void>, raw: *mut ::cxx::CxxVector<#elem #ty_generics>);
                }
                let mut repr = ::cxx::core::mem::MaybeUninit::uninit();
                unsafe {
                    __unique_ptr_raw(#raw_mut repr, raw);
                }
                repr
            }
            unsafe fn __unique_ptr_get(repr: ::cxx::core::mem::MaybeUninit<*mut ::cxx::core::ffi::c_void>) -> *const ::cxx::CxxVector<Self> {
                #UnsafeExtern extern "C" {
                    #[link_name = #link_unique_ptr_get]
                    fn __unique_ptr_get #impl_generics(this: *const ::cxx::core::mem::MaybeUninit<*mut ::cxx::core::ffi::c_void>) -> *const ::cxx::CxxVector<#elem #ty_generics>;
                }
                unsafe { __unique_ptr_get(#raw_const repr) }
            }
            unsafe fn __unique_ptr_release(mut repr: ::cxx::core::mem::MaybeUninit<*mut ::cxx::core::ffi::c_void>) -> *mut ::cxx::CxxVector<Self> {
                #UnsafeExtern extern "C" {
                    #[link_name = #link_unique_ptr_release]
                    fn __unique_ptr_release #impl_generics(this: *mut ::cxx::core::mem::MaybeUninit<*mut ::cxx::core::ffi::c_void>) -> *mut ::cxx::CxxVector<#elem #ty_generics>;
                }
                unsafe { __unique_ptr_release(#raw_mut repr) }
            }
            unsafe fn __unique_ptr_drop(mut repr: ::cxx::core::mem::MaybeUninit<*mut ::cxx::core::ffi::c_void>) {
                #UnsafeExtern extern "C" {
                    #[link_name = #link_unique_ptr_drop]
                    fn __unique_ptr_drop(this: *mut ::cxx::core::mem::MaybeUninit<*mut ::cxx::core::ffi::c_void>);
                }
                unsafe {
                    __unique_ptr_drop(#raw_mut repr);
                }
            }
        }
    }
}

fn expand_return_type(ret: &Option<Type>) -> TokenStream {
    match ret {
        Some(ret) => quote!(-> #ret),
        None => TokenStream::new(),
    }
}

fn indirect_return(sig: &Signature, types: &Types) -> bool {
    sig.ret
        .as_ref()
        .is_some_and(|ret| sig.throws || types.needs_indirect_abi(ret))
}

fn expand_extern_type(ty: &Type, types: &Types, proper: bool) -> TokenStream {
    match ty {
        Type::Ident(ident) if ident.rust == RustString => {
            let span = ident.rust.span();
            quote_spanned!(span=> ::cxx::private::RustString)
        }
        Type::RustBox(ty) | Type::UniquePtr(ty) => {
            let span = ty.name.span();
            if proper && types.is_considered_improper_ctype(&ty.inner) {
                quote_spanned!(span=> *mut ::cxx::core::ffi::c_void)
            } else {
                let inner = expand_extern_type(&ty.inner, types, proper);
                quote_spanned!(span=> *mut #inner)
            }
        }
        Type::RustVec(ty) => {
            let span = ty.name.span();
            let langle = ty.langle;
            let elem = expand_extern_type(&ty.inner, types, proper);
            let rangle = ty.rangle;
            quote_spanned!(span=> ::cxx::private::RustVec #langle #elem #rangle)
        }
        Type::Ref(ty) => {
            let ampersand = ty.ampersand;
            let lifetime = &ty.lifetime;
            let mutability = ty.mutability;
            match &ty.inner {
                Type::Ident(ident) if ident.rust == RustString => {
                    let span = ident.rust.span();
                    quote_spanned!(span=> #ampersand #lifetime #mutability ::cxx::private::RustString)
                }
                Type::RustVec(ty) => {
                    let span = ty.name.span();
                    let langle = ty.langle;
                    let inner = expand_extern_type(&ty.inner, types, proper);
                    let rangle = ty.rangle;
                    quote_spanned!(span=> #ampersand #lifetime #mutability ::cxx::private::RustVec #langle #inner #rangle)
                }
                inner if proper && types.is_considered_improper_ctype(inner) => {
                    let star = Token![*](ampersand.span);
                    match ty.mutable {
                        false => quote!(#star const ::cxx::core::ffi::c_void),
                        true => quote!(#star #mutability ::cxx::core::ffi::c_void),
                    }
                }
                _ => quote!(#ty),
            }
        }
        Type::Ptr(ty) => {
            if proper && types.is_considered_improper_ctype(&ty.inner) {
                let star = ty.star;
                let mutability = ty.mutability;
                let constness = ty.constness;
                quote!(#star #mutability #constness ::cxx::core::ffi::c_void)
            } else {
                quote!(#ty)
            }
        }
        Type::Str(ty) => {
            let span = ty.ampersand.span;
            let rust_str = Ident::new("RustStr", syn::spanned::Spanned::span(&ty.inner));
            quote_spanned!(span=> ::cxx::private::#rust_str)
        }
        Type::SliceRef(ty) => {
            let span = ty.ampersand.span;
            let rust_slice = Ident::new("RustSlice", ty.bracket.span.join());
            quote_spanned!(span=> ::cxx::private::#rust_slice)
        }
        _ => quote!(#ty),
    }
}

fn expand_extern_return_type(ret: &Option<Type>, types: &Types, proper: bool) -> TokenStream {
    let ret = match ret {
        Some(ret) if !types.needs_indirect_abi(ret) => ret,
        _ => return TokenStream::new(),
    };
    let ty = expand_extern_type(ret, types, proper);
    quote!(-> #ty)
}

// #UnsafeExtern extern "C" {...}
// https://blog.rust-lang.org/2024/10/17/Rust-1.82.0.html#safe-items-with-unsafe-extern
struct UnsafeExtern;

impl ToTokens for UnsafeExtern {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        if rustversion::cfg!(since(1.82)) {
            Token![unsafe](Span::call_site()).to_tokens(tokens);
        }
    }
}

// #[#UnsafeAttr(#ExportNameAttr = "...")]
// https://blog.rust-lang.org/2024/10/17/Rust-1.82.0.html#unsafe-attributes
struct UnsafeAttr;
struct ExportNameAttr;

impl ToTokens for UnsafeAttr {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        if rustversion::cfg!(since(1.82)) {
            Token![unsafe](Span::call_site()).to_tokens(tokens);
        } else {
            Ident::new("cfg_attr", Span::call_site()).to_tokens(tokens);
        }
    }
}

impl ToTokens for ExportNameAttr {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        if rustversion::cfg!(since(1.82)) {
            Ident::new("export_name", Span::call_site()).to_tokens(tokens);
        } else {
            tokens.extend(quote!(all(), export_name));
        }
    }
}
