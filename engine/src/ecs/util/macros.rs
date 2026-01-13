/// Implement another macro for tuples of types recursively.
#[macro_export]
macro_rules! for_every_tuple {
    ($m:ident !! $head_ty:ident) => {
        $m!($head_ty);
    };
    ($m:ident !! $head_ty:ident, $($tail_ty:ident),*) => (
        $m!($head_ty, $( $tail_ty ),*);
        $crate::for_every_tuple!($m !! $( $tail_ty ),*);
    );
}

/// Apply a macro to all tuple combinations from A to Z.
#[macro_export]
macro_rules! all_tuples {
    ($m:ident) => {
        $crate::for_every_tuple!($m !! A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z);
    };
}

#[cfg(test)]
mod tests {
    use std::marker::PhantomData;

    struct Data<Params>(PhantomData<Params>);

    macro_rules! test_tupel_macro {
         ($($name: ident),*) => {
            #[allow(dead_code)]
            impl<$($name),*> Data<($($name,)*)> {
                pub fn works(&self) -> bool {
                    true
                 }
            }
        }
    }

    all_tuples!(test_tupel_macro);

    #[test]
    fn test_macro_works() {
        // Given
        let data = Data::<(
            i32,
            f32,
            f32,
            f32,
            f32,
            f32,
            f32,
            f32,
            f32,
            f32,
            f32,
            f32,
            f32,
            f32,
            f32,
            f32,
            f32,
            f32,
            f32,
            f32,
            f32,
            f32,
            f32,
            f32,
            f32,
            f32,
        )>(PhantomData);
        // Then
        assert!(data.works());
    }
}

