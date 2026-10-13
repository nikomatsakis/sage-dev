use sage_stash::Stash;

fn main() {
    let mut stash = Stash::new();
    let values = stash.alloc_slice(&[1_u32, 2]);
    stash[values][0] = 99;
}
