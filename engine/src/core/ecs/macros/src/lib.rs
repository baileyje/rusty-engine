mod component;

use proc_macro::TokenStream;

#[proc_macro_derive(Component)]
pub fn derive_component(item: TokenStream) -> TokenStream {
    component::derive_component(item)
}
