extern crate proc_macro;

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Ident, ItemImpl};

extern "C" {
    fn get_sizeof_window() -> usize;
}

#[proc_macro_attribute]
pub fn xwindow_info_struct(attr: TokenStream, input: TokenStream) -> TokenStream {
    let ident = parse_macro_input!(attr as Ident);
    let mut input = parse_macro_input!(input as ItemImpl);
    let size = unsafe { get_sizeof_window() } as usize;
    let gen_new = quote! {
        pub fn new() -> Self {
            Self {
                disp: std::ptr::null(),
                win: [0u8; #size],
                desktop_id: -2,
                title: [0; 4096usize],
                should_activate: 0
            }
        }
    };
    input.items.push(syn::ImplItem::Verbatim(gen_new));
    let gen = quote! {
        #[repr(C)]
        #[derive(Copy, Clone)]
        pub struct #ident {
            disp: *const ::std::os::raw::c_void,
            win: [u8; #size],
            desktop_id: ::std::os::raw::c_long,
            title: [::std::os::raw::c_char; 4096usize],
            should_activate: ::std::os::raw::c_int,
        }
        unsafe impl Send for #ident {}
        #input
    };
    gen.into()
}
