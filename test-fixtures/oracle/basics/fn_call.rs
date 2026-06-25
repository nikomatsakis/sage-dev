#![allow(warnings)]

fn add(a: u32, b: u32) -> u32 {
    a + b
}

fn caller() -> u32 {
    add(0, 0)
}
