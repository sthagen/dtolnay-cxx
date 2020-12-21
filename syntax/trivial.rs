use crate::syntax::set::OrderedSet as Set;
use crate::syntax::{Api, Enum, ExternFn, Pair, RustName, Struct, Type};
use proc_macro2::Ident;
use std::collections::BTreeMap as Map;
use std::fmt::{self, Display};

#[derive(Copy, Clone)]
pub enum TrivialReason<'a> {
    StructField(&'a Struct),
    FunctionArgument(&'a ExternFn),
    FunctionReturn(&'a ExternFn),
    BoxTarget,
    VecElement,
    UnpinnedMutArg(&'a ExternFn),
}

pub fn required_trivial_reasons<'a>(
    apis: &'a [Api],
    all: &Set<&'a Type>,
    structs: &Map<&'a Ident, &'a Struct>,
    enums: &Map<&'a Ident, &'a Enum>,
    cxx: &Set<&'a Ident>,
) -> Map<&'a Ident, Vec<TrivialReason<'a>>> {
    let mut required_trivial = Map::new();

    let mut insist_extern_types_are_trivial = |ident: &'a RustName, reason| {
        if cxx.contains(&ident.rust)
            && !structs.contains_key(&ident.rust)
            && !enums.contains_key(&ident.rust)
        {
            required_trivial
                .entry(&ident.rust)
                .or_insert_with(Vec::new)
                .push(reason);
        }
    };

    for api in apis {
        match api {
            Api::Struct(strct) => {
                for field in &strct.fields {
                    if let Type::Ident(ident) = &field.ty {
                        let reason = TrivialReason::StructField(strct);
                        insist_extern_types_are_trivial(ident, reason);
                    }
                }
            }
            Api::CxxFunction(efn) | Api::RustFunction(efn) => {
                if let Some(receiver) = &efn.receiver {
                    if receiver.mutable && !receiver.pinned {
                        let reason = TrivialReason::UnpinnedMutArg(efn);
                        insist_extern_types_are_trivial(&receiver.ty, reason);
                    }
                }
                for arg in &efn.args {
                    match &arg.ty {
                        Type::Ident(ident) => {
                            let reason = TrivialReason::FunctionArgument(efn);
                            insist_extern_types_are_trivial(ident, reason);
                        }
                        Type::Ref(ty) => {
                            if ty.mutable && !ty.pinned {
                                if let Type::Ident(ident) = &ty.inner {
                                    let reason = TrivialReason::UnpinnedMutArg(efn);
                                    insist_extern_types_are_trivial(ident, reason);
                                }
                            }
                        }
                        _ => {}
                    }
                }
                if let Some(ret) = &efn.ret {
                    if let Type::Ident(ident) = &ret {
                        let reason = TrivialReason::FunctionReturn(efn);
                        insist_extern_types_are_trivial(ident, reason);
                    }
                }
            }
            _ => {}
        }
    }

    for ty in all {
        match ty {
            Type::RustBox(ty) => {
                if let Type::Ident(ident) = &ty.inner {
                    let reason = TrivialReason::BoxTarget;
                    insist_extern_types_are_trivial(ident, reason);
                }
            }
            Type::RustVec(ty) => {
                if let Type::Ident(ident) = &ty.inner {
                    let reason = TrivialReason::VecElement;
                    insist_extern_types_are_trivial(ident, reason);
                }
            }
            _ => {}
        }
    }

    required_trivial
}

// Context:
// "type {type} should be trivially move constructible and trivially destructible in C++ to be used as {what} in Rust"
// "needs a cxx::ExternType impl in order to be used as {what}"
pub fn as_what<'a>(name: &'a Pair, reasons: &'a [TrivialReason]) -> impl Display + 'a {
    struct Description<'a> {
        name: &'a Pair,
        reasons: &'a [TrivialReason<'a>],
    }

    impl<'a> Display for Description<'a> {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            let mut field_of = Set::new();
            let mut argument_of = Set::new();
            let mut return_of = Set::new();
            let mut box_target = false;
            let mut vec_element = false;
            let mut unpinned_mut = Set::new();

            for reason in self.reasons {
                match reason {
                    TrivialReason::StructField(strct) => {
                        field_of.insert(&strct.name.rust);
                    }
                    TrivialReason::FunctionArgument(efn) => {
                        argument_of.insert(&efn.name.rust);
                    }
                    TrivialReason::FunctionReturn(efn) => {
                        return_of.insert(&efn.name.rust);
                    }
                    TrivialReason::BoxTarget => box_target = true,
                    TrivialReason::VecElement => vec_element = true,
                    TrivialReason::UnpinnedMutArg(efn) => {
                        unpinned_mut.insert(&efn.name.rust);
                    }
                }
            }

            let mut clauses = Vec::new();
            if !field_of.is_empty() {
                clauses.push(Clause::Set {
                    article: "a",
                    desc: "field of",
                    set: &field_of,
                });
            }
            if !argument_of.is_empty() {
                clauses.push(Clause::Set {
                    article: "an",
                    desc: "argument of",
                    set: &argument_of,
                });
            }
            if !return_of.is_empty() {
                clauses.push(Clause::Set {
                    article: "a",
                    desc: "return value of",
                    set: &return_of,
                });
            }
            if box_target {
                clauses.push(Clause::Ty1 {
                    article: "type",
                    desc: "Box",
                    param: self.name,
                });
            }
            if vec_element {
                clauses.push(Clause::Ty1 {
                    article: "a",
                    desc: "vector element in Vec",
                    param: self.name,
                });
            }
            if !unpinned_mut.is_empty() {
                clauses.push(Clause::Set {
                    article: "a",
                    desc: "non-pinned mutable reference argument of",
                    set: &unpinned_mut,
                });
            }

            for (i, clause) in clauses.iter().enumerate() {
                if i == 0 {
                    write!(f, "{} ", clause.article())?;
                } else if i + 1 < clauses.len() {
                    write!(f, ", ")?;
                } else {
                    write!(f, " or ")?;
                }
                clause.fmt(f)?;
            }

            Ok(())
        }
    }

    enum Clause<'a> {
        Set {
            article: &'a str,
            desc: &'a str,
            set: &'a Set<&'a Ident>,
        },
        Ty1 {
            article: &'a str,
            desc: &'a str,
            param: &'a Pair,
        },
    }

    impl<'a> Clause<'a> {
        fn article(&self) -> &'a str {
            match self {
                Clause::Set { article, .. } | Clause::Ty1 { article, .. } => article,
            }
        }

        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            match self {
                Clause::Set {
                    article: _,
                    desc,
                    set,
                } => {
                    write!(f, "{} ", desc)?;
                    for (i, ident) in set.iter().take(3).enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "`{}`", ident)?;
                    }
                    Ok(())
                }
                Clause::Ty1 {
                    article: _,
                    desc,
                    param,
                } => write!(f, "{}<{}>", desc, param.rust),
            }
        }
    }

    Description { name, reasons }
}