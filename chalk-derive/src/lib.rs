extern crate proc_macro;

use proc_macro2::TokenStream;
use quote::quote;
use syn::{parse_quote, DeriveInput, GenericParam, Ident, TypeParamBound};

use synstructure::decl_derive;

/// Checks whether a generic parameter has a `: HasInterner` bound
fn has_interner(param: &GenericParam) -> Option<&Ident> {
    bounded_by_trait(param, "HasInterner")
}

/// Checks whether a generic parameter has a `: Interner` bound
fn is_interner(param: &GenericParam) -> Option<&Ident> {
    bounded_by_trait(param, "Interner")
}

fn has_interner_attr(input: &DeriveInput) -> Option<TokenStream> {
    Some(
        input
            .attrs
            .iter()
            .find(|a| a.path.is_ident("has_interner"))?
            .parse_args::<TokenStream>()
            .expect("Expected has_interner argument"),
    )
}

fn bounded_by_trait<'p>(param: &'p GenericParam, name: &str) -> Option<&'p Ident> {
    let name = Some(String::from(name));
    match param {
        GenericParam::Type(ref t) => t.bounds.iter().find_map(|b| {
            if let TypeParamBound::Trait(trait_bound) = b {
                if trait_bound
                    .path
                    .segments
                    .last()
                    .map(|s| s.ident.to_string())
                    == name
                {
                    return Some(&t.ident);
                }
            }
            None
        }),
        _ => None,
    }
}

fn get_generic_param(input: &DeriveInput) -> &GenericParam {
    match input.generics.params.len() {
        1 => {}

        0 => panic!(
            "deriving this trait requires a single type parameter or a `#[has_interner]` attr"
        ),

        _ => panic!("deriving this trait only works with a single type parameter"),
    };
    &input.generics.params[0]
}

fn get_generic_param_name(input: &DeriveInput) -> Option<&Ident> {
    match get_generic_param(input) {
        GenericParam::Type(t) => Some(&t.ident),
        _ => None,
    }
}

