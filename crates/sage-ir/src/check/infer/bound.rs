use crate::ty::Ty;
use sage_stash::Ptr;

/// The bound on an inference variable — narrows monotonically.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Bound<'db> {
    /// No information yet.
    None,
    /// At least this type — more updates may come.
    AtLeast(Ptr<Ty<'db>>),
    /// Exactly this type — final.
    Exactly(Ptr<Ty<'db>>),
}

impl<'db> Bound<'db> {
    pub fn is_none(&self) -> bool {
        matches!(self, Bound::None)
    }

    pub fn is_exactly(&self) -> bool {
        matches!(self, Bound::Exactly(_))
    }

    pub fn ty(&self) -> Option<Ptr<Ty<'db>>> {
        match self {
            Bound::None => None,
            Bound::AtLeast(ty) | Bound::Exactly(ty) => Some(*ty),
        }
    }
}
