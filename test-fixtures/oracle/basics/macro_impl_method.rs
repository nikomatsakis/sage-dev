struct Widget;

macro_rules! impl_widget {
    () => {
        impl Widget {
            fn touch(&self) {}
        }
    };
}

impl_widget!();
