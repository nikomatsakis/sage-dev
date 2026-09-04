#![allow(dead_code, unreachable_code)]

struct Left;
struct Right;
struct Sum;

impl core::ops::Add<Right> for Left {
    type Output = Sum;

    fn add(self, _right: Right) -> Sum {
        Sum
    }
}

fn primitive_add(left: u32, right: u32) -> u32 {
    left + right
}

fn add_literal(left: u32) -> u32 {
    left + 1
}

fn primitive_shift(left: u8, right: i16) -> u8 {
    left << right
}

fn equal(left: u32, right: u32) -> bool {
    left == right
}

fn less(left: u32, right: u32) -> bool {
    left < right
}

fn lazy(left: bool, right: bool) -> bool {
    left && right
}

fn never_to_any_comparison(value: u32) -> bool {
    value == return true
}

fn custom(left: Left, right: Right) -> Sum {
    left + right
}
