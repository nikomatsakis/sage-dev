use crate::Db;

#[salsa::db]
#[derive(Clone, Default)]
pub struct Database {
    storage: salsa::Storage<Self>,
}

#[salsa::db]
impl Db for Database {}

#[salsa::db]
impl salsa::Database for Database {}
