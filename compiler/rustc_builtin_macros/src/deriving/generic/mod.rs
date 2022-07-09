//! Some code that abstracts away much of the boilerplate of writing
//! `derive` instances for traits. Among other things it manages getting
//! access to the fields of the 4 different sorts of structs and enum
//! variants, as well as creating the method and impl ast instances.
//!
//! Supported features (fairly exhaustive):
//!
//! - Methods taking any number of parameters of any type, and returning
//!   any type, other than vectors, bottom and closures.
//! - Generating `impl`s for types with type parameters and lifetimes
//!   (e.g., `Option<T>`), the parameters are automatically given the
//!   current trait as a bound. (This includes separate type parameters
//!   and lifetimes for methods.)
//! - Additional bounds on the type parameters (`TraitDef.additional_bounds`)
//!
//! The most important thing for implementors is the `Substructure` and
//! `SubstructureFields` objects. The latter groups 5 possibilities of the
//! arguments:
//!
//! - `Struct`, when `Self` is a struct (including tuple structs, e.g
//!   `struct T(i32, char)`).
//! - `EnumMatching`, when `Self` is an enum and all the arguments are the
//!   same variant of the enum (e.g., `Some(1)`, `Some(3)` and `Some(4)`)
//! - `EnumNonMatchingCollapsed` when `Self` is an enum and the arguments
//!   are not the same variant (e.g., `None`, `Some(1)` and `None`).
//! - `StaticEnum` and `StaticStruct` for static methods, where the type
//!   being derived upon is either an enum or struct respectively. (Any
//!   argument with type Self is just grouped among the non-self
//!   arguments.)
//!
//! In the first two cases, the values from the corresponding fields in
//! all the arguments are grouped together. For `EnumNonMatchingCollapsed`
//! this isn't possible (different variants have different fields), so the
//! fields are inaccessible. (Previous versions of the deriving infrastructure
//! had a way to expand into code that could access them, at the cost of
//! generating exponential amounts of code; see issue #15375). There are no
//! fields with values in the static cases, so these are treated entirely
//! differently.
//!
//! The non-static cases have `Option<ident>` in several places associated
//! with field `expr`s. This represents the name of the field it is
//! associated with. It is only not `None` when the associated field has
//! an identifier in the source code. For example, the `x`s in the
//! following snippet
//!
//! ```rust
//! # #![allow(dead_code)]
//! struct A { x : i32 }
//!
//! struct B(i32);
//!
//! enum C {
//!     C0(i32),
//!     C1 { x: i32 }
//! }
//! ```
//!
//! The `i32`s in `B` and `C0` don't have an identifier, so the
//! `Option<ident>`s would be `None` for them.
//!
//! In the static cases, the structure is summarized, either into the just
//! spans of the fields or a list of spans and the field idents (for tuple
//! structs and record structs, respectively), or a list of these, for
//! enums (one for each variant). For empty struct and empty enum
//! variants, it is represented as a count of 0.
//!
//! # "`cs`" functions
//!
//! The `cs_...` functions ("combine substructure") are designed to
//! make life easier by providing some pre-made recipes for common
//! threads; mostly calling the function being derived on all the
//! arguments and then combining them back together in some way (or
//! letting the user chose that). They are not meant to be the only
//! way to handle the structures that this code creates.
//!
//! # Examples
//!
//! The following simplified `PartialEq` is used for in-code examples:
//!
//! ```rust
//! trait PartialEq {
//!     fn eq(&self, other: &Self) -> bool;
//! }
//! impl PartialEq for i32 {
//!     fn eq(&self, other: &i32) -> bool {
//!         *self == *other
//!     }
//! }
//! ```
//!
//! Some examples of the values of `SubstructureFields` follow, using the
//! above `PartialEq`, `A`, `B` and `C`.
//!
//! ## Structs
//!
//! When generating the `expr` for the `A` impl, the `SubstructureFields` is
//!
//! ```{.text}
//! Struct(vec![FieldInfo {
//!            span: <span of x>
//!            name: Some(<ident of x>),
//!            self_: <expr for &self.x>,
//!            other: vec![<expr for &other.x]
//!          }])
//! ```
//!
//! For the `B` impl, called with `B(a)` and `B(b)`,
//!
//! ```{.text}
//! Struct(vec![FieldInfo {
//!           span: <span of `i32`>,
//!           name: None,
//!           self_: <expr for &a>
//!           other: vec![<expr for &b>]
//!          }])
//! ```
//!
//! ## Enums
//!
//! When generating the `expr` for a call with `self == C0(a)` and `other
//! == C0(b)`, the SubstructureFields is
//!
//! ```{.text}
//! EnumMatching(0, <ast::Variant for C0>,
//!              vec![FieldInfo {
//!                 span: <span of i32>
//!                 name: None,
//!                 self_: <expr for &a>,
//!                 other: vec![<expr for &b>]
//!               }])
//! ```
//!
//! For `C1 {x}` and `C1 {x}`,
//!
//! ```{.text}
//! EnumMatching(1, <ast::Variant for C1>,
//!              vec![FieldInfo {
//!                 span: <span of x>
//!                 name: Some(<ident of x>),
//!                 self_: <expr for &self.x>,
//!                 other: vec![<expr for &other.x>]
//!                }])
//! ```
//!
//! For `C0(a)` and `C1 {x}` ,
//!
//! ```{.text}
//! EnumNonMatchingCollapsed(
//!     &[<ident for self index value>, <ident of __arg_1 index value>])
//! ```
//!
//! It is the same for when the arguments are flipped to `C1 {x}` and
//! `C0(a)`; the only difference is what the values of the identifiers
//! <ident for self index value> and <ident of __arg_1 index value> will
//! be in the generated code.
//!
//! `EnumNonMatchingCollapsed` deliberately provides far less information
//! than is generally available for a given pair of variants; see #15375
//! for discussion.
//!
//! ## Static
//!
//! A static method on the types above would result in,
//!
//! ```{.text}
//! StaticStruct(<ast::VariantData of A>, Named(vec![(<ident of x>, <span of x>)]))
//!
//! StaticStruct(<ast::VariantData of B>, Unnamed(vec![<span of x>]))
//!
//! StaticEnum(<ast::EnumDef of C>,
//!            vec![(<ident of C0>, <span of C0>, Unnamed(vec![<span of i32>])),
//!                 (<ident of C1>, <span of C1>, Named(vec![(<ident of x>, <span of x>)]))])
//! ```

pub use StaticFields::*;
pub use SubstructureFields::*;

use std::cell::RefCell;
use std::iter;
use std::vec;

use rustc_ast::ptr::P;
use rustc_ast::{self as ast, BinOpKind, EnumDef, Expr, Generics, PatKind};
use rustc_ast::{GenericArg, GenericParamKind, VariantData};
use rustc_attr as attr;
use rustc_data_structures::map_in_place::MapInPlace;
use rustc_expand::base::{Annotatable, ExtCtxt};
use rustc_span::symbol::{kw, sym, Ident, Symbol};
use rustc_span::Span;

use ty::{Bounds, Path, Ref, Self_, Ty};

use crate::deriving;

pub mod ty;

