//! Proc-macro support for `rvtest`.
//!
//! Provides `#[describe]` / `#[it]` / `#[tag]` / `#[timeout]` / `#[retries]`
//! attribute macros that let you write BDD specs without the builder boilerplate:
//!
//! ```ignore
//! use rvtest::*;
//!
//! #[describe("Calculator")]
//! mod calc {
//!     #[it("adds two numbers")]
//!     fn adds() {
//!         assert_eq!(2 + 2, 4);
//!     }
//!
//!     #[it("subtracts")]
//!     #[tag("arithmetic")]
//!     fn subtracts() {
//!         assert_eq!(5 - 3, 2);
//!     }
//!
//!     #[describe("advanced")]
//!     mod adv {
//!         #[it("multiplies")]
//!         fn mult() {
//!             assert_eq!(2 * 3, 6);
//!         }
//!     }
//! }
//! ```

use proc_macro::TokenStream;
use quote::quote;
use syn::{
    parse_macro_input, Attribute, Expr, Ident, Item, ItemFn, ItemMod, Lit, LitStr, Meta,
};

// ---------------------------------------------------------------------------
// No-op marker attributes
// ---------------------------------------------------------------------------

/// Marks a function as a test case inside a `#[describe]` block.
#[proc_macro_attribute]
pub fn it(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

/// Attaches a tag to the nearest enclosing spec or test.  Repeatable.
#[proc_macro_attribute]
pub fn tag(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

/// Sets a timeout on the nearest enclosing spec or test.
#[proc_macro_attribute]
pub fn timeout(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

/// Sets a retry count on the nearest enclosing spec or test.
#[proc_macro_attribute]
pub fn retries(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

/// Registers a `before_all` hook on the enclosing spec block.
#[proc_macro_attribute]
pub fn before_all(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

/// Registers an `after_all` hook on the enclosing spec block.
#[proc_macro_attribute]
pub fn after_all(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

// ---------------------------------------------------------------------------
// #[describe] — the main entry point
// ---------------------------------------------------------------------------

/// Transforms a `mod` block into a `#[test]` function that builds and runs a
/// spec hierarchy using the `rvtest` BDD API.
#[proc_macro_attribute]
pub fn describe(attr: TokenStream, item: TokenStream) -> TokenStream {
    let description = parse_macro_input!(attr as LitStr);
    let module = parse_macro_input!(item as ItemMod);

    let (chain, _) = build_chain(&module);
    let test_fn_name = &module.ident;

    // Preserve any non-macro items (use, static, const, fn without #[it], etc.)
    let preserved = preserve_other_items(&module);

    let output = quote! {
        #preserved

        #[test]
        fn #test_fn_name() {
            rvtest::spec::describe(#description)
                #chain
                .run()
                .assert_all_pass();
        }
    };

    output.into()
}

// ---------------------------------------------------------------------------
// Spec builder chain (recursive)
// ---------------------------------------------------------------------------

/// Returns the chain of builder calls (`.it()`, `.tag()`, `.describe()`, etc.)
/// *after* the initial `describe("name")` and *before* `.run().assert_all_pass()`.
///
/// For nested `#[describe]` blocks, the chain starts with `.describe("name")`
/// and includes all inner items recursively.
fn build_chain(module: &ItemMod) -> (proc_macro2::TokenStream, bool) {
    let items = module
        .content
        .as_ref()
        .map_or(&[] as &[syn::Item], |(_, items)| items);

    let mut chain = proc_macro2::TokenStream::new();
    let mut has_children = false;

    for item in items {
        match item {
            // ---- #[it("name")] fn name() { body } ----
            Item::Fn(func) if has_attr(func, "it") => {
                let test_name = extract_str_attr(func, "it")
                    .unwrap_or_else(|| func.sig.ident.to_string());
                let body = &func.block;

                chain.extend(quote! {
                    .it(#test_name, || #body)
                });
                apply_metadata_attrs(&func.attrs, &mut chain);
                has_children = true;
            }

            // ---- #[describe("name")] mod name { ... } (recursive) ----
            Item::Mod(sub) if has_attr(sub, "describe") => {
                let sub_desc = extract_str_attr(sub, "describe")
                    .unwrap_or_else(|| sub.ident.to_string());
                let (sub_chain, sub_has) = build_chain(sub);

                if sub_has {
                    chain.extend(quote! {
                        .describe(#sub_desc)
                            #sub_chain
                    });
                    apply_metadata_attrs(&sub.attrs, &mut chain);
                    has_children = true;
                }
            }

            _ => {}
        }
    }

    apply_metadata_attrs(&module.attrs, &mut chain);

    (chain, has_children)
}

// ---------------------------------------------------------------------------
// Preserving non-macro items
// ---------------------------------------------------------------------------

/// Collect any items that are NOT consumed by `#[describe]` / `#[it]` etc.
/// so they are preserved in the output (e.g. `use`, `static`, regular `fn`).
fn preserve_other_items(module: &ItemMod) -> proc_macro2::TokenStream {
    let items = match module.content.as_ref() {
        Some((_, items)) => items,
        None => return proc_macro2::TokenStream::new(),
    };

    let mut out = proc_macro2::TokenStream::new();
    for item in items {
        let keep = match item {
            // Skip items consumed by the macro
            Item::Fn(f) if has_attr(f, "it") => false,
            Item::Mod(m) if has_attr(m, "describe") => false,
            // Keep everything else
            _ => true,
        };
        if keep {
            out.extend(quote! { #item });
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Attribute extraction helpers
// ---------------------------------------------------------------------------

fn has_attr(item: &impl HasAttrs, name: &str) -> bool {
    item.get_attrs()
        .iter()
        .any(|a| a.path().is_ident(name))
}

fn extract_str_attr(item: &impl HasAttrs, name: &str) -> Option<String> {
    for attr in item.get_attrs() {
        if attr.path().is_ident(name) {
            if let Meta::NameValue(nv) = &attr.meta {
                if let Expr::Lit(expr_lit) = &nv.value {
                    if let Lit::Str(s) = &expr_lit.lit {
                        return Some(s.value());
                    }
                }
            }
        }
    }
    None
}

fn extract_expr_attr(item: &impl HasAttrs, name: &str) -> Option<proc_macro2::TokenStream> {
    for attr in item.get_attrs() {
        if attr.path().is_ident(name) {
            if let Meta::List(list) = &attr.meta {
                return Some(list.tokens.clone());
            }
        }
    }
    None
}

fn collect_tag_values(item: &impl HasAttrs) -> Vec<String> {
    let mut tags = Vec::new();
    for attr in item.get_attrs() {
        if attr.path().is_ident("tag") {
            if let Meta::NameValue(nv) = &attr.meta {
                if let Expr::Lit(expr_lit) = &nv.value {
                    if let Lit::Str(s) = &expr_lit.lit {
                        tags.push(s.value());
                    }
                }
            }
        }
    }
    tags
}

fn apply_metadata_attrs(attrs: &[Attribute], chain: &mut proc_macro2::TokenStream) {
    for tag_val in collect_tag_values(&AttrHolder(attrs)) {
        chain.extend(quote! { .tag(#tag_val) });
    }
    if let Some(to) = extract_expr_attr(&AttrHolder(attrs), "timeout") {
        chain.extend(quote! { .timeout(#to) });
    }
    if let Some(r) = extract_expr_attr(&AttrHolder(attrs), "retries") {
        chain.extend(quote! { .retries(#r) });
    }
    for name in &["before_all", "after_all"] {
        if let Some(hook) = extract_expr_attr(&AttrHolder(attrs), name) {
            let hook_ident = Ident::new(name, proc_macro2::Span::call_site());
            chain.extend(quote! { .#hook_ident(#hook) });
        }
    }
}

// ---------------------------------------------------------------------------
// Helper trait for unified attribute access
// ---------------------------------------------------------------------------

trait HasAttrs {
    fn get_attrs(&self) -> &[Attribute];
}

impl HasAttrs for ItemFn {
    fn get_attrs(&self) -> &[Attribute] {
        &self.attrs
    }
}

impl HasAttrs for ItemMod {
    fn get_attrs(&self) -> &[Attribute] {
        &self.attrs
    }
}

struct AttrHolder<'a>(&'a [Attribute]);

impl HasAttrs for AttrHolder<'_> {
    fn get_attrs(&self) -> &[Attribute] {
        self.0
    }
}
