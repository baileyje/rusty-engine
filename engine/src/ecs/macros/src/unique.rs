use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, parse_macro_input};

pub fn derive_unique(input: TokenStream) -> TokenStream {
    // Parse the input tokens into a syntax tree
    let ast = parse_macro_input!(input as DeriveInput);

    // Get the struct name we are annotating
    let struct_name = &ast.ident;

    // Use ::rusty_engine::ecs::unique::Unique which works both inside and outside the crate.
    // Inside the crate, this works because of `extern crate self as rusty_engine;` in lib.rs
    // Outside the crate, this naturally resolves to the rusty_engine dependency.
    TokenStream::from(quote! {
        impl ::rusty_engine::ecs::Unique for #struct_name {
        }
    })
}