pub struct TraitDef<'a> {
    /// The span for the current #[derive(Foo)] header.
    pub span: Span,

    pub attributes: Vec<ast::Attribute>,

    /// Path of the trait, including any type parameters
    pub path: Path,

    /// Additional bounds required of any type parameters of the type,
    /// other than the current trait
    pub additional_bounds: Vec<Ty>,

    /// Any extra lifetimes and/or bounds, e.g., `D: serialize::Decoder`
    pub generics: Bounds,

    /// Can this trait be derived for unions?
    pub supports_unions: bool,

    pub methods: Vec<MethodDef<'a>>,

    pub associated_types: Vec<(Ident, Ty)>,
}

pub struct MethodDef<'a> {
    /// name of the method
    pub name: Symbol,
    /// List of generics, e.g., `R: rand::Rng`
    pub generics: Bounds,

    /// Is there is a `&self` argument? If not, it is a static function.
    pub explicit_self: bool,

    /// Arguments other than the self argument.
    pub nonself_args: Vec<(Ty, Symbol)>,

    /// Returns type
    pub ret_ty: Ty,

    pub attributes: Vec<ast::Attribute>,

    /// Can we combine fieldless variants for enums into a single match arm?
    pub unify_fieldless_variants: bool,

    pub combine_substructure: RefCell<CombineSubstructureFunc<'a>>,
}

/// All the data about the data structure/method being derived upon.
pub struct Substructure<'a> {
    /// ident of self
    pub type_ident: Ident,
    /// Verbatim access to any non-selflike arguments, i.e. arguments that
    /// don't have type `&Self`.
    pub nonselflike_args: &'a [P<Expr>],
    pub fields: &'a SubstructureFields<'a>,
}

/// Summary of the relevant parts of a struct/enum field.
pub struct FieldInfo {
    pub span: Span,
    /// None for tuple structs/normal enum variants, Some for normal
    /// structs/struct enum variants.
    pub name: Option<Ident>,
    /// The expression corresponding to this field of `self`
    /// (specifically, a reference to it).
    pub self_expr: P<Expr>,
    /// The expressions corresponding to references to this field in
    /// the other selflike arguments.
    pub other_selflike_exprs: Vec<P<Expr>>,
}

/// Fields for a static method
pub enum StaticFields {
    /// Tuple and unit structs/enum variants like this.
    Unnamed(Vec<Span>, bool /*is tuple*/),
    /// Normal structs/struct variants.
    Named(Vec<(Ident, Span)>),
}

