mod component;
mod event;
mod unique;

use proc_macro::TokenStream;

#[proc_macro_derive(Component)]
pub fn derive_component(item: TokenStream) -> TokenStream {
    component::derive_component(item)
}

#[proc_macro_derive(Unique)]
pub fn derive_unique(item: TokenStream) -> TokenStream {
    unique::derive_unique(item)
}

#[proc_macro_derive(Event)]
pub fn derive_event(item: TokenStream) -> TokenStream {
    event::derive_event(item)
}
