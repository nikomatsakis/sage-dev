use sage_stash::{AllocStashData, Ptr, Slice};

use crate::cst::paths::PathCst;
use crate::name::Name;
use crate::span::RelativeSpan;
use crate::types::Mutability;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub struct TypeCst<'db> {
    pub kind: TypeCstKind<'db>,
    pub span: RelativeSpan,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum TypeCstKind<'db> {
    Path(Ptr<PathCst<'db>>),
    Reference(Ptr<TypeCst<'db>>, Mutability),
    Slice(Ptr<TypeCst<'db>>),
    Array(Ptr<TypeCst<'db>>),
    Tuple(Slice<TypeCst<'db>>),
    Fn(Slice<TypeCst<'db>>, Option<Ptr<TypeCst<'db>>>),
    Never,
    Infer,
    Error,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum LifetimeCst<'db> {
    Named(Name<'db>),
    Anonymous,
}