/// A summary of the possible sets of fields.
pub enum SubstructureFields<'a> {
    Struct(&'a ast::VariantData, Vec<FieldInfo>),
    /// Matching variants of the enum: variant index, variant count, ast::Variant,
    /// fields: the field name is only non-`None` in the case of a struct
    /// variant.
    EnumMatching(usize, usize, &'a ast::Variant, Vec<FieldInfo>),

    /// Non-matching variants of the enum, but with all state hidden from the
    /// consequent code. The field is a list of `Ident`s bound to the variant
    /// index values for each of the actual input `Self` arguments.
    EnumNonMatchingCollapsed(&'a [Ident]),

    /// A static method where `Self` is a struct.
    StaticStruct(&'a ast::VariantData, StaticFields),
    /// A static method where `Self` is an enum.
    StaticEnum(&'a ast::EnumDef, Vec<(Ident, Span, StaticFields)>),
}

/// Combine the values of all the fields together. The last argument is
/// all the fields of all the structures.
pub type CombineSubstructureFunc<'a> =
    Box<dyn FnMut(&mut ExtCtxt<'_>, Span, &Substructure<'_>) -> BlockOrExpr + 'a>;

pub fn combine_substructure(
    f: CombineSubstructureFunc<'_>,
) -> RefCell<CombineSubstructureFunc<'_>> {
    RefCell::new(f)
}

struct TypeParameter {
    bound_generic_params: Vec<ast::GenericParam>,
    ty: P<ast::Ty>,
}

// The code snippets built up for derived code are sometimes used as blocks
// (e.g. in a function body) and sometimes used as expressions (e.g. in a match
// arm). This structure avoids committing to either form until necessary,
// avoiding the insertion of any unnecessary blocks.
//
// The statements come before the expression.
pub struct BlockOrExpr(Vec<ast::Stmt>, Option<P<Expr>>);

impl BlockOrExpr {
    pub fn new_stmts(stmts: Vec<ast::Stmt>) -> BlockOrExpr {
        BlockOrExpr(stmts, None)
    }

    pub fn new_expr(expr: P<Expr>) -> BlockOrExpr {
        BlockOrExpr(vec![], Some(expr))
    }

    pub fn new_mixed(stmts: Vec<ast::Stmt>, expr: P<Expr>) -> BlockOrExpr {
        BlockOrExpr(stmts, Some(expr))
    }

    // Converts it into a block.
    fn into_block(mut self, cx: &ExtCtxt<'_>, span: Span) -> P<ast::Block> {
        if let Some(expr) = self.1 {
            self.0.push(cx.stmt_expr(expr));
        }
        cx.block(span, self.0)
    }

    // Converts it into an expression.
    fn into_expr(self, cx: &ExtCtxt<'_>, span: Span) -> P<Expr> {
        if self.0.is_empty() {
            match self.1 {
                None => cx.expr_block(cx.block(span, vec![])),
                Some(expr) => expr,
            }
        } else {
            cx.expr_block(self.into_block(cx, span))
        }
    }
}

/// This method helps to extract all the type parameters referenced from a
/// type. For a type parameter `<T>`, it looks for either a `TyPath` that
/// is not global and starts with `T`, or a `TyQPath`.
/// Also include bound generic params from the input type.
fn find_type_parameters(
    ty: &ast::Ty,
    ty_param_names: &[Symbol],
    cx: &ExtCtxt<'_>,
) -> Vec<TypeParameter> {
    use rustc_ast::visit;

    struct Visitor<'a, 'b> {
        cx: &'a ExtCtxt<'b>,
        ty_param_names: &'a [Symbol],
        bound_generic_params_stack: Vec<ast::GenericParam>,
        type_params: Vec<TypeParameter>,
    }

    impl<'a, 'b> visit::Visitor<'a> for Visitor<'a, 'b> {
        fn visit_ty(&mut self, ty: &'a ast::Ty) {
            if let ast::TyKind::Path(_, ref path) = ty.kind {
                if let Some(segment) = path.segments.first() {
                    if self.ty_param_names.contains(&segment.ident.name) {
                        self.type_params.push(TypeParameter {
                            bound_generic_params: self.bound_generic_params_stack.clone(),
                            ty: P(ty.clone()),
                        });
                    }
                }
            }

            visit::walk_ty(self, ty)
        }

        // Place bound generic params on a stack, to extract them when a type is encountered.
        fn visit_poly_trait_ref(
            &mut self,
            trait_ref: &'a ast::PolyTraitRef,
            modifier: &'a ast::TraitBoundModifier,
        ) {
            let stack_len = self.bound_generic_params_stack.len();
            self.bound_generic_params_stack
                .extend(trait_ref.bound_generic_params.clone().into_iter());

            visit::walk_poly_trait_ref(self, trait_ref, modifier);

            self.bound_generic_params_stack.truncate(stack_len);
        }

        fn visit_mac_call(&mut self, mac: &ast::MacCall) {
            self.cx.span_err(mac.span(), "`derive` cannot be used on items with type macros");
        }
    }

    let mut visitor = Visitor {
        cx,
        ty_param_names,
        bound_generic_params_stack: Vec::new(),
        type_params: Vec::new(),
    };
    visit::Visitor::visit_ty(&mut visitor, ty);

    visitor.type_params
}

impl<'a> TraitDef<'a> {
    pub fn expand(
        self,
        cx: &mut ExtCtxt<'_>,
        mitem: &ast::MetaItem,
        item: &'a Annotatable,
        push: &mut dyn FnMut(Annotatable),
    ) {
        self.expand_ext(cx, mitem, item, push, false);
    }

    pub fn expand_ext(
        self,
        cx: &mut ExtCtxt<'_>,
        mitem: &ast::MetaItem,
        item: &'a Annotatable,
        push: &mut dyn FnMut(Annotatable),
        from_scratch: bool,
    ) {
        match *item {
            Annotatable::Item(ref item) => {
                let is_packed = item.attrs.iter().any(|attr| {
                    for r in attr::find_repr_attrs(&cx.sess, attr) {
                        if let attr::ReprPacked(_) = r {
                            return true;
                        }
                    }
                    false
                });
                let has_no_type_params = match item.kind {
                    ast::ItemKind::Struct(_, ref generics)
                    | ast::ItemKind::Enum(_, ref generics)
                    | ast::ItemKind::Union(_, ref generics) => !generics
                        .params
                        .iter()
                        .any(|param| matches!(param.kind, ast::GenericParamKind::Type { .. })),
                    _ => unreachable!(),
                };
                let container_id = cx.current_expansion.id.expn_data().parent.expect_local();
                let always_copy = has_no_type_params && cx.resolver.has_derive_copy(container_id);
                let use_temporaries = is_packed && always_copy;

                let newitem = match item.kind {
                    ast::ItemKind::Struct(ref struct_def, ref generics) => self.expand_struct_def(
                        cx,
                        &struct_def,
                        item.ident,
                        generics,
                        from_scratch,
                        use_temporaries,
                        is_packed,
                    ),
                    ast::ItemKind::Enum(ref enum_def, ref generics) => {
                        // We ignore `use_temporaries` here, because
                        // `repr(packed)` enums cause an error later on.
                        //
                        // This can only cause further compilation errors
                        // downstream in blatantly illegal code, so it
                        // is fine.
                        self.expand_enum_def(cx, enum_def, item.ident, generics, from_scratch)
                    }
                    ast::ItemKind::Union(ref struct_def, ref generics) => {
                        if self.supports_unions {
                            self.expand_struct_def(
                                cx,
                                &struct_def,
                                item.ident,
                                generics,
                                from_scratch,
                                use_temporaries,
                                is_packed,
                            )
                        } else {
                            cx.span_err(mitem.span, "this trait cannot be derived for unions");
                            return;
                        }
                    }
                    _ => unreachable!(),
                };
                // Keep the lint attributes of the previous item to control how the
                // generated implementations are linted
                let mut attrs = newitem.attrs.clone();
                attrs.extend(
                    item.attrs
                        .iter()
                        .filter(|a| {
                            [
                                sym::allow,
                                sym::warn,
                                sym::deny,
                                sym::forbid,
                                sym::stable,
                                sym::unstable,
                            ]
                            .contains(&a.name_or_empty())
                        })
                        .cloned(),
                );
                push(Annotatable::Item(P(ast::Item { attrs, ..(*newitem).clone() })))
            }
            _ => unreachable!(),
        }
    }

    /// Given that we are deriving a trait `DerivedTrait` for a type like:
    ///
    /// ```ignore (only-for-syntax-highlight)
    /// struct Struct<'a, ..., 'z, A, B: DeclaredTrait, C, ..., Z> where C: WhereTrait {
    ///     a: A,
    ///     b: B::Item,
    ///     b1: <B as DeclaredTrait>::Item,
    ///     c1: <C as WhereTrait>::Item,
    ///     c2: Option<<C as WhereTrait>::Item>,
    ///     ...
    /// }
    /// ```
    ///
    /// create an impl like:
    ///
    /// ```ignore (only-for-syntax-highlight)
    /// impl<'a, ..., 'z, A, B: DeclaredTrait, C, ... Z> where
    ///     C:                       WhereTrait,
    ///     A: DerivedTrait + B1 + ... + BN,
    ///     B: DerivedTrait + B1 + ... + BN,
    ///     C: DerivedTrait + B1 + ... + BN,
    ///     B::Item:                 DerivedTrait + B1 + ... + BN,
    ///     <C as WhereTrait>::Item: DerivedTrait + B1 + ... + BN,
    ///     ...
    /// {
    ///     ...
    /// }
    /// ```
    ///
    /// where B1, ..., BN are the bounds given by `bounds_paths`.'. Z is a phantom type, and
    /// therefore does not get bound by the derived trait.
    fn create_derived_impl(
        &self,
        cx: &mut ExtCtxt<'_>,
        type_ident: Ident,
        generics: &Generics,
        field_tys: Vec<P<ast::Ty>>,
        methods: Vec<P<ast::AssocItem>>,
    ) -> P<ast::Item> {
        let trait_path = self.path.to_path(cx, self.span, type_ident, generics);

        // Transform associated types from `deriving::ty::Ty` into `ast::AssocItem`
        let associated_types = self.associated_types.iter().map(|&(ident, ref type_def)| {
            P(ast::AssocItem {
                id: ast::DUMMY_NODE_ID,
                span: self.span,
                ident,
                vis: ast::Visibility {
                    span: self.span.shrink_to_lo(),
                    kind: ast::VisibilityKind::Inherited,
                    tokens: None,
                },
                attrs: Vec::new(),
                kind: ast::AssocItemKind::TyAlias(Box::new(ast::TyAlias {
                    defaultness: ast::Defaultness::Final,
                    generics: Generics::default(),
                    where_clauses: (
                        ast::TyAliasWhereClause::default(),
                        ast::TyAliasWhereClause::default(),
                    ),
                    where_predicates_split: 0,
                    bounds: Vec::new(),
                    ty: Some(type_def.to_ty(cx, self.span, type_ident, generics)),
                })),
                tokens: None,
            })
        });

        let Generics { mut params, mut where_clause, .. } =
            self.generics.to_generics(cx, self.span, type_ident, generics);
        where_clause.span = generics.where_clause.span;
        let ctxt = self.span.ctxt();
        let span = generics.span.with_ctxt(ctxt);

        // Create the generic parameters
        params.extend(generics.params.iter().map(|param| match &param.kind {
            GenericParamKind::Lifetime { .. } => param.clone(),
            GenericParamKind::Type { .. } => {
                // I don't think this can be moved out of the loop, since
                // a GenericBound requires an ast id
                let bounds: Vec<_> =
                    // extra restrictions on the generics parameters to the
                    // type being derived upon
                    self.additional_bounds.iter().map(|p| {
                        cx.trait_bound(p.to_path(cx, self.span, type_ident, generics))
                    }).chain(
                        // require the current trait
                        iter::once(cx.trait_bound(trait_path.clone()))
                    ).chain(
                        // also add in any bounds from the declaration
                        param.bounds.iter().cloned()
                    ).collect();

                cx.typaram(param.ident.span.with_ctxt(ctxt), param.ident, vec![], bounds, None)
            }
            GenericParamKind::Const { ty, kw_span, .. } => {
                let const_nodefault_kind = GenericParamKind::Const {
                    ty: ty.clone(),
                    kw_span: kw_span.with_ctxt(ctxt),

                    // We can't have default values inside impl block
                    default: None,
                };
                let mut param_clone = param.clone();
                param_clone.kind = const_nodefault_kind;
                param_clone
            }
        }));

        // and similarly for where clauses
        where_clause.predicates.extend(generics.where_clause.predicates.iter().map(|clause| {
            match clause {
                ast::WherePredicate::BoundPredicate(wb) => {
                    let span = wb.span.with_ctxt(ctxt);
                    ast::WherePredicate::BoundPredicate(ast::WhereBoundPredicate {
                        span,
                        ..wb.clone()
                    })
                }
                ast::WherePredicate::RegionPredicate(wr) => {
                    let span = wr.span.with_ctxt(ctxt);
                    ast::WherePredicate::RegionPredicate(ast::WhereRegionPredicate {
                        span,
                        ..wr.clone()
                    })
                }
                ast::WherePredicate::EqPredicate(we) => {
                    let span = we.span.with_ctxt(ctxt);
                    ast::WherePredicate::EqPredicate(ast::WhereEqPredicate {
                        id: ast::DUMMY_NODE_ID,
                        span,
                        ..we.clone()
                    })
                }
            }
        }));

        {
            // Extra scope required here so ty_params goes out of scope before params is moved

            let mut ty_params = params
                .iter()
                .filter(|param| matches!(param.kind, ast::GenericParamKind::Type { .. }))
                .peekable();

            if ty_params.peek().is_some() {
                let ty_param_names: Vec<Symbol> =
                    ty_params.map(|ty_param| ty_param.ident.name).collect();

                for field_ty in field_tys {
                    let field_ty_params = find_type_parameters(&field_ty, &ty_param_names, cx);

                    for field_ty_param in field_ty_params {
                        // if we have already handled this type, skip it
                        if let ast::TyKind::Path(_, ref p) = field_ty_param.ty.kind {
                            if p.segments.len() == 1
                                && ty_param_names.contains(&p.segments[0].ident.name)
                            {
                                continue;
                            };
                        }
                        let mut bounds: Vec<_> = self
                            .additional_bounds
                            .iter()
                            .map(|p| cx.trait_bound(p.to_path(cx, self.span, type_ident, generics)))
                            .collect();

                        // require the current trait
                        bounds.push(cx.trait_bound(trait_path.clone()));

                        let predicate = ast::WhereBoundPredicate {
                            span: self.span,
                            bound_generic_params: field_ty_param.bound_generic_params,
                            bounded_ty: field_ty_param.ty,
                            bounds,
                        };

                        let predicate = ast::WherePredicate::BoundPredicate(predicate);
                        where_clause.predicates.push(predicate);
                    }
                }
            }
        }

        let trait_generics = Generics { params, where_clause, span };

        // Create the reference to the trait.
        let trait_ref = cx.trait_ref(trait_path);

        let self_params: Vec<_> = generics
            .params
            .iter()
            .map(|param| match param.kind {
                GenericParamKind::Lifetime { .. } => {
                    GenericArg::Lifetime(cx.lifetime(param.ident.span.with_ctxt(ctxt), param.ident))
                }
                GenericParamKind::Type { .. } => {
                    GenericArg::Type(cx.ty_ident(param.ident.span.with_ctxt(ctxt), param.ident))
                }
                GenericParamKind::Const { .. } => {
                    GenericArg::Const(cx.const_ident(param.ident.span.with_ctxt(ctxt), param.ident))
                }
            })
            .collect();

        // Create the type of `self`.
        let path = cx.path_all(self.span, false, vec![type_ident], self_params);
        let self_type = cx.ty_path(path);

        let attr = cx.attribute(cx.meta_word(self.span, sym::automatically_derived));
        let opt_trait_ref = Some(trait_ref);
        let unused_qual = {
            let word = rustc_ast::attr::mk_nested_word_item(Ident::new(
                sym::unused_qualifications,
                self.span,
            ));
            let list = rustc_ast::attr::mk_list_item(Ident::new(sym::allow, self.span), vec![word]);
            cx.attribute(list)
        };

        let mut a = vec![attr, unused_qual];
        a.extend(self.attributes.iter().cloned());

        cx.item(
            self.span,
            Ident::empty(),
            a,
            ast::ItemKind::Impl(Box::new(ast::Impl {
                unsafety: ast::Unsafe::No,
                polarity: ast::ImplPolarity::Positive,
                defaultness: ast::Defaultness::Final,
                constness: ast::Const::No,
                generics: trait_generics,
                of_trait: opt_trait_ref,
                self_ty: self_type,
                items: methods.into_iter().chain(associated_types).collect(),
            })),
        )
    }

    fn expand_struct_def(
        &self,
        cx: &mut ExtCtxt<'_>,
        struct_def: &'a VariantData,
        type_ident: Ident,
        generics: &Generics,
        from_scratch: bool,
        use_temporaries: bool,
        is_packed: bool,
    ) -> P<ast::Item> {
        let field_tys: Vec<P<ast::Ty>> =
            struct_def.fields().iter().map(|field| field.ty.clone()).collect();

        let methods = self
            .methods
            .iter()
            .map(|method_def| {
                let (explicit_self, selflike_args, nonselflike_args, nonself_arg_tys) =
                    method_def.extract_arg_details(cx, self, type_ident, generics);

                let body = if from_scratch || method_def.is_static() {
                    method_def.expand_static_struct_method_body(
                        cx,
                        self,
                        struct_def,
                        type_ident,
                        &nonselflike_args,
                    )
                } else {
                    method_def.expand_struct_method_body(
                        cx,
                        self,
                        struct_def,
                        type_ident,
                        &selflike_args,
                        &nonselflike_args,
                        use_temporaries,
                        is_packed,
                    )
                };

                method_def.create_method(
                    cx,
                    self,
                    type_ident,
                    generics,
                    explicit_self,
                    nonself_arg_tys,
                    body,
                )
            })
            .collect();

        self.create_derived_impl(cx, type_ident, generics, field_tys, methods)
    }

    fn expand_enum_def(
        &self,
        cx: &mut ExtCtxt<'_>,
        enum_def: &'a EnumDef,
        type_ident: Ident,
        generics: &Generics,
        from_scratch: bool,
    ) -> P<ast::Item> {
        let mut field_tys = Vec::new();

        for variant in &enum_def.variants {
            field_tys.extend(variant.data.fields().iter().map(|field| field.ty.clone()));
        }

        let methods = self
            .methods
            .iter()
            .map(|method_def| {
                let (explicit_self, selflike_args, nonselflike_args, nonself_arg_tys) =
                    method_def.extract_arg_details(cx, self, type_ident, generics);

                let body = if from_scratch || method_def.is_static() {
                    method_def.expand_static_enum_method_body(
                        cx,
                        self,
                        enum_def,
                        type_ident,
                        &nonselflike_args,
                    )
                } else {
                    method_def.expand_enum_method_body(
                        cx,
                        self,
                        enum_def,
                        type_ident,
                        selflike_args,
                        &nonselflike_args,
                    )
                };

                method_def.create_method(
                    cx,
                    self,
                    type_ident,
                    generics,
                    explicit_self,
                    nonself_arg_tys,
                    body,
                )
            })
            .collect();

        self.create_derived_impl(cx, type_ident, generics, field_tys, methods)
    }
}

impl<'a> MethodDef<'a> {
    fn call_substructure_method(
        &self,
        cx: &mut ExtCtxt<'_>,
        trait_: &TraitDef<'_>,
        type_ident: Ident,
        nonselflike_args: &[P<Expr>],
        fields: &SubstructureFields<'_>,
    ) -> BlockOrExpr {
        let span = trait_.span;
        let substructure = Substructure { type_ident, nonselflike_args, fields };
        let mut f = self.combine_substructure.borrow_mut();
        let f: &mut CombineSubstructureFunc<'_> = &mut *f;
        f(cx, span, &substructure)
    }

    fn get_ret_ty(
        &self,
        cx: &mut ExtCtxt<'_>,
        trait_: &TraitDef<'_>,
        generics: &Generics,
        type_ident: Ident,
    ) -> P<ast::Ty> {
        self.ret_ty.to_ty(cx, trait_.span, type_ident, generics)
    }

    fn is_static(&self) -> bool {
        !self.explicit_self
    }

    // The return value includes:
    // - explicit_self: The `&self` arg, if present.
    // - selflike_args: Expressions for `&self` (if present) and also any other
    //   args with the same type (e.g. the `other` arg in `PartialEq::eq`).
    // - nonselflike_args: Expressions for all the remaining args.
    // - nonself_arg_tys: Additional information about all the args other than
    //   `&self`.
    fn extract_arg_details(
        &self,
        cx: &mut ExtCtxt<'_>,
        trait_: &TraitDef<'_>,
        type_ident: Ident,
        generics: &Generics,
    ) -> (Option<ast::ExplicitSelf>, Vec<P<Expr>>, Vec<P<Expr>>, Vec<(Ident, P<ast::Ty>)>) {
        let mut selflike_args = Vec::new();
        let mut nonselflike_args = Vec::new();
        let mut nonself_arg_tys = Vec::new();
        let span = trait_.span;

        let explicit_self = if self.explicit_self {
            let (self_expr, explicit_self) = ty::get_explicit_self(cx, span);
            selflike_args.push(self_expr);
            Some(explicit_self)
        } else {
            None
        };

        for (ty, name) in self.nonself_args.iter() {
            let ast_ty = ty.to_ty(cx, span, type_ident, generics);
            let ident = Ident::new(*name, span);
            nonself_arg_tys.push((ident, ast_ty));

            let arg_expr = cx.expr_ident(span, ident);

            match ty {
                // Selflike (`&Self`) arguments only occur in non-static methods.
                Ref(box Self_, _) if !self.is_static() => {
                    selflike_args.push(cx.expr_deref(span, arg_expr))
                }
                Self_ => cx.span_bug(span, "`Self` in non-return position"),
                _ => nonselflike_args.push(arg_expr),
            }
        }

        (explicit_self, selflike_args, nonselflike_args, nonself_arg_tys)
    }

    fn create_method(
        &self,
        cx: &mut ExtCtxt<'_>,
        trait_: &TraitDef<'_>,
        type_ident: Ident,
        generics: &Generics,
        explicit_self: Option<ast::ExplicitSelf>,
        nonself_arg_tys: Vec<(Ident, P<ast::Ty>)>,
        body: BlockOrExpr,
    ) -> P<ast::AssocItem> {
        let span = trait_.span;
        // Create the generics that aren't for `Self`.
        let fn_generics = self.generics.to_generics(cx, span, type_ident, generics);

        let args = {
            let self_arg = explicit_self.map(|explicit_self| {
                let ident = Ident::with_dummy_span(kw::SelfLower).with_span_pos(span);
                ast::Param::from_self(ast::AttrVec::default(), explicit_self, ident)
            });
            let nonself_args =
                nonself_arg_tys.into_iter().map(|(name, ty)| cx.param(span, name, ty));
            self_arg.into_iter().chain(nonself_args).collect()
        };

        let ret_type = self.get_ret_ty(cx, trait_, generics, type_ident);

        let method_ident = Ident::new(self.name, span);
        let fn_decl = cx.fn_decl(args, ast::FnRetTy::Ty(ret_type));
        let body_block = body.into_block(cx, span);

        let trait_lo_sp = span.shrink_to_lo();

        let sig = ast::FnSig { header: ast::FnHeader::default(), decl: fn_decl, span };
        let defaultness = ast::Defaultness::Final;

        // Create the method.
        P(ast::AssocItem {
            id: ast::DUMMY_NODE_ID,
            attrs: self.attributes.clone(),
            span,
            vis: ast::Visibility {
                span: trait_lo_sp,
                kind: ast::VisibilityKind::Inherited,
                tokens: None,
            },
            ident: method_ident,
            kind: ast::AssocItemKind::Fn(Box::new(ast::Fn {
                defaultness,
                sig,
                generics: fn_generics,
                body: Some(body_block),
            })),
            tokens: None,
        })
    }

    /// The normal case uses field access.
    /// ```
    /// #[derive(PartialEq)]
    /// # struct Dummy;
    /// struct A { x: i32, y: i32 }
    ///
    /// // equivalent to:
    /// impl PartialEq for A {
    ///     fn eq(&self, other: &A) -> bool {
    ///         self.x == other.x && self.y == other.y
    ///     }
    /// }
    /// ```
    /// But if the struct is `repr(packed)`, we can't use something like
    /// `&self.x` on a packed type (as required for e.g. `Debug` and `Hash`)
    /// because that might cause an unaligned ref. So we use let-destructuring
    /// instead.
    /// ```
    /// # struct A { x: i32, y: i32 }
    /// impl PartialEq for A {
    ///     fn eq(&self, other: &A) -> bool {
    ///         let Self { x: ref __self_0_0, y: ref __self_0_1 } = *self;
    ///         let Self { x: ref __self_1_0, y: ref __self_1_1 } = *other;
    ///         *__self_0_0 == *__self_1_0 && *__self_0_1 == *__self_1_1
    ///     }
    /// }
    /// ```
    fn expand_struct_method_body<'b>(
        &self,
        cx: &mut ExtCtxt<'_>,
        trait_: &TraitDef<'b>,
        struct_def: &'b VariantData,
        type_ident: Ident,
        selflike_args: &[P<Expr>],
        nonselflike_args: &[P<Expr>],
        use_temporaries: bool,
        is_packed: bool,
    ) -> BlockOrExpr {
        let span = trait_.span;
        assert!(selflike_args.len() == 1 || selflike_args.len() == 2);

        let mk_body = |cx, selflike_fields| {
            self.call_substructure_method(
                cx,
                trait_,
                type_ident,
                nonselflike_args,
                &Struct(struct_def, selflike_fields),
            )
        };

        if !is_packed {
            let selflike_fields =
                trait_.create_struct_field_access_fields(cx, selflike_args, struct_def);
            mk_body(cx, selflike_fields)
        } else {
            let prefixes: Vec<_> =
                (0..selflike_args.len()).map(|i| format!("__self_{}", i)).collect();
            let selflike_fields =
                trait_.create_struct_pattern_fields(cx, struct_def, &prefixes, use_temporaries);
            let mut body = mk_body(cx, selflike_fields);

            let struct_path = cx.path(span, vec![Ident::new(kw::SelfUpper, type_ident.span)]);
            let patterns = trait_.create_struct_patterns(
                cx,
                struct_path,
                struct_def,
                &prefixes,
                ast::Mutability::Not,
                use_temporaries,
            );

            // Do the let-destructuring.
            let mut stmts: Vec<_> = iter::zip(selflike_args, patterns)
                .map(|(selflike_arg_expr, pat)| {
                    cx.stmt_let_pat(span, pat, selflike_arg_expr.clone())
                })
                .collect();
            stmts.extend(std::mem::take(&mut body.0));
            BlockOrExpr(stmts, body.1)
        }
    }

    fn expand_static_struct_method_body(
        &self,
        cx: &mut ExtCtxt<'_>,
        trait_: &TraitDef<'_>,
        struct_def: &VariantData,
        type_ident: Ident,
        nonselflike_args: &[P<Expr>],
    ) -> BlockOrExpr {
        let summary = trait_.summarise_struct(cx, struct_def);

        self.call_substructure_method(
            cx,
            trait_,
            type_ident,
            nonselflike_args,
            &StaticStruct(struct_def, summary),
        )
    }

    /// ```
    /// #[derive(PartialEq)]
    /// # struct Dummy;
    /// enum A {
    ///     A1,
    ///     A2(i32)
    /// }
    /// ```
    /// is equivalent to:
    /// ```
    /// impl ::core::cmp::PartialEq for A {
    ///     #[inline]
    ///     fn eq(&self, other: &A) -> bool {
    ///         {
    ///             let __self_vi = ::core::intrinsics::discriminant_value(&*self);
    ///             let __arg_1_vi = ::core::intrinsics::discriminant_value(&*other);
    ///             if true && __self_vi == __arg_1_vi {
    ///                 match (&*self, &*other) {
    ///                     (&A::A2(ref __self_0), &A::A2(ref __arg_1_0)) =>
    ///                         (*__self_0) == (*__arg_1_0),
    ///                     _ => true,
    ///                 }
    ///             } else {
    ///                 false // catch-all handler
    ///             }
    ///         }
    ///     }
    /// }
    /// ```
    /// Creates a match for a tuple of all `selflike_args`, where either all
    /// variants match, or it falls into a catch-all for when one variant
    /// does not match.
    ///
    /// There are N + 1 cases because is a case for each of the N
    /// variants where all of the variants match, and one catch-all for
    /// when one does not match.
    ///
    /// As an optimization we generate code which checks whether all variants
    /// match first which makes llvm see that C-like enums can be compiled into
    /// a simple equality check (for PartialEq).
    ///
    /// The catch-all handler is provided access the variant index values
    /// for each of the selflike_args, carried in precomputed variables.
    fn expand_enum_method_body<'b>(
        &self,
        cx: &mut ExtCtxt<'_>,
        trait_: &TraitDef<'b>,
        enum_def: &'b EnumDef,
        type_ident: Ident,
        mut selflike_args: Vec<P<Expr>>,
        nonselflike_args: &[P<Expr>],
    ) -> BlockOrExpr {
        let span = trait_.span;
        let variants = &enum_def.variants;

        let prefixes = iter::once("__self".to_string())
            .chain(
                selflike_args
                    .iter()
                    .enumerate()
                    .skip(1)
                    .map(|(arg_count, _selflike_arg)| format!("__arg_{}", arg_count)),
            )
            .collect::<Vec<String>>();

        // The `vi_idents` will be bound, solely in the catch-all, to
        // a series of let statements mapping each selflike_arg to an int
        // value corresponding to its discriminant.
        let vi_idents = prefixes
            .iter()
            .map(|name| {
                let vi_suffix = format!("{}_vi", name);
                Ident::from_str_and_span(&vi_suffix, span)
            })
            .collect::<Vec<Ident>>();

        // Builds, via callback to call_substructure_method, the
        // delegated expression that handles the catch-all case,
        // using `__variants_tuple` to drive logic if necessary.
        let catch_all_substructure = EnumNonMatchingCollapsed(&vi_idents);

        let first_fieldless = variants.iter().find(|v| v.data.fields().is_empty());

        // These arms are of the form:
        // (Variant1, Variant1, ...) => Body1
        // (Variant2, Variant2, ...) => Body2
        // ...
        // where each tuple has length = selflike_args.len()

        let mut match_arms: Vec<ast::Arm> = variants
            .iter()
            .enumerate()
            .filter(|&(_, v)| !(self.unify_fieldless_variants && v.data.fields().is_empty()))
            .map(|(index, variant)| {
                // A single arm has form (&VariantK, &VariantK, ...) => BodyK
                // (see "Final wrinkle" note below for why.)

                let use_temporaries = false; // enums can't be repr(packed)
                let fields = trait_.create_struct_pattern_fields(
                    cx,
                    &variant.data,
                    &prefixes,
                    use_temporaries,
                );

                let sp = variant.span.with_ctxt(trait_.span.ctxt());
                let variant_path = cx.path(sp, vec![type_ident, variant.ident]);
                let mut subpats: Vec<_> = trait_
                    .create_struct_patterns(
                        cx,
                        variant_path,
                        &variant.data,
                        &prefixes,
                        ast::Mutability::Not,
                        use_temporaries,
                    )
                    .into_iter()
                    .map(|p| cx.pat(span, PatKind::Ref(p, ast::Mutability::Not)))
                    .collect();

                // Here is the pat = `(&VariantK, &VariantK, ...)`
                let single_pat = if subpats.len() == 1 {
                    subpats.pop().unwrap()
                } else {
                    cx.pat_tuple(span, subpats)
                };

                // For the BodyK, we need to delegate to our caller,
                // passing it an EnumMatching to indicate which case
                // we are in.
                //
                // Now, for some given VariantK, we have built up
                // expressions for referencing every field of every
                // Self arg, assuming all are instances of VariantK.
                // Build up code associated with such a case.
                let substructure = EnumMatching(index, variants.len(), variant, fields);
                let arm_expr = self
                    .call_substructure_method(
                        cx,
                        trait_,
                        type_ident,
                        nonselflike_args,
                        &substructure,
                    )
                    .into_expr(cx, span);

                cx.arm(span, single_pat, arm_expr)
            })
            .collect();

        let default = match first_fieldless {
            Some(v) if self.unify_fieldless_variants => {
                // We need a default case that handles the fieldless variants.
                // The index and actual variant aren't meaningful in this case,
                // so just use whatever
                let substructure = EnumMatching(0, variants.len(), v, Vec::new());
                Some(
                    self.call_substructure_method(
                        cx,
                        trait_,
                        type_ident,
                        nonselflike_args,
                        &substructure,
                    )
                    .into_expr(cx, span),
                )
            }
            _ if variants.len() > 1 && selflike_args.len() > 1 => {
                // Since we know that all the arguments will match if we reach
                // the match expression we add the unreachable intrinsics as the
                // result of the catch all which should help llvm in optimizing it
                Some(deriving::call_unreachable(cx, span))
            }
            _ => None,
        };
        if let Some(arm) = default {
            match_arms.push(cx.arm(span, cx.pat_wild(span), arm));
        }

        // We will usually need the catch-all after matching the
        // tuples `(VariantK, VariantK, ...)` for each VariantK of the
        // enum.  But:
        //
        // * when there is only one Self arg, the arms above suffice
        // (and the deriving we call back into may not be prepared to
        // handle EnumNonMatchCollapsed), and,
        //
        // * when the enum has only one variant, the single arm that
        // is already present always suffices.
        //
        // * In either of the two cases above, if we *did* add a
        //   catch-all `_` match, it would trigger the
        //   unreachable-pattern error.
        //
        if variants.len() > 1 && selflike_args.len() > 1 {
            // Build a series of let statements mapping each selflike_arg
            // to its discriminant value.
            //
            // i.e., for `enum E<T> { A, B(1), C(T, T) }`, and a deriving
            // with three Self args, builds three statements:
            // ```
            // let __self_vi = std::intrinsics::discriminant_value(&self);
            // let __arg_1_vi = std::intrinsics::discriminant_value(&arg1);
            // let __arg_2_vi = std::intrinsics::discriminant_value(&arg2);
            // ```
            let mut index_let_stmts: Vec<ast::Stmt> = Vec::with_capacity(vi_idents.len() + 1);

            // We also build an expression which checks whether all discriminants are equal:
            // `__self_vi == __arg_1_vi && __self_vi == __arg_2_vi && ...`
            let mut discriminant_test = cx.expr_bool(span, true);
            for (i, (&ident, selflike_arg)) in iter::zip(&vi_idents, &selflike_args).enumerate() {
                let selflike_addr = cx.expr_addr_of(span, selflike_arg.clone());
                let variant_value = deriving::call_intrinsic(
                    cx,
                    span,
                    sym::discriminant_value,
                    vec![selflike_addr],
                );
                let let_stmt = cx.stmt_let(span, false, ident, variant_value);
                index_let_stmts.push(let_stmt);

                if i > 0 {
                    let id0 = cx.expr_ident(span, vi_idents[0]);
                    let id = cx.expr_ident(span, ident);
                    let test = cx.expr_binary(span, BinOpKind::Eq, id0, id);
                    discriminant_test = if i == 1 {
                        test
                    } else {
                        cx.expr_binary(span, BinOpKind::And, discriminant_test, test)
                    };
                }
            }

            let arm_expr = self
                .call_substructure_method(
                    cx,
                    trait_,
                    type_ident,
                    nonselflike_args,
                    &catch_all_substructure,
                )
                .into_expr(cx, span);

            // Final wrinkle: the selflike_args are expressions that deref
            // down to desired places, but we cannot actually deref
            // them when they are fed as r-values into a tuple
            // expression; here add a layer of borrowing, turning
            // `(*self, *__arg_0, ...)` into `(&*self, &*__arg_0, ...)`.
            selflike_args.map_in_place(|selflike_arg| cx.expr_addr_of(span, selflike_arg));
            let match_arg = cx.expr(span, ast::ExprKind::Tup(selflike_args));

            // Lastly we create an expression which branches on all discriminants being equal
            //  if discriminant_test {
            //      match (...) {
            //          (Variant1, Variant1, ...) => Body1
            //          (Variant2, Variant2, ...) => Body2,
            //          ...
            //          _ => ::core::intrinsics::unreachable()
            //      }
            //  }
            //  else {
            //      <delegated expression referring to __self_vi, et al.>
            //  }
            let all_match = cx.expr_match(span, match_arg, match_arms);
            let arm_expr = cx.expr_if(span, discriminant_test, all_match, Some(arm_expr));
            BlockOrExpr(index_let_stmts, Some(arm_expr))
        } else if variants.is_empty() {
            // There is no sensible code to be generated for *any* deriving on
            // a zero-variant enum. So we just generate a failing expression
            // for the zero variant case.
            BlockOrExpr(vec![], Some(deriving::call_unreachable(cx, span)))
        } else {
            // Final wrinkle: the selflike_args are expressions that deref
            // down to desired places, but we cannot actually deref
            // them when they are fed as r-values into a tuple
            // expression; here add a layer of borrowing, turning
            // `(*self, *__arg_0, ...)` into `(&*self, &*__arg_0, ...)`.
            selflike_args.map_in_place(|selflike_arg| cx.expr_addr_of(span, selflike_arg));
            let match_arg = if selflike_args.len() == 1 {
                selflike_args.pop().unwrap()
            } else {
                cx.expr(span, ast::ExprKind::Tup(selflike_args))
            };
            BlockOrExpr(vec![], Some(cx.expr_match(span, match_arg, match_arms)))
        }
    }

    fn expand_static_enum_method_body(
        &self,
        cx: &mut ExtCtxt<'_>,
        trait_: &TraitDef<'_>,
        enum_def: &EnumDef,
        type_ident: Ident,
        nonselflike_args: &[P<Expr>],
    ) -> BlockOrExpr {
        let summary = enum_def
            .variants
            .iter()
            .map(|v| {
                let sp = v.span.with_ctxt(trait_.span.ctxt());
                let summary = trait_.summarise_struct(cx, &v.data);
                (v.ident, sp, summary)
            })
            .collect();
        self.call_substructure_method(
            cx,
            trait_,
            type_ident,
            nonselflike_args,
            &StaticEnum(enum_def, summary),
        )
    }
}

