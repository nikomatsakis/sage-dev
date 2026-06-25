#![allow(warnings)]

enum Wrapper {
    Val(u32),
}

fn wrap(x: u32) -> Wrapper {
    Wrapper::Val(x)
}
