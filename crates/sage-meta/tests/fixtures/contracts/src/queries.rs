use salsa::Accumulator;

#[sage::item(crate)]
#[sage::item(crate::nodes)]
#[sage::item(crate::nodes::Root)]
#[sage::item(crate::nodes::Helper)]
#[sage::item(crate::nodes::Leaf)]
mod r#abstract {}

#[salsa::input]
pub struct Input {
    pub value: u32,
    pub violate: bool,
    pub fail: bool,
}

#[salsa::accumulator]
#[derive(Debug)]
pub struct Note(pub u32);

#[sage::item(crate::nodes::Root::run)]
#[salsa::tracked]
pub fn start(db: &dyn salsa::Database, input: Input) -> u32 {
    input.compute(db)
}

#[salsa::tracked]
impl Input {
    #[sage::item(crate::nodes::Leaf::read)]
    #[salsa::tracked]
    pub fn another(self, db: &dyn salsa::Database) -> u32 {
        self.value(db)
    }

    #[sage::item(crate::nodes::Helper::work)]
    #[salsa::tracked]
    pub fn compute(self, db: &dyn salsa::Database) -> u32 {
        assert!(!self.fail(db), "fixture panic");
        Note(self.value(db)).accumulate(db);
        if self.violate(db) {
            forbidden(db, self)
        } else {
            self.value(db) / 2
        }
    }
}

#[sage::item(crate::nodes::Leaf::read)]
#[salsa::tracked]
pub fn forbidden(db: &dyn salsa::Database, input: Input) -> u32 {
    input.value(db)
}

// Unsupported generated provenance must remain an explicit coverage gap.
macro_rules! generate_query {
    ($name:ident) => {
        #[sage::item(crate::nodes::Leaf::read)]
        #[salsa::tracked]
        pub fn $name(db: &dyn salsa::Database, input: Input) -> u32 {
            input.value(db)
        }
    };
}
generate_query!(generated);
