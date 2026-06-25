#![allow(warnings)]

mod types;

use types::Wrapper;

fn wrap(x: u32) -> Wrapper {
    Wrapper { value: x }
}

fn unwrap(w: Wrapper) -> u32 {
    w.value
}