// general helper methods.
impl<'a> TraitDef<'a> {
    fn summarise_struct(&self, cx: &mut ExtCtxt<'_>, struct_def: &VariantData) -> StaticFields {
        let mut named_idents = Vec::new();
        let mut just_spans = Vec::new();
        for field in struct_def.fields() {
            let sp = field.span.with_ctxt(self.span.ctxt());
            match field.ident {
                Some(ident) => named_idents.push((ident, sp)),
                _ => just_spans.push(sp),
            }
        }

        let is_tuple = matches!(struct_def, ast::VariantData::Tuple(..));
        match (just_spans.is_empty(), named_idents.is_empty()) {
            (false, false) => {
                cx.span_bug(self.span, "a struct with named and unnamed fields in generic `derive`")
            }
            // named fields
            (_, false) => Named(named_idents),
            // unnamed fields
            (false, _) => Unnamed(just_spans, is_tuple),
            // empty
            _ => Named(Vec::new()),
        }
    }

    fn create_struct_patterns(
        &self,
        cx: &mut ExtCtxt<'_>,
        struct_path: ast::Path,
        struct_def: &'a VariantData,
        prefixes: &[String],
        mutbl: ast::Mutability,
        use_temporaries: bool,
    ) -> Vec<P<ast::Pat>> {
        prefixes
            .iter()
            .map(|prefix| {
                let pieces_iter =
                    struct_def.fields().iter().enumerate().map(|(i, struct_field)| {
                        let sp = struct_field.span.with_ctxt(self.span.ctxt());
                        let binding_mode = if use_temporaries {
                            ast::BindingMode::ByValue(ast::Mutability::Not)
                        } else {
                            ast::BindingMode::ByRef(mutbl)
                        };
                        let ident = self.mk_pattern_ident(prefix, i);
                        let path = ident.with_span_pos(sp);
                        (
                            sp,
                            struct_field.ident,
                            cx.pat(path.span, PatKind::Ident(binding_mode, path, None)),
                        )
                    });

                let struct_path = struct_path.clone();
                match *struct_def {
                    VariantData::Struct(..) => {
                        let field_pats = pieces_iter
                            .map(|(sp, ident, pat)| {
                                if ident.is_none() {
                                    cx.span_bug(
                                        sp,
                                        "a braced struct with unnamed fields in `derive`",
                                    );
                                }
                                ast::PatField {
                                    ident: ident.unwrap(),
                                    is_shorthand: false,
                                    attrs: ast::AttrVec::new(),
                                    id: ast::DUMMY_NODE_ID,
                                    span: pat.span.with_ctxt(self.span.ctxt()),
                                    pat,
                                    is_placeholder: false,
                                }
                            })
                            .collect();
                        cx.pat_struct(self.span, struct_path, field_pats)
                    }
                    VariantData::Tuple(..) => {
                        let subpats = pieces_iter.map(|(_, _, subpat)| subpat).collect();
                        cx.pat_tuple_struct(self.span, struct_path, subpats)
                    }
                    VariantData::Unit(..) => cx.pat_path(self.span, struct_path),
                }
            })
            .collect()
    }

