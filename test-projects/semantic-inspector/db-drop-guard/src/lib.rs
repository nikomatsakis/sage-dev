#[derive(Clone)]
struct Db {
    shared: bool,
}

struct DbDropGuard {
    db: Db,
}

impl DbDropGuard {
    fn db(&self) -> Db {
        self.db.clone()
    }
}
