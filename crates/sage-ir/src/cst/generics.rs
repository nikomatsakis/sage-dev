use sage_stash::{AllocStashData, Ptr, Slice};

use crate::cst::paths::PathCst;
use crate::cst::ty::TypeCst;
use crate::name::Name;
use crate::span::RelativeSpan;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum GenericParamCst<'db> {
    Type {
        name: Name<'db>,
        bounds: Slice<TypeBoundCst<'db>>,
        span: RelativeSpan,
    },
    Lifetime {
        name: Name<'db>,
        span: RelativeSpan,
    },
    Const {
        name: Name<'db>,
        ty: Ptr<TypeCst<'db>>,
        span: RelativeSpan,
    },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, AllocStashData)]
pub enum TypeBoundCst<'db> {
    Trait(Ptr<PathCst<'db>>),
    Lifetime(Name<'db>),
}
