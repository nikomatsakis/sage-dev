#![allow(warnings)]

fn identity(x: u32) -> u32 {
    x
}

fn add(a: u32, b: u32) -> u32 {
    a + b
}

struct Point {
    x: u32,
    y: u32,
}

fn origin() -> Point {
    Point { x: 0, y: 0 }
}

fn get_x(p: Point) -> u32 {
    p.x
}