    fn create_fields<F>(&self, struct_def: &'a VariantData, mk_exprs: F) -> Vec<FieldInfo>
    where
        F: Fn(usize, &ast::FieldDef, Span) -> Vec<P<ast::Expr>>,
    {
        struct_def
            .fields()
            .iter()
            .enumerate()
            .map(|(i, struct_field)| {
                // For this field, get an expr for each selflike_arg. E.g. for
                // `PartialEq::eq`, one for each of `&self` and `other`.
                let sp = struct_field.span.with_ctxt(self.span.ctxt());
                let mut exprs: Vec<_> = mk_exprs(i, struct_field, sp);
                let self_expr = exprs.remove(0);
                let other_selflike_exprs = exprs;
                FieldInfo {
                    span: sp.with_ctxt(self.span.ctxt()),
                    name: struct_field.ident,
                    self_expr,
                    other_selflike_exprs,
                }
            })
            .collect()
    }

    fn mk_pattern_ident(&self, prefix: &str, i: usize) -> Ident {
        Ident::from_str_and_span(&format!("{}_{}", prefix, i), self.span)
    }

    fn create_struct_pattern_fields(
        &self,
        cx: &mut ExtCtxt<'_>,
        struct_def: &'a VariantData,
        prefixes: &[String],
        use_temporaries: bool,
    ) -> Vec<FieldInfo> {
        self.create_fields(struct_def, |i, _struct_field, sp| {
            prefixes
                .iter()
                .map(|prefix| {
                    let ident = self.mk_pattern_ident(prefix, i);
                    let expr = cx.expr_path(cx.path_ident(sp, ident));
                    if use_temporaries { expr } else { cx.expr_deref(sp, expr) }
                })
                .collect()
        })
    }

    fn create_struct_field_access_fields(
        &self,
        cx: &mut ExtCtxt<'_>,
        selflike_args: &[P<Expr>],
        struct_def: &'a VariantData,
    ) -> Vec<FieldInfo> {
        self.create_fields(struct_def, |i, struct_field, sp| {
            selflike_args
                .iter()
                .map(|mut selflike_arg| {
                    // We don't the need the deref, if there is one.
                    if let ast::ExprKind::Unary(ast::UnOp::Deref, inner) = &selflike_arg.kind {
                        selflike_arg = inner;
                    }
                    // Note: we must use `struct_field.span` rather than `span` in the
                    // `unwrap_or_else` case otherwise the hygiene is wrong and we get
                    // "field `0` of struct `Point` is private" errors on tuple
                    // structs.
                    cx.expr(
                        sp,
                        ast::ExprKind::Field(
                            selflike_arg.clone(),
                            struct_field.ident.unwrap_or_else(|| {
                                Ident::from_str_and_span(&i.to_string(), struct_field.span)
                            }),
                        ),
                    )
                })
                .collect()
        })
    }
}

/// The function passed to `cs_fold` is called repeatedly with a value of this
/// type. It describes one part of the code generation. The result is always an
/// expression.
pub enum CsFold<'a> {
    /// The basic case: a field expression for one or more selflike args. E.g.
    /// for `PartialEq::eq` this is something like `self.x == other.x`.
    Single(&'a FieldInfo),

    /// The combination of two field expressions. E.g. for `PartialEq::eq` this
    /// is something like `<field1 equality> && <field2 equality>`.
    Combine(Span, P<Expr>, P<Expr>),

    // The fallback case for a struct or enum variant with no fields.
    Fieldless,

    /// The fallback case for non-matching enum variants. The slice is the
    /// identifiers holding the variant index value for each of the `Self`
    /// arguments.
    EnumNonMatching(Span, &'a [Ident]),
}

/// Folds over fields, combining the expressions for each field in a sequence.
/// Statics may not be folded over.
pub fn cs_fold<F>(
    use_foldl: bool,
    cx: &mut ExtCtxt<'_>,
    trait_span: Span,
    substructure: &Substructure<'_>,
    mut f: F,
) -> P<Expr>
where
    F: FnMut(&mut ExtCtxt<'_>, CsFold<'_>) -> P<Expr>,
{
    match *substructure.fields {
        EnumMatching(.., ref all_fields) | Struct(_, ref all_fields) => {
            if all_fields.is_empty() {
                return f(cx, CsFold::Fieldless);
            }

            let (base_field, rest) = if use_foldl {
                all_fields.split_first().unwrap()
            } else {
                all_fields.split_last().unwrap()
            };

            let base_expr = f(cx, CsFold::Single(base_field));

            let op = |old, field: &FieldInfo| {
                let new = f(cx, CsFold::Single(field));
                f(cx, CsFold::Combine(field.span, old, new))
            };

            if use_foldl {
                rest.iter().fold(base_expr, op)
            } else {
                rest.iter().rfold(base_expr, op)
            }
        }
        EnumNonMatchingCollapsed(tuple) => f(cx, CsFold::EnumNonMatching(trait_span, tuple)),
        StaticEnum(..) | StaticStruct(..) => cx.span_bug(trait_span, "static function in `derive`"),
    }
}

/// Returns `true` if the type has no value fields
/// (for an enum, no variant has any fields)
pub fn is_type_without_fields(item: &Annotatable) -> bool {
    if let Annotatable::Item(ref item) = *item {
        match item.kind {
            ast::ItemKind::Enum(ref enum_def, _) => {
                enum_def.variants.iter().all(|v| v.data.fields().is_empty())
            }
            ast::ItemKind::Struct(ref variant_data, _) => variant_data.fields().is_empty(),
            _ => false,
        }
    } else {
        false
    }
}
