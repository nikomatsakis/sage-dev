//# RUSTC ERROR
enum Color {
    Red,
}

fn bad() -> Color {
    Color::Blue //# ERROR
}