fn find_interner(s: &mut synstructure::Structure) -> (TokenStream, DeriveKind) {
    let input = s.ast();

    if let Some(arg) = has_interner_attr(input) {
        // Hardcoded interner:
        //
        // #[has_interner(ChalkIr)]
        // struct S {
        //
        // }
        return (arg, DeriveKind::FromHasInternerAttr);
    }

    let generic_param0 = get_generic_param(input);

    if let Some(param) = has_interner(&generic_param0) {
        // HasInterner bound:
        //
        // Example:
        //
        // struct Binders<T: HasInterner> { }
        s.add_impl_generic(parse_quote! { _I });

        s.add_where_predicate(parse_quote! { _I: ::chalk_ir::interner::Interner });
        s.add_where_predicate(
            parse_quote! { #param: ::chalk_ir::interner::HasInterner<Interner = _I> },
        );

        (quote! { _I }, DeriveKind::FromHasInterner)
    } else if let Some(i) = is_interner(&generic_param0) {
        // Interner bound:
        //
        // Example:
        //
        // struct Foo<I: Interner> { }
        (quote! { #i }, DeriveKind::FromInterner)
    } else {
        panic!("deriving this trait requires a parameter that implements HasInterner or Interner",);
    }
}

#[derive(Copy, Clone, PartialEq)]
enum DeriveKind {
    FromHasInternerAttr,
    FromHasInterner,
    FromInterner,
}

decl_derive!([HasInterner, attributes(has_interner)] => derive_has_interner);
decl_derive!([Visit, attributes(has_interner)] => derive_visit);
decl_derive!([SuperVisit, attributes(has_interner)] => derive_super_visit);
decl_derive!([Fold, attributes(has_interner)] => derive_fold);

fn derive_has_interner(mut s: synstructure::Structure) -> TokenStream {
    let (interner, _) = find_interner(&mut s);

    s.add_bounds(synstructure::AddBounds::None);
    s.bound_impl(
        quote!(::chalk_ir::interner::HasInterner),
        quote! {
            type Interner = #interner;
        },
    )
}

/// Derives Visit for structs and enums for which one of the following is true:
/// - It has a `#[has_interner(TheInterner)]` attribute
/// - There is a single parameter `T: HasInterner` (does not have to be named `T`)
/// - There is a single parameter `I: Interner` (does not have to be named `I`)
fn derive_visit(s: synstructure::Structure) -> TokenStream {
    derive_any_visit(s, parse_quote! { Visit }, parse_quote! { visit_with })
}

/// Same as Visit, but derives SuperVisit instead
fn derive_super_visit(s: synstructure::Structure) -> TokenStream {
    derive_any_visit(
        s,
        parse_quote! { SuperVisit },
        parse_quote! { super_visit_with },
    )
}

fn derive_any_visit(
    mut s: synstructure::Structure,
    trait_name: Ident,
    method_name: Ident,
) -> TokenStream {
    let input = s.ast();
    let (interner, kind) = find_interner(&mut s);

    let body = s.each(|bi| {
        quote! {
            result = result.combine(::chalk_ir::visit::Visit::visit_with(#bi, visitor, outer_binder));
            if result.return_early() {
                return result;
            }
        }
    });

    if kind == DeriveKind::FromHasInterner {
        let param = get_generic_param_name(input).unwrap();
        s.add_where_predicate(parse_quote! { #param: ::chalk_ir::visit::Visit<#interner> });
    }

    s.add_bounds(synstructure::AddBounds::None);
    s.bound_impl(
        quote!(::chalk_ir::visit:: #trait_name <#interner>),
        quote! {
            fn #method_name <'i, R: ::chalk_ir::visit::VisitResult>(
                &self,
                visitor: &mut dyn ::chalk_ir::visit::Visitor < 'i, #interner, Result = R >,
                outer_binder: ::chalk_ir::DebruijnIndex,
            ) -> R
            where
                #interner: 'i
            {
                let mut result = R::new();
                match *self {
                    #body
                }
                return result;
            }
        },
    )
}

/// Derives Fold for structs and enums for which one of the following is true:
/// - It has a `#[has_interner(TheInterner)]` attribute
/// - There is a single parameter `T: HasInterner` (does not have to be named `T`)
/// - There is a single parameter `I: Interner` (does not have to be named `I`)
fn derive_fold(mut s: synstructure::Structure) -> TokenStream {
    let input = s.ast();

    let (interner, kind) = find_interner(&mut s);

    let body = s.each_variant(|vi| {
        let bindings = vi.bindings();
        vi.construct(|_, index| {
            let bind = &bindings[index];
            quote! {
                ::chalk_ir::fold::Fold::fold_with(#bind, folder, outer_binder)?
            }
        })
    });

    let type_name = &input.ident;

    let (target_interner, result) = match kind {
        DeriveKind::FromHasInternerAttr => (interner.clone(), quote! { #type_name }),
        DeriveKind::FromHasInterner => {
            let param = get_generic_param_name(input).unwrap();

            s.add_impl_generic(parse_quote! { _U })
                .add_impl_generic(parse_quote! { _TI })
                .add_where_predicate(
                    parse_quote! { #param: ::chalk_ir::fold::Fold<#interner, _TI, Result = _U> },
                )
                .add_where_predicate(
                    parse_quote! { _U: ::chalk_ir::interner::HasInterner<Interner = _TI> },
                )
                .add_where_predicate(
                    parse_quote! { _TI: ::chalk_ir::interner::TargetInterner<#interner> },
                );

            (quote! { _TI }, quote! { #type_name<_U> })
        }
        DeriveKind::FromInterner => {
            s.add_impl_generic(parse_quote! { _TI })
                .add_where_predicate(
                    parse_quote! { _TI: ::chalk_ir::interner::TargetInterner<#interner> },
                );

            (quote! { _TI }, quote! { #type_name<_TI> })
        }
    };

    s.add_bounds(synstructure::AddBounds::None);
    s.bound_impl(
        quote!(::chalk_ir::fold::Fold<#interner, #target_interner>),
        quote! {
            type Result = #result;

            fn fold_with<'i>(
                &self,
                folder: &mut dyn ::chalk_ir::fold::Folder < 'i, #interner, #target_interner >,
                outer_binder: ::chalk_ir::DebruijnIndex,
            ) -> ::chalk_engine::fallible::Fallible<Self::Result>
            where
                #interner: 'i,
                #target_interner: 'i,
            {
                Ok(match *self { #body })
            }
        },
    )
}
