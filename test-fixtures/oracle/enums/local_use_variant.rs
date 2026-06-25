enum Container {
    Wrapped(u32),
}

use Container::Wrapped;

fn make_wrapped(x: u32) -> Container {
    Wrapped(x)
}
