use sage_stash::Stash;

fn main() {
    let mut stash = Stash::new();
    let value = stash.alloc(42_u32);
    stash[value] = 99;
}
