#![feature(register_tool)]
#![register_tool(sage)]
#![cfg(feature = "salsa")]

use sage_stash::{Ptr, Slice, Stash, Stashed};
use salsa::Setter;
use std::sync::{Arc, Mutex};

#[salsa::db]
#[derive(Clone, Default)]
struct Database {
    storage: salsa::Storage<Self>,
    executions: Arc<Mutex<Vec<&'static str>>>,
}

#[salsa::db]
trait Db: salsa::Database {
    fn executed(&self, query: &'static str);
}

#[salsa::db]
impl salsa::Database for Database {}

#[salsa::db]
impl Db for Database {
    fn executed(&self, query: &'static str) {
        self.executions.lock().unwrap().push(query);
    }
}

#[salsa::input]
struct Input {
    value: u32,
    layout: bool,
    unrelated: u32,
}

#[salsa::tracked]
fn produce(db: &dyn Db, input: Input) -> Stashed<Slice<Ptr<u32>>> {
    db.executed("produce");
    let mut stash = Stash::new();
    let layout = input.layout(db);
    if layout {
        stash.alloc(999_u64);
    }
    let value = input.value(db);
    let (first, second) = if layout {
        let second = stash.alloc(value + 1);
        (stash.alloc(value), second)
    } else {
        let first = stash.alloc(value);
        (first, stash.alloc(value + 1))
    };
    let root = stash.alloc_slice(&[first, second]);
    Stashed::new(stash, root)
}

#[salsa::tracked]
fn consume(db: &dyn Db, input: Input) -> u32 {
    db.executed("consume");
    let tree = produce(db, input);
    let (stash, root) = tree.open();
    stash[root].iter().map(|pointer| stash[*pointer]).sum()
}

#[test]
#[sage::test(crates.sage_stash.storage.query_value)]
fn equal_reconstructed_roots_backdate_but_changed_content_reexecutes_consumers() {
    let mut db = Database::default();
    let input = Input::new(&db, 10, false, 0);
    assert_eq!(consume(&db, input), 21);
    assert_eq!(*db.executions.lock().unwrap(), ["consume", "produce"]);
    db.executions.lock().unwrap().clear();

    assert_eq!(consume(&db, input), 21);
    assert!(
        db.executions.lock().unwrap().is_empty(),
        "warm demand reuses both queries"
    );

    input.set_layout(&mut db).to(true);
    assert!(
        db.executions.lock().unwrap().is_empty(),
        "edits alone do not execute queries"
    );
    assert_eq!(consume(&db, input), 21);
    assert_eq!(
        *db.executions.lock().unwrap(),
        ["produce"],
        "equal root stops downstream execution despite new handles and allocation order"
    );
    db.executions.lock().unwrap().clear();

    input.set_value(&mut db).to(20);
    assert_eq!(consume(&db, input), 41);
    assert_eq!(
        *db.executions.lock().unwrap(),
        ["produce", "consume"],
        "reachable changes propagate through the root value"
    );
    db.executions.lock().unwrap().clear();

    input.set_unrelated(&mut db).to(1);
    assert_eq!(consume(&db, input), 41);
    assert!(
        db.executions.lock().unwrap().is_empty(),
        "unread inputs do not invalidate the tree"
    );
}
