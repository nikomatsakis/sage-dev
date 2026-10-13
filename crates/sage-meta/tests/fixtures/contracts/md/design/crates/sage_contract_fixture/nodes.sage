module crate::nodes;

class Root {
    """An entry point whose computation may use a helper but never the leaf."""
    query run() -> u32 {
        """Request the helper's result."""
        envelope {
        invokes crate::nodes::Helper::work();
        forbid transitive crate::nodes::Leaf::read;
        }
    }
}

class Helper {
    """An operation that must compute without requesting the leaf."""
    query work() -> u32 {
        """Compute a small result from the fixture inputs."""
        envelope {}
    }
}

class Leaf {
    """A separate operation used to demonstrate a forbidden dependency."""
    query read() -> u32 {
        """Return the input value."""
        envelope {}
    }
}
