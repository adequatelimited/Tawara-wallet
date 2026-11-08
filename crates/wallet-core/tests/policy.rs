// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! The repository's standing rules, as checks that run with `cargo test`.
//!
//! Each check reads the tree as committed -- manifests, the lockfile, cargo's
//! own configuration files in the repository, the sources -- and states in
//! its failure message which rule it holds. None of them reaches the network.
//!
//! - The wallet library is pinned by full commit hash and never patched,
//!   replaced, path-overridden, vendored or forked.
//! - The release profile keeps `panic = "unwind"` and overflow checks, which
//!   the library's key scrubs depend on and which a dependent has to restate
//!   because cargo reads profiles only from the root of the workspace being
//!   built.
//! - `Kdf::CHEAP_FOR_TESTS` never appears outside test code.
//! - `wallet-core` has no interface dependency, and the interface crates
//!   never depend on or name the library.
//! - The third-party notices accept the licences deny.toml admits and the
//!   named exceptions, and the committed file ends with the Mochimo agreement
//!   and holds no licence template's placeholder.
//!
//! What no check here can see: cargo configuration outside the repository
//! (`$CARGO_HOME/config.toml`), and flags passed through the environment
//! (`RUSTFLAGS`, `CARGO_PROFILE_*`). CI sets neither.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use quote::ToTokens;
use syn::ext::IdentExt;
use syn::visit_mut::VisitMut;

/// The one source the library may come from.
const LIBRARY_GIT: &str = "https://github.com/adequatelimited/mcm-rust-cli-windows";

/// The library's package name.
const LIBRARY: &str = "mochimo-crypto";

/// The fork of iced with mobile support: the one source iced may come from,
/// and the one source of the packages the fork ships with, winit and dpi.
const ICED_GIT: &str = "https://github.com/patricksmithlaravel/iced_mobile";

/// The features the `iced` line in the workspace manifest names, in any order.
/// Beyond iced 0.14.0's defaults: `canvas` and `advanced` for custom widgets,
/// `qr_code` for the Receive code, and `image-without-codecs` for the Android
/// scanner's preview (`image` would bring codecs under a licence deny.toml
/// does not admit). Each feature turns on code, crates or a platform backend
/// in every build, so the list is held exactly.
const ICED_FEATURES: [&str; 11] = [
    "wgpu",
    "tiny-skia",
    "canvas",
    "advanced",
    "qr_code",
    "image-without-codecs",
    "crisp",
    "web-colors",
    "thread-pool",
    "x11",
    "wayland",
];

/// The library's crate name, as source code names it.
const LIBRARY_IDENT: &str = "mochimo_crypto";

/// The library's test-only key-derivation parameters.
const CHEAP_KDF: &str = "CHEAP_FOR_TESTS";

/// The crates that make up the interface. They talk to `wallet-core` and never
/// to the library.
const INTERFACE_CRATES: [&str; 2] = ["app", "desktop"];

/// Package names that would put an interface toolkit, a windowing layer, a
/// renderer or the interface itself into `wallet-core`'s dependency closure.
const INTERFACE_DEPENDENCY_PREFIXES: [&str; 10] = [
    "iced",
    "winit",
    "wgpu",
    "tiny-skia",
    "softbuffer",
    "android-activity",
    "ndk",
    "tawara-app",
    "tawara-desktop",
    "tawara-mobile",
];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("the workspace root is two levels above crates/wallet-core")
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

fn read_toml(path: &Path) -> toml::Table {
    read(path)
        .parse::<toml::Table>()
        .unwrap_or_else(|e| panic!("{} is not valid TOML: {e}", path.display()))
}

/// Whether `dir` is a cargo build directory: a `target` directory beside a
/// manifest. Only those are skipped; a source directory that happens to be
/// called `target` is walked like any other.
fn is_build_output(dir: &Path) -> bool {
    dir.file_name().is_some_and(|n| n == "target")
        && dir.parent().is_some_and(|p| p.join("Cargo.toml").exists())
}

/// Every file under `dir` for which `keep` holds, skipping build output and
/// git's own directory.
fn files_under(dir: &Path, keep: &dyn Fn(&Path) -> bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let entries =
            fs::read_dir(&d).unwrap_or_else(|e| panic!("cannot list {}: {e}", d.display()));
        for entry in entries {
            let path = entry.expect("directory entry").path();
            if path.is_dir() {
                if is_build_output(&path) || path.file_name().is_some_and(|n| n == ".git") {
                    continue;
                }
                stack.push(path);
            } else if keep(&path) {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

fn rust_files(dir: &Path) -> Vec<PathBuf> {
    files_under(dir, &|p| p.extension().is_some_and(|e| e == "rs"))
}

/// Every crate directory under `crates/`.
fn crate_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(workspace_root().join("crates"))
        .expect("crates/ exists")
        .map(|e| e.expect("directory entry").path())
        .filter(|p| p.join("Cargo.toml").exists())
        .collect();
    dirs.sort();
    dirs
}

/// Every cargo configuration file inside the repository.
fn cargo_configs() -> Vec<PathBuf> {
    files_under(&workspace_root(), &|p| {
        p.parent()
            .and_then(|d| d.file_name())
            .is_some_and(|d| d == ".cargo")
            && p.file_name()
                .is_some_and(|n| n == "config" || n == "config.toml")
    })
}

// ---------------------------------------------------------------------------
// Test-only code, as rustc decides it.
// ---------------------------------------------------------------------------

/// Whether an attribute marks what it sits on as compiled only for tests:
/// `#[test]`, `#[cfg(test)]`, or `#[cfg(all(test, ...))]`.
/// `#[cfg(any(test, ...))]` is NOT test-only, since its other arms compile
/// the item into ordinary builds; neither is `#[cfg(not(test))]`.
fn is_test_only(attr: &syn::Attribute) -> bool {
    if attr.path().is_ident("test") {
        return true;
    }
    if !attr.path().is_ident("cfg") {
        return false;
    }
    let Ok(meta) = attr.parse_args::<syn::Meta>() else {
        return false;
    };
    match meta {
        syn::Meta::Path(p) => p.is_ident("test"),
        syn::Meta::List(list) if list.path.is_ident("all") => list
            .parse_args_with(
                syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
            )
            .map(|args| {
                args.iter()
                    .any(|m| matches!(m, syn::Meta::Path(p) if p.is_ident("test")))
            })
            .unwrap_or(false),
        _ => false,
    }
}

fn test_only(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(is_test_only)
}

fn item_attrs(item: &syn::Item) -> &[syn::Attribute] {
    match item {
        syn::Item::Const(i) => &i.attrs,
        syn::Item::Enum(i) => &i.attrs,
        syn::Item::ExternCrate(i) => &i.attrs,
        syn::Item::Fn(i) => &i.attrs,
        syn::Item::ForeignMod(i) => &i.attrs,
        syn::Item::Impl(i) => &i.attrs,
        syn::Item::Macro(i) => &i.attrs,
        syn::Item::Mod(i) => &i.attrs,
        syn::Item::Static(i) => &i.attrs,
        syn::Item::Struct(i) => &i.attrs,
        syn::Item::Trait(i) => &i.attrs,
        syn::Item::TraitAlias(i) => &i.attrs,
        syn::Item::Type(i) => &i.attrs,
        syn::Item::Union(i) => &i.attrs,
        syn::Item::Use(i) => &i.attrs,
        _ => &[],
    }
}

fn expr_attrs(expr: &syn::Expr) -> &[syn::Attribute] {
    use syn::Expr::*;
    match expr {
        Array(e) => &e.attrs,
        Assign(e) => &e.attrs,
        Async(e) => &e.attrs,
        Await(e) => &e.attrs,
        Binary(e) => &e.attrs,
        Block(e) => &e.attrs,
        Break(e) => &e.attrs,
        Call(e) => &e.attrs,
        Cast(e) => &e.attrs,
        Closure(e) => &e.attrs,
        Const(e) => &e.attrs,
        Continue(e) => &e.attrs,
        Field(e) => &e.attrs,
        ForLoop(e) => &e.attrs,
        Group(e) => &e.attrs,
        If(e) => &e.attrs,
        Index(e) => &e.attrs,
        Infer(e) => &e.attrs,
        Let(e) => &e.attrs,
        Lit(e) => &e.attrs,
        Loop(e) => &e.attrs,
        Macro(e) => &e.attrs,
        Match(e) => &e.attrs,
        MethodCall(e) => &e.attrs,
        Paren(e) => &e.attrs,
        Path(e) => &e.attrs,
        Range(e) => &e.attrs,
        RawAddr(e) => &e.attrs,
        Reference(e) => &e.attrs,
        Repeat(e) => &e.attrs,
        Return(e) => &e.attrs,
        Struct(e) => &e.attrs,
        Try(e) => &e.attrs,
        TryBlock(e) => &e.attrs,
        Tuple(e) => &e.attrs,
        Unary(e) => &e.attrs,
        Unsafe(e) => &e.attrs,
        While(e) => &e.attrs,
        Yield(e) => &e.attrs,
        _ => &[],
    }
}

/// Removes everything a non-test build does not compile: a file whose own
/// inner attributes make it test-only, and every item, impl item, trait
/// item, statement, match arm, struct field and enum variant marked
/// test-only. What is left is what a non-test build compiles.
struct StripTestCode;

impl VisitMut for StripTestCode {
    fn visit_file_mut(&mut self, file: &mut syn::File) {
        if test_only(&file.attrs) {
            file.items.clear();
            return;
        }
        file.items.retain(|item| !test_only(item_attrs(item)));
        syn::visit_mut::visit_file_mut(self, file);
    }

    fn visit_item_mod_mut(&mut self, module: &mut syn::ItemMod) {
        if let Some((_, items)) = &mut module.content {
            items.retain(|item| !test_only(item_attrs(item)));
        }
        syn::visit_mut::visit_item_mod_mut(self, module);
    }

    fn visit_item_impl_mut(&mut self, imp: &mut syn::ItemImpl) {
        imp.items.retain(|item| {
            !test_only(match item {
                syn::ImplItem::Const(i) => &i.attrs,
                syn::ImplItem::Fn(i) => &i.attrs,
                syn::ImplItem::Type(i) => &i.attrs,
                syn::ImplItem::Macro(i) => &i.attrs,
                _ => &[],
            })
        });
        syn::visit_mut::visit_item_impl_mut(self, imp);
    }

    fn visit_item_trait_mut(&mut self, tr: &mut syn::ItemTrait) {
        tr.items.retain(|item| {
            !test_only(match item {
                syn::TraitItem::Const(i) => &i.attrs,
                syn::TraitItem::Fn(i) => &i.attrs,
                syn::TraitItem::Type(i) => &i.attrs,
                syn::TraitItem::Macro(i) => &i.attrs,
                _ => &[],
            })
        });
        syn::visit_mut::visit_item_trait_mut(self, tr);
    }

    fn visit_block_mut(&mut self, block: &mut syn::Block) {
        block.stmts.retain(|stmt| match stmt {
            syn::Stmt::Item(item) => !test_only(item_attrs(item)),
            syn::Stmt::Local(local) => !test_only(&local.attrs),
            syn::Stmt::Macro(mac) => !test_only(&mac.attrs),
            syn::Stmt::Expr(expr, _) => !test_only(expr_attrs(expr)),
        });
        syn::visit_mut::visit_block_mut(self, block);
    }

    fn visit_expr_match_mut(&mut self, m: &mut syn::ExprMatch) {
        m.arms.retain(|arm| !test_only(&arm.attrs));
        syn::visit_mut::visit_expr_match_mut(self, m);
    }

    fn visit_expr_struct_mut(&mut self, s: &mut syn::ExprStruct) {
        s.fields = std::mem::take(&mut s.fields)
            .into_pairs()
            .filter(|pair| !test_only(&pair.value().attrs))
            .collect();
        syn::visit_mut::visit_expr_struct_mut(self, s);
    }

    fn visit_fields_named_mut(&mut self, fields: &mut syn::FieldsNamed) {
        fields.named = std::mem::take(&mut fields.named)
            .into_pairs()
            .filter(|pair| !test_only(&pair.value().attrs))
            .collect();
        syn::visit_mut::visit_fields_named_mut(self, fields);
    }

    fn visit_item_enum_mut(&mut self, e: &mut syn::ItemEnum) {
        e.variants = std::mem::take(&mut e.variants)
            .into_pairs()
            .filter(|pair| !test_only(&pair.value().attrs))
            .collect();
        syn::visit_mut::visit_item_enum_mut(self, e);
    }
}

/// Every identifier in a token stream, including those inside macro
/// invocations, whose bodies `syn` keeps as raw tokens. Raw identifiers are
/// compared without their `r#`, so `r#CHEAP_FOR_TESTS` is `CHEAP_FOR_TESTS`.
fn idents(tokens: proc_macro2::TokenStream, out: &mut Vec<String>) {
    for tt in tokens {
        match tt {
            proc_macro2::TokenTree::Ident(i) => out.push(i.unraw().to_string()),
            proc_macro2::TokenTree::Group(g) => idents(g.stream(), out),
            _ => {}
        }
    }
}

/// A source file with what only test builds compile removed.
fn non_test_file(source: &str) -> Result<syn::File, syn::Error> {
    let mut file = syn::parse_file(source)?;
    StripTestCode.visit_file_mut(&mut file);
    Ok(file)
}

fn names_cheap_kdf_outside_tests(source: &str) -> Result<bool, syn::Error> {
    let mut found = Vec::new();
    idents(non_test_file(source)?.into_token_stream(), &mut found);
    Ok(found.iter().any(|i| i == CHEAP_KDF))
}

// ---------------------------------------------------------------------------
// The source a non-test build compiles, and what is provably test-only.
// ---------------------------------------------------------------------------

/// A module context: where the `mod name;` declarations of a source file
/// resolve. A crate root, a `mod.rs`, a `#[path]` file or an `include!`d file
/// resolves them beside itself; any other file in a directory named after it.
/// For `include!`, rustc makes the included file's directory the module
/// directory, whatever module the `include!` sits in: measured on rustc
/// 1.98.0, `src/nested/inc.rs` included from `src/lib.rs` loads
/// `#[path = "x.rs"]` as `src/nested/x.rs` and `mod b;` as `src/nested/b.rs`,
/// and a file included from `src/foo.rs`, or from inside `mod m { .. }`,
/// loads `mod c;` as `src/c.rs`.
#[derive(Clone)]
struct ModFile {
    path: PathBuf,
    mod_rs: bool,
}

/// A source file to read, and the module context its declarations resolve in.
#[derive(Clone)]
struct Source {
    path: PathBuf,
    context: ModFile,
}

/// Whether a `cfg` predicate holds only in test builds: `test`, or
/// `all(test, ..)`. `any(test, ..)` and `not(test)` do not.
fn predicate_is_test_only(meta: &syn::Meta) -> bool {
    match meta {
        syn::Meta::Path(p) => p.is_ident("test"),
        syn::Meta::List(list) if list.path.is_ident("all") => list
            .parse_args_with(
                syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
            )
            .map(|args| {
                args.iter()
                    .any(|m| matches!(m, syn::Meta::Path(p) if p.is_ident("test")))
            })
            .unwrap_or(false),
        _ => false,
    }
}

/// One file a module declaration's attributes can select, and whether only
/// test builds select it.
#[derive(Debug)]
struct PathChoice {
    path: String,
    test_only: bool,
}

fn path_value(meta: &syn::Meta) -> Result<Option<String>, String> {
    match meta {
        syn::Meta::NameValue(nv) if nv.path.is_ident("path") => match &nv.value {
            syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(s),
                ..
            }) => Ok(Some(s.value())),
            _ => Err("a `path` attribute whose value is not a string literal".into()),
        },
        _ => Ok(None),
    }
}

fn cfg_attr_choices(
    attr_args: &syn::MetaList,
    enclosing_test: bool,
    out: &mut Vec<PathChoice>,
) -> Result<(), String> {
    let args = attr_args
        .parse_args_with(syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated)
        .map_err(|e| format!("a `cfg_attr` this check cannot read: {e}"))?;
    let mut args = args.into_iter();
    let predicate = args.next().ok_or("a `cfg_attr` with no predicate")?;
    let test_only = enclosing_test || predicate_is_test_only(&predicate);
    for meta in args {
        if let Some(path) = path_value(&meta)? {
            out.push(PathChoice { path, test_only });
        } else if let syn::Meta::List(inner) = &meta
            && inner.path.is_ident("cfg_attr")
        {
            cfg_attr_choices(inner, test_only, out)?;
        }
    }
    Ok(())
}

/// Every file a declaration's attributes can select: a direct `#[path]`, and
/// the `path` of every `#[cfg_attr(..)]`, nested ones included. rustc picks
/// one of them for each configuration; this check takes all of them.
fn path_choices(attrs: &[syn::Attribute]) -> Result<Vec<PathChoice>, String> {
    let mut out = Vec::new();
    for attr in attrs {
        if attr.path().is_ident("path") {
            if let Some(path) = path_value(&attr.meta)? {
                out.push(PathChoice {
                    path,
                    test_only: false,
                });
            } else {
                return Err("a `#[path]` attribute that is not `path = \"...\"`".into());
            }
        } else if attr.path().is_ident("cfg_attr") {
            let syn::Meta::List(list) = &attr.meta else {
                return Err("a `cfg_attr` that is not a list".into());
            };
            cfg_attr_choices(list, false, &mut out)?;
        }
    }
    Ok(out)
}

/// An out-of-line `mod name;` declaration, with the inline modules it sits
/// in, its attributes, and whether it sits in test-only code (its own
/// attributes, or an enclosing inline module's).
struct ModDecl {
    inline: Vec<String>,
    name: String,
    attrs: Vec<syn::Attribute>,
    test_only: bool,
}

fn mod_decls(
    items: &[syn::Item],
    inline: &[String],
    enclosing_test: bool,
    out: &mut Vec<ModDecl>,
) -> Result<(), String> {
    for item in items {
        let syn::Item::Mod(m) = item else { continue };
        let test_only = enclosing_test || test_only(&m.attrs);
        match &m.content {
            None => out.push(ModDecl {
                inline: inline.to_vec(),
                name: m.ident.unraw().to_string(),
                attrs: m.attrs.clone(),
                test_only,
            }),
            Some((_, inner)) => {
                let choices = path_choices(&m.attrs)?;
                if choices.iter().any(|c| !c.test_only) && choices.len() > 1
                    || m.attrs.iter().any(|a| a.path().is_ident("cfg_attr")) && !choices.is_empty()
                {
                    return Err(format!(
                        "inline module `{}` takes a conditional or repeated path, which this \
                         check does not model",
                        m.ident
                    ));
                }
                let segment = choices
                    .first()
                    .map(|c| c.path.clone())
                    .unwrap_or_else(|| m.ident.unraw().to_string());
                let mut nested = inline.to_vec();
                nested.push(segment);
                mod_decls(inner, &nested, test_only, out)?;
            }
        }
    }
    Ok(())
}

/// Every `include!(..)` invocation in a syntax tree: `Some(path)` for a
/// string literal, `None` for anything else (`concat!`, `env!`, a macro
/// variable), which this check cannot resolve.
#[derive(Default)]
struct Includes(Vec<Option<String>>);

impl<'ast> syn::visit::Visit<'ast> for Includes {
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        if mac
            .path
            .segments
            .last()
            .is_some_and(|s| s.ident.unraw() == "include")
        {
            self.0.push(
                syn::parse2::<syn::LitStr>(mac.tokens.clone())
                    .ok()
                    .map(|l| l.value()),
            );
        }
        syn::visit::visit_macro(self, mac);
    }
}

/// A source file's content as rustc may meet it: a file of items, or (for an
/// `include!`d file) a single expression.
enum Parsed {
    File(syn::File),
    Expr(syn::Expr),
}

fn parse_source(path: &Path) -> Result<Parsed, String> {
    let text = read(path);
    match syn::parse_file(&text) {
        Ok(f) => Ok(Parsed::File(f)),
        Err(file_err) => syn::parse_str::<syn::Expr>(&text)
            .map(Parsed::Expr)
            .map_err(|_| {
                format!(
                    "{} does not parse, so this check cannot say what it compiles: {file_err}",
                    path.display()
                )
            }),
    }
}

fn includes_in(parsed: &Parsed) -> Vec<Option<String>> {
    use syn::visit::Visit;
    let mut found = Includes::default();
    match parsed {
        Parsed::File(f) => found.visit_file(f),
        Parsed::Expr(e) => found.visit_expr(e),
    }
    found.0
}

/// Routes into source that the walk does not follow, in code a non-test
/// build compiles. rustc accepts an out-of-line `mod` inside a block (a
/// function body, a `const` initialiser) when it carries a `#[path]`, and the
/// walk reads `mod` only at item level; a macro's tokens are raw to `syn`, so a
/// `mod` or `include!` it expands to is not seen; `use std::include as load;`
/// gives `include!` a name the walk does not know; and a macro from a
/// dependency can expand to `include!` of a path it is given. Each of these is
/// refused rather than modelled, which keeps the walk's set of production
/// files complete, and the exemption of test-only files sound.
///
/// The crate's own code cannot reach `include!` by another route. A macro
/// defined in a dependency can build any path it likes, so for those the walk
/// refuses only what a hand-off looks like: a string, as rustc reads it, that
/// may name a `.rs` file. Dependencies themselves are reviewed through
/// `Cargo.lock` and `deny.toml`.
#[derive(Default)]
struct Unfollowed {
    block_depth: usize,
    found: Vec<String>,
}

impl<'ast> syn::visit::Visit<'ast> for Unfollowed {
    fn visit_block(&mut self, block: &'ast syn::Block) {
        self.block_depth += 1;
        syn::visit::visit_block(self, block);
        self.block_depth -= 1;
    }

    fn visit_item_mod(&mut self, m: &'ast syn::ItemMod) {
        if self.block_depth > 0 && m.content.is_none() {
            self.found.push(format!(
                "an out-of-line `mod {};` inside a block",
                m.ident.unraw()
            ));
        }
        syn::visit::visit_item_mod(self, m);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        let is_include = mac
            .path
            .segments
            .last()
            .is_some_and(|s| s.ident.unraw() == "include");
        if !is_include {
            let name = mac.path.to_token_stream().to_string().replace(' ', "");
            let mut words = Vec::new();
            idents(mac.tokens.clone(), &mut words);
            if words.iter().any(|w| w == "mod" || w == "include") {
                self.found.push(format!(
                    "a `{name}!` whose tokens declare a module or use `include!`"
                ));
            } else if names_rust_file(mac.tokens.clone()) {
                self.found.push(format!(
                    "a `{name}!` given a string that may name a `.rs` file"
                ));
            }
        }
        syn::visit::visit_macro(self, mac);
    }

    fn visit_use_rename(&mut self, rename: &'ast syn::UseRename) {
        if rename.ident.unraw() == "include" {
            self.found
                .push(format!("`include` imported as `{}`", rename.rename.unraw()));
        }
        syn::visit::visit_use_rename(self, rename);
    }
}

/// Whether a token stream holds a string that may name a `.rs` file, judged
/// by its value as rustc reads it, not its spelling: a literal whose value
/// ends in `.rs` once escapes are decoded (`"shared.r\x73"` is `shared.rs`),
/// a `concat!` or `stringify!` whose value does, or a `concat!` with a part
/// this check cannot evaluate (an `env!`, a macro variable), since `concat!`
/// is how a path is put together from pieces. The suffix is compared without
/// case: on macOS and Windows `shared.RS` opens `shared.rs`. A bare `env!` is
/// not refused: its value is set outside the source, like any path a
/// dependency's macro builds for itself, and refusing it would refuse every
/// version string.
fn names_rust_file(tokens: proc_macro2::TokenStream) -> bool {
    let is_rust_file = |v: &str| v.to_ascii_lowercase().ends_with(".rs");
    let tokens: Vec<proc_macro2::TokenTree> = tokens.into_iter().collect();
    tokens.iter().enumerate().any(|(i, tt)| match tt {
        proc_macro2::TokenTree::Literal(lit) => literal_text(lit).is_some_and(|t| is_rust_file(&t)),
        proc_macro2::TokenTree::Group(g) => names_rust_file(g.stream()),
        proc_macro2::TokenTree::Ident(_) => {
            string_macro(&tokens[i..]).is_some_and(|value| value.is_none_or(|v| is_rust_file(&v)))
        }
        proc_macro2::TokenTree::Punct(_) => false,
    })
}

/// A literal's value as `concat!` would write it, with escapes decoded.
fn literal_text(lit: &proc_macro2::Literal) -> Option<String> {
    match syn::Lit::new(lit.clone()) {
        syn::Lit::Str(s) => Some(s.value()),
        syn::Lit::ByteStr(s) => Some(String::from_utf8_lossy(&s.value()).into_owned()),
        syn::Lit::CStr(s) => Some(s.value().to_string_lossy().into_owned()),
        syn::Lit::Char(c) => Some(c.value().to_string()),
        syn::Lit::Int(n) => Some(n.base10_digits().to_string()),
        syn::Lit::Float(n) => Some(n.base10_digits().to_string()),
        _ => None,
    }
}

/// For tokens that start with `concat!(..)` or `stringify!(..)`: `Some` of
/// the string it expands to, where this check can evaluate it.
fn string_macro(tokens: &[proc_macro2::TokenTree]) -> Option<Option<String>> {
    use proc_macro2::TokenTree::{Group, Ident, Punct};
    let [Ident(name), Punct(bang), Group(args), ..] = tokens else {
        return None;
    };
    if bang.as_char() != '!' {
        return None;
    }
    match name.unraw().to_string().as_str() {
        // rustc's stringify! spaces tokens its own way; `.rs` at the end
        // survives any spacing once whitespace is dropped.
        "stringify" => Some(Some(args.stream().to_string().split_whitespace().collect())),
        "concat" => Some(concat_value(args.stream())),
        _ => None,
    }
}

/// `concat!`'s value: each comma-separated part is a literal, a negative
/// number, `true` or `false`, or a nested `concat!` or `stringify!`. Anything
/// else is `None`.
fn concat_value(args: proc_macro2::TokenStream) -> Option<String> {
    use proc_macro2::TokenTree::{Ident, Literal, Punct};
    let tokens: Vec<proc_macro2::TokenTree> = args.into_iter().collect();
    let mut value = String::new();
    for part in tokens.split(|tt| matches!(tt, Punct(p) if p.as_char() == ',')) {
        match part {
            [] => {}
            [Literal(lit)] => value.push_str(&literal_text(lit)?),
            [Punct(minus), Literal(lit)] if minus.as_char() == '-' => {
                value.push('-');
                value.push_str(&literal_text(lit)?);
            }
            [Ident(b)] if *b == "true" || *b == "false" => value.push_str(&b.to_string()),
            [_, _, _] => value.push_str(&string_macro(part)??),
            _ => return None,
        }
    }
    Some(value)
}

fn unfollowed_in(parsed: &Parsed) -> Vec<String> {
    use syn::visit::Visit;
    let mut found = Unfollowed::default();
    match parsed {
        Parsed::File(f) => found.visit_file(f),
        Parsed::Expr(e) => found.visit_expr(e),
    }
    found.found
}

/// Out-of-line `mod` declarations inside blocks, with the inline modules
/// (outside any block) they sit in and their attributes, where the walk can
/// resolve them: directly in a block, not under an inline module inside one
/// (rustc resolves those by other rules). Once `Unfollowed` has refused them in
/// non-test code, only test code holds them, and a test fixture loaded this way
/// is then exempt.
#[derive(Default)]
struct BlockMods {
    block_depth: usize,
    inline_in_block: usize,
    inline: Vec<String>,
    found: Vec<(Vec<String>, Vec<syn::Attribute>)>,
}

impl<'ast> syn::visit::Visit<'ast> for BlockMods {
    fn visit_block(&mut self, block: &'ast syn::Block) {
        self.block_depth += 1;
        syn::visit::visit_block(self, block);
        self.block_depth -= 1;
    }

    fn visit_item_mod(&mut self, m: &'ast syn::ItemMod) {
        match (&m.content, self.block_depth > 0) {
            (None, true) if self.inline_in_block == 0 => {
                self.found.push((self.inline.clone(), m.attrs.clone()));
            }
            (None, _) => {}
            (Some(_), true) => {
                self.inline_in_block += 1;
                syn::visit::visit_item_mod(self, m);
                self.inline_in_block -= 1;
            }
            (Some(_), false) => {
                let segment = path_choices(&m.attrs)
                    .ok()
                    .and_then(|c| c.into_iter().next())
                    .map(|c| c.path)
                    .unwrap_or_else(|| m.ident.unraw().to_string());
                self.inline.push(segment);
                syn::visit::visit_item_mod(self, m);
                self.inline.pop();
            }
        }
    }
}

fn block_mods_in(parsed: &Parsed) -> Vec<(Vec<String>, Vec<syn::Attribute>)> {
    use syn::visit::Visit;
    let mut found = BlockMods::default();
    match parsed {
        Parsed::File(f) => found.visit_file(f),
        Parsed::Expr(e) => found.visit_expr(e),
    }
    found.found
}

/// The files a `#[path]` on a `mod` inside a block can load: relative to the
/// enclosing module's directory, and to the declaring file's. Only test code
/// reaches this, and every production route is followed or refused, so taking
/// both cannot exempt a file a non-test build compiles.
fn block_mod_targets(
    context: &ModFile,
    here: &Path,
    inline: &[String],
    attrs: &[syn::Attribute],
) -> Result<Vec<Source>, String> {
    let (module_dir, _) = module_dirs(context, inline);
    let mut out = Vec::new();
    for choice in path_choices(attrs)? {
        for base in [module_dir.as_path(), here] {
            let file = ModFile {
                path: base.join(&choice.path),
                mod_rs: true,
            };
            out.push(Source {
                path: file.path.clone(),
                context: file,
            });
        }
    }
    Ok(out)
}

fn strip(parsed: &Parsed) -> Parsed {
    match parsed {
        Parsed::File(f) => {
            let mut f = f.clone();
            StripTestCode.visit_file_mut(&mut f);
            Parsed::File(f)
        }
        Parsed::Expr(e) => {
            let mut e = e.clone();
            StripTestCode.visit_expr_mut(&mut e);
            Parsed::Expr(e)
        }
    }
}

fn idents_of(parsed: &Parsed) -> Vec<String> {
    let mut out = Vec::new();
    match parsed {
        Parsed::File(f) => idents(f.to_token_stream(), &mut out),
        Parsed::Expr(e) => idents(e.to_token_stream(), &mut out),
    }
    out
}

fn normalise(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// The directory a declaration in `context`, inside `inline` modules,
/// resolves a default `name.rs` / `name/mod.rs` in, and a `#[path]` in.
fn module_dirs(context: &ModFile, inline: &[String]) -> (PathBuf, PathBuf) {
    let dir = context
        .path
        .parent()
        .expect("a source file has a directory");
    let module_dir = if context.mod_rs {
        dir.to_path_buf()
    } else {
        dir.join(context.path.file_stem().expect("a source file has a stem"))
    };
    let nested = inline.iter().fold(module_dir, |b, m| b.join(m));
    // Outside an inline module, `#[path]` is relative to the declaring file's
    // directory; inside one, to the inline module's directory.
    let path_base = if inline.is_empty() {
        dir.to_path_buf()
    } else {
        nested.clone()
    };
    (nested, path_base)
}

/// The file `mod name;` loads by default, if exactly one candidate exists.
fn default_mod_file(base: &Path, name: &str) -> Result<Option<ModFile>, String> {
    let flat = base.join(format!("{name}.rs"));
    let nested = base.join(name).join("mod.rs");
    match (flat.exists(), nested.exists()) {
        (true, false) => Ok(Some(ModFile {
            path: flat,
            mod_rs: false,
        })),
        (false, true) => Ok(Some(ModFile {
            path: nested,
            mod_rs: true,
        })),
        (true, true) => Err(format!(
            "both {} and {} exist",
            flat.display(),
            nested.display()
        )),
        (false, false) => Ok(None),
    }
}

/// The crate roots of every target in `crate_dir` that is not a test or a
/// benchmark: the library, every binary, the build script and every example,
/// read from the manifest where it names a path and from cargo's default
/// layout where it does not.
fn non_test_roots(crate_dir: &Path) -> Vec<PathBuf> {
    let manifest = read_toml(&crate_dir.join("Cargo.toml"));
    let mut roots = Vec::new();
    let mut push = |p: PathBuf| {
        if p.exists() && !roots.contains(&p) {
            roots.push(p);
        }
    };

    let named = |key: &str| -> Vec<PathBuf> {
        manifest
            .get(key)
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
            .filter_map(|t| t.get("path").and_then(|p| p.as_str()))
            .map(|p| crate_dir.join(p))
            .collect()
    };

    match manifest
        .get("lib")
        .and_then(|l| l.get("path"))
        .and_then(|p| p.as_str())
    {
        Some(p) => push(crate_dir.join(p)),
        None => push(crate_dir.join("src/lib.rs")),
    }
    push(crate_dir.join("src/main.rs"));
    for p in named("bin") {
        push(p);
    }
    for p in named("example") {
        push(p);
    }
    for (sub, also) in [("src/bin", "main.rs"), ("examples", "main.rs")] {
        if let Ok(entries) = fs::read_dir(crate_dir.join(sub)) {
            for entry in entries {
                let p = entry.expect("directory entry").path();
                if p.is_dir() {
                    push(p.join(also));
                } else if p.extension().is_some_and(|e| e == "rs") {
                    push(p);
                }
            }
        }
    }
    match manifest.get("package").and_then(|p| p.get("build")) {
        Some(toml::Value::String(p)) => push(crate_dir.join(p)),
        Some(toml::Value::Boolean(false)) => {}
        _ => push(crate_dir.join("build.rs")),
    }
    roots.sort();
    roots
}

/// What a crate compiles in a non-test build, and what only its tests compile.
struct SourceWalk {
    /// Every file the module tree of a non-test target reaches, through `mod`
    /// (with every `#[path]` and `cfg_attr` path) and `include!`.
    reached: BTreeSet<PathBuf>,
    /// Files reachable only through test-only declarations: these, and only
    /// these, are exempt from the scan.
    exempt: BTreeSet<PathBuf>,
    /// Every file the scan reads: all Rust files under the crate except its
    /// top-level `tests/` and `benches/`, plus every file `reached` names,
    /// minus `exempt`. A file nothing declares is scanned as production code.
    scanned: BTreeSet<PathBuf>,
}

/// The file an `include!` in a file in `here` loads: its path is relative to
/// the including file's directory, and it resolves its own declarations beside
/// itself (see `ModFile`).
fn included(here: &Path, rel: &str) -> Source {
    let path = here.join(rel);
    Source {
        context: ModFile {
            path: path.clone(),
            mod_rs: true,
        },
        path,
    }
}

/// A file and the context it is read in. One file can be reached in two
/// contexts, as a module (`mod x;`) and by `include!("x.rs")`, and rustc then
/// resolves its declarations in both: measured on rustc 1.98.0, a `#[path]`
/// inside an inline module of such a `src/x.rs` loads one file below
/// `src/x/` and another below `src/`. So the walk visits each pair once, not
/// each file once. The paths are canonical, which is sound only because
/// production code reached through a symbolic link is refused (see
/// `through_link`): without links, two spellings of one file sit in one
/// directory, and resolve alike.
fn visit_key(source: &Source) -> (PathBuf, PathBuf, bool) {
    (
        normalise(&source.path),
        normalise(&source.context.path),
        source.context.mod_rs,
    )
}

/// Whether `path` is reached through a symbolic link: the file itself, or a
/// directory on its path below `crate_dir` (what lies above the crate
/// directory is the checkout's business, not the crate's). rustc resolves the
/// declarations of a file reached through a link from the link's directory:
/// measured on rustc 1.98.0, `src/alias/common.rs`, a link to
/// `src/actual/common.rs`, loads `#[path = "shared.rs"]` as
/// `src/alias/shared.rs`, while the target loads `src/actual/shared.rs`.
fn through_link(path: &Path, crate_dir: &Path) -> bool {
    path.ancestors()
        .take_while(|p| !crate_dir.starts_with(p))
        .any(|p| fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink()))
}

fn walk_sources(crate_dir: &Path) -> Result<SourceWalk, String> {
    let mut visited = BTreeSet::new();
    let mut reached = BTreeSet::new();
    let mut test_targets: Vec<Source> = Vec::new();
    let mut stack: Vec<Source> = non_test_roots(crate_dir)
        .into_iter()
        .map(|path| Source {
            context: ModFile {
                path: path.clone(),
                mod_rs: true,
            },
            path,
        })
        .collect();

    while let Some(source) = stack.pop() {
        // Before the visit key, which canonicalises: a link and its target
        // would share a key, and the link's own context would go unread.
        if through_link(&source.path, crate_dir) {
            return Err(format!(
                "{} is reached through a symbolic link in non-test code; rustc resolves the \
                 modules it declares from the link's directory, which this check does not \
                 model. Use the file's own path, or a copy",
                source.path.display()
            ));
        }
        if !visited.insert(visit_key(&source)) {
            continue;
        }
        reached.insert(normalise(&source.path));
        let raw = parse_source(&source.path)?;
        let stripped = strip(&raw);
        let here = source.path.parent().expect("a source file has a directory");

        if let Some(what) = unfollowed_in(&stripped).first() {
            return Err(format!(
                "{} has {what} in non-test code; this check cannot see what it loads",
                source.path.display()
            ));
        }
        // What is left of `mod` inside blocks is test code: a fixture it
        // loads by `#[path]` is a test target.
        for (inline, attrs) in block_mods_in(&raw) {
            test_targets.extend(block_mod_targets(&source.context, here, &inline, &attrs)?);
        }

        // Out-of-line modules: every non-test choice is compiled in some
        // configuration; a test-only choice is a test target.
        let mut decls = Vec::new();
        if let Parsed::File(f) = &raw {
            mod_decls(&f.items, &[], false, &mut decls)?;
        }
        for decl in decls {
            let (default_base, path_base) = module_dirs(&source.context, &decl.inline);
            let choices = path_choices(&decl.attrs)?;
            let has_direct = decl.attrs.iter().any(|a| a.path().is_ident("path"));
            for choice in &choices {
                let file = ModFile {
                    path: path_base.join(&choice.path),
                    mod_rs: true,
                };
                let target = Source {
                    path: file.path.clone(),
                    context: file,
                };
                // Every file a non-test declaration can select must exist. A
                // missing one may be a configuration rustc never builds, or a
                // path this check resolves differently from rustc; it cannot
                // tell which, so it refuses rather than skips.
                if decl.test_only || choice.test_only {
                    test_targets.push(target);
                } else if target.path.exists() {
                    stack.push(target);
                } else {
                    return Err(format!(
                        "`mod {};` in {} can select {}, which does not exist; this check \
                         refuses a module file it cannot find",
                        decl.name,
                        source.path.display(),
                        target.path.display()
                    ));
                }
            }
            if !has_direct {
                match default_mod_file(&default_base, &decl.name)? {
                    Some(file) => {
                        let target = Source {
                            path: file.path.clone(),
                            context: file,
                        };
                        if decl.test_only {
                            test_targets.push(target);
                        } else {
                            stack.push(target);
                        }
                    }
                    None if !decl.test_only => {
                        return Err(format!(
                            "`mod {};` in {} resolves to neither {name}.rs nor {name}/mod.rs \
                             under {}",
                            decl.name,
                            source.path.display(),
                            default_base.display(),
                            name = decl.name
                        ));
                    }
                    None => {}
                }
            }
        }

        // `include!`: in non-test code it must be a literal this check can
        // follow; in test-only code it is a test target.
        let non_test_includes = includes_in(&stripped);
        for inc in &non_test_includes {
            let Some(rel) = inc else {
                return Err(format!(
                    "{} uses `include!` with an argument that is not a string literal; this \
                     check cannot see what it loads",
                    source.path.display()
                ));
            };
            stack.push(included(here, rel));
        }
        let mut test_includes = includes_in(&raw);
        for inc in &non_test_includes {
            if let Some(pos) = test_includes.iter().position(|t| t == inc) {
                test_includes.remove(pos);
            }
        }
        for rel in test_includes.into_iter().flatten() {
            test_targets.push(included(here, &rel));
        }
    }

    // Everything reachable from a test target, through any declaration.
    let mut test_visited = BTreeSet::new();
    let mut test_reached = BTreeSet::new();
    while let Some(source) = test_targets.pop() {
        if !source.path.exists() || !test_visited.insert(visit_key(&source)) {
            continue;
        }
        test_reached.insert(normalise(&source.path));
        let Ok(raw) = parse_source(&source.path) else {
            continue;
        };
        let here = source.path.parent().expect("a source file has a directory");
        let mut decls = Vec::new();
        if let Parsed::File(f) = &raw {
            mod_decls(&f.items, &[], true, &mut decls)?;
        }
        for decl in decls {
            let (default_base, path_base) = module_dirs(&source.context, &decl.inline);
            for choice in path_choices(&decl.attrs)? {
                let file = ModFile {
                    path: path_base.join(&choice.path),
                    mod_rs: true,
                };
                test_targets.push(Source {
                    path: file.path.clone(),
                    context: file,
                });
            }
            if let Some(file) = default_mod_file(&default_base, &decl.name)? {
                test_targets.push(Source {
                    path: file.path.clone(),
                    context: file,
                });
            }
        }
        for rel in includes_in(&raw).into_iter().flatten() {
            test_targets.push(included(here, &rel));
        }
        for (inline, attrs) in block_mods_in(&raw) {
            test_targets.extend(block_mod_targets(&source.context, here, &inline, &attrs)?);
        }
    }

    let exempt: BTreeSet<PathBuf> = test_reached.difference(&reached).cloned().collect();
    let mut scanned: BTreeSet<PathBuf> = rust_files(crate_dir)
        .into_iter()
        .filter(|p| {
            let mut parts = p
                .strip_prefix(crate_dir)
                .expect("walked from the crate directory")
                .components();
            !matches!(parts.next(), Some(c) if c.as_os_str() == "tests" || c.as_os_str() == "benches")
        })
        .map(|p| normalise(&p))
        .collect();
    scanned.extend(reached.iter().cloned());
    for e in &exempt {
        scanned.remove(e);
    }
    Ok(SourceWalk {
        reached,
        exempt,
        scanned,
    })
}

/// Every scanned file of `crate_dir`, parsed and stripped of test-only code.
fn scanned_sources(crate_dir: &Path) -> Vec<(PathBuf, Parsed)> {
    let walk = walk_sources(crate_dir).unwrap_or_else(|e| panic!("{e}"));
    walk.scanned
        .into_iter()
        .map(|path| {
            let parsed = parse_source(&path).unwrap_or_else(|e| panic!("{e}"));
            let stripped = strip(&parsed);
            (path, stripped)
        })
        .collect()
}

fn cheap_kdf_offenders(crate_dir: &Path) -> Vec<PathBuf> {
    scanned_sources(crate_dir)
        .into_iter()
        .filter(|(_, parsed)| idents_of(parsed).iter().any(|i| i == CHEAP_KDF))
        .map(|(path, _)| path)
        .collect()
}

/// `Kdf::CHEAP_FOR_TESTS` derives the store key at a cost chosen to make tests
/// fast, which is the cost an attacker with a stolen store would choose too.
/// Production code uses `Kdf::RECOMMENDED`. This holds every crate's non-test
/// code to never naming the cheap parameters.
///
/// The scan is conservative by construction. It reads every Rust file under a
/// crate except its top-level `tests/` and `benches/` (integration tests and
/// benchmarks), plus every file the module walk reaches, wherever it lives.
/// It exempts a file only when it is reachable solely through test-only
/// declarations (`#[cfg(test)] mod`, a `cfg_attr(test, path = ..)`, a module
/// nested in a test-only one, an `include!` in test-only code). The walk
/// follows `mod`, every `#[path]` and `cfg_attr` path, and literal
/// `include!`s; a non-literal `include!` in non-test code is refused, since
/// what it loads cannot be read. In every file, code under `#[cfg(test)]`,
/// `#[cfg(all(test, ..))]` or `#[test]` is removed before looking.
///
/// Only the workspace's crates under `crates/` are scanned.
#[test]
fn cheap_kdf_appears_only_in_test_code() {
    let crates_dir = workspace_root().join("crates");
    let mut reached = BTreeSet::new();
    let mut offenders = Vec::new();
    for dir in crate_dirs() {
        let walk = walk_sources(&dir).unwrap_or_else(|e| panic!("{e}"));
        reached.extend(walk.reached);
        offenders.extend(cheap_kdf_offenders(&dir));
    }

    // A walk that reached nothing would pass. Each crate's root must be in it.
    for root in [
        "wallet-core/src/lib.rs",
        "app/src/lib.rs",
        "desktop/src/main.rs",
    ] {
        let root = normalise(&crates_dir.join(root));
        assert!(
            reached.contains(&root),
            "the walk did not reach {}; a check over nothing passes vacuously",
            root.display()
        );
    }
    assert!(
        offenders.is_empty(),
        "Kdf::{CHEAP_KDF} appears outside test code in: {offenders:?}. Production code \
         derives the store key with Kdf::RECOMMENDED; the cheap parameters are for tests only."
    );
}

/// The detector above, held to cases where the answer is known, so that a
/// detector that sees nothing cannot pass the walk.
#[test]
fn the_cheap_kdf_detector_sees_what_it_must() {
    let flagged = [
        "const K: Kdf = Kdf::CHEAP_FOR_TESTS;",
        "fn f() { let k = Kdf::CHEAP_FOR_TESTS; }",
        "use mochimo_crypto::keystore::Kdf; fn f() { g(Kdf::CHEAP_FOR_TESTS) }",
        "fn f() { open!(Kdf::CHEAP_FOR_TESTS); }",
        "#[cfg(any(test, feature = \"fast\"))] const K: Kdf = Kdf::CHEAP_FOR_TESTS;",
        "#[cfg(not(test))] const K: Kdf = Kdf::CHEAP_FOR_TESTS;",
        "impl S { fn k() -> Kdf { Kdf::CHEAP_FOR_TESTS } }",
        "mod inner { pub fn k() -> Kdf { Kdf::CHEAP_FOR_TESTS } }",
        "const K: Kdf = Kdf::r#CHEAP_FOR_TESTS;",
        "fn f(x: u8) -> Kdf { match x { 0 => Kdf::CHEAP_FOR_TESTS, _ => Kdf::RECOMMENDED } }",
    ];
    for source in flagged {
        assert!(
            names_cheap_kdf_outside_tests(source).expect("test source parses"),
            "the detector missed a non-test use: {source}"
        );
    }

    let allowed = [
        "#[cfg(test)] mod tests { const K: Kdf = Kdf::CHEAP_FOR_TESTS; }",
        "#[cfg(test)] const K: Kdf = Kdf::CHEAP_FOR_TESTS;",
        "#[cfg(all(test, unix))] fn k() -> Kdf { Kdf::CHEAP_FOR_TESTS }",
        "#[test] fn opens() { let k = Kdf::CHEAP_FOR_TESTS; }",
        "impl S { #[cfg(test)] fn k() -> Kdf { Kdf::CHEAP_FOR_TESTS } }",
        "fn f() { #[cfg(test)] let k = Kdf::CHEAP_FOR_TESTS; }",
        "#![cfg(test)] const K: Kdf = Kdf::CHEAP_FOR_TESTS;",
        "fn f() { #[cfg(test)] { use_kdf(Kdf::CHEAP_FOR_TESTS); } }",
        "fn f(x: u8) -> Kdf { match x { #[cfg(test)] 0 => Kdf::CHEAP_FOR_TESTS, _ => Kdf::RECOMMENDED } }",
        "fn f() -> S { S { #[cfg(test)] k: Kdf::CHEAP_FOR_TESTS, r: Kdf::RECOMMENDED } }",
        "const K: Kdf = Kdf::RECOMMENDED;",
    ];
    for source in allowed {
        assert!(
            !names_cheap_kdf_outside_tests(source).expect("test source parses"),
            "the detector flagged test-only code: {source}"
        );
    }
}

/// Lays out a probe crate under the temporary directory and returns its root.
fn probe_crate(tag: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tawara-policy-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&dir);
    for (rel, body) in files {
        let p = dir.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, body).unwrap();
    }
    normalise(&dir)
}

/// The walk and the scan, held to a crate laid out on disk where the answer
/// is known. Production code reached through a nested module, an
/// `include!`, a `cfg_attr` path, or nothing at all is flagged; code reached
/// only through test-only declarations is exempt; an integration test is not
/// scanned, unless production code loads it. Declarations in an `include!`d
/// file resolve beside it, as rustc 1.98.0 resolves them; the `decoy` files
/// sit where they would resolve otherwise.
#[test]
fn the_source_walk_scans_what_rustc_may_compile() {
    const CHEAP: &str = "const K: Kdf = Kdf::CHEAP_FOR_TESTS;\n";
    const SAFE: &str = "const K: Kdf = Kdf::RECOMMENDED;\n";
    let dir = probe_crate(
        "walk",
        &[
            ("Cargo.toml", "[package]\nname = \"probe\"\n"),
            (
                "src/lib.rs",
                "mod a;\n\
                 #[cfg(test)] mod tests;\n\
                 #[path = \"elsewhere/c.rs\"] mod c;\n\
                 mod inline { mod d; }\n\
                 include!(\"kdf_impl.rs\");\n\
                 #[cfg_attr(unix, path = \"unix.rs\")] mod platform;\n\
                 #[cfg_attr(windows, path = \"win_only.rs\")] mod plat2;\n\
                 #[cfg_attr(test, path = \"test_path.rs\")] mod tp;\n\
                 #[cfg(test)] mod t2 { mod deep; }\n\
                 #[cfg(test)] mod t3 { include!(\"fixture.rs\"); }\n\
                 #[test] fn fx() { #[path = \"fixture_fn.rs\"] mod f; }\n\
                 #[cfg(test)] mod t5 { #[test] fn fx() { #[path = \"fixture_t5.rs\"] mod f; } }\n\
                 mod shared;\n\
                 #[cfg(test)] #[path = \"shared.rs\"] mod shared_t;\n\
                 #[path = \"d.rs\"] #[cfg_attr(unix, path = \"d_unix.rs\")] mod dd;\n\
                 include!(\"nested/inc.rs\");\n\
                 mod m { include!(\"inc_m.rs\"); }\n\
                 mod foo;\n\
                 mod dual;\n\
                 include!(\"dual.rs\");\n",
            ),
            (
                "src/dual.rs",
                "mod inner { #[path = \"../../tests/dual_t.rs\"] mod y; }\n",
            ),
            // rustc resolves `..` only through directories that exist.
            ("src/dual/inner/keep.txt", ""),
            ("src/inner/keep.txt", ""),
            ("src/tests/dual_t.rs", SAFE),
            ("tests/dual_t.rs", CHEAP),
            (
                "src/nested/inc.rs",
                "#[path = \"../../tests/shared_it.rs\"] mod imp;\n\
                 mod nb;\n\
                 mod ne { mod nf; }\n",
            ),
            ("tests/shared_it.rs", CHEAP),
            ("src/nested/nb.rs", SAFE),
            ("src/nested/ne/nf.rs", SAFE),
            ("src/nb.rs", SAFE),
            ("src/ne/nf.rs", SAFE),
            ("src/inc_m.rs", "mod mc;\n"),
            ("src/mc.rs", SAFE),
            ("src/m/mc.rs", SAFE),
            ("src/foo.rs", "include!(\"inc_foo.rs\");\n"),
            ("src/inc_foo.rs", "mod fd;\n"),
            ("src/fd.rs", SAFE),
            ("src/foo/fd.rs", SAFE),
            ("src/plat2.rs", SAFE),
            ("src/d_unix.rs", SAFE),
            ("src/tests.rs", CHEAP),
            ("src/a.rs", "mod b;\n"),
            ("src/a/b.rs", CHEAP),
            ("src/elsewhere/c.rs", SAFE),
            ("src/inline/d.rs", SAFE),
            ("src/kdf_impl.rs", CHEAP),
            ("src/platform.rs", SAFE),
            ("src/unix.rs", CHEAP),
            ("src/win_only.rs", SAFE),
            ("src/tp.rs", SAFE),
            ("src/test_path.rs", CHEAP),
            ("src/t2/deep.rs", CHEAP),
            ("src/fixture.rs", CHEAP),
            ("src/fixture_fn.rs", CHEAP),
            ("src/t5/fixture_t5.rs", CHEAP),
            ("src/shared.rs", CHEAP),
            ("src/d.rs", SAFE),
            ("src/stray.rs", CHEAP),
            ("tests/it.rs", CHEAP),
        ],
    );
    let rel = |set: &BTreeSet<PathBuf>| -> BTreeSet<String> {
        set.iter()
            .map(|p| {
                p.strip_prefix(&dir)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect()
    };
    let walk = walk_sources(&dir).expect("the probe crate walks");
    let reached = rel(&walk.reached);
    let exempt = rel(&walk.exempt);
    let offenders = rel(&cheap_kdf_offenders(&dir).into_iter().collect());
    let _ = fs::remove_dir_all(&dir);

    for expected in [
        "src/lib.rs",
        "src/a.rs",
        "src/a/b.rs",
        "src/elsewhere/c.rs",
        "src/inline/d.rs",
        "src/kdf_impl.rs",
        "src/platform.rs",
        "src/unix.rs",
        "src/win_only.rs",
        "src/tp.rs",
        "src/shared.rs",
        "src/d.rs",
        "src/d_unix.rs",
        "src/plat2.rs",
        "src/nested/inc.rs",
        "tests/shared_it.rs",
        "src/nested/nb.rs",
        "src/nested/ne/nf.rs",
        "src/inc_m.rs",
        "src/mc.rs",
        "src/foo.rs",
        "src/inc_foo.rs",
        "src/fd.rs",
        "src/dual.rs",
        "src/tests/dual_t.rs",
        "tests/dual_t.rs",
    ] {
        assert!(
            reached.contains(expected),
            "the walk did not reach {expected}: {reached:?}"
        );
    }
    for decoy in ["src/nb.rs", "src/ne/nf.rs", "src/m/mc.rs", "src/foo/fd.rs"] {
        assert!(
            !reached.contains(decoy),
            "the walk resolved a declaration in an included file to {decoy}, where rustc does \
             not look: {reached:?}"
        );
    }
    assert_eq!(
        exempt,
        BTreeSet::from(
            [
                "src/tests.rs",
                "src/test_path.rs",
                "src/t2/deep.rs",
                "src/fixture.rs",
                "src/fixture_fn.rs",
                "src/t5/fixture_t5.rs"
            ]
            .map(String::from)
        ),
        "only files reached solely through test-only declarations are exempt"
    );
    assert_eq!(
        offenders,
        BTreeSet::from(
            [
                "src/a/b.rs",
                "src/kdf_impl.rs",
                "src/unix.rs",
                "src/stray.rs",
                "src/shared.rs",
                "tests/shared_it.rs",
                "tests/dual_t.rs"
            ]
            .map(String::from)
        ),
        "production code reached by any route, or by none, is flagged, even when a test also \
         reaches it; tests/ is not scanned unless production code loads it"
    );
}

/// What the walk cannot read, it refuses rather than passes.
#[test]
fn the_source_walk_refuses_what_it_cannot_read() {
    // Each case: the probe crate's src/lib.rs, any other files, and what the
    // refusal must say.
    type Extra<'a> = &'a [(&'a str, &'a str)];
    let cases: [(&str, Extra, &str); 17] = [
        (
            "include!(concat!(env!(\"OUT_DIR\"), \"/generated.rs\"));\n",
            &[],
            "not a string literal",
        ),
        (
            "#[cfg_attr(unix, path = \"u\")] mod inline_cond { }\n",
            &[],
            "conditional or repeated path",
        ),
        (
            "pub fn f() -> u32 { #[path = \"../../outside.rs\"] mod imp; 0 }\n",
            &[],
            "inside a block",
        ),
        (
            "macro_rules! m { () => { #[path = \"x.rs\"] mod imp; }; }\n",
            &[],
            "declare a module or use `include!`",
        ),
        (
            "macro_rules! l { () => { include!(\"x.rs\"); }; }\nl!();\n",
            &[],
            "declare a module or use `include!`",
        ),
        (
            "some_dep::load!(\"../../outside.rs\");\n",
            &[],
            "may name a `.rs` file",
        ),
        // The value counts, not the spelling: this is `shared.rs`.
        (
            "some_dep::load!(\"shared.r\\x73\");\n",
            &[],
            "may name a `.rs` file",
        ),
        (
            "some_dep::load!(\"shared.RS\");\n",
            &[],
            "may name a `.rs` file",
        ),
        (
            "some_dep::load!(concat!(\"shared.\", 'r', stringify!(s)));\n",
            &[],
            "may name a `.rs` file",
        ),
        (
            "some_dep::load!(concat!(\"shared\", env!(\"SUFFIX\")));\n",
            &[],
            "may name a `.rs` file",
        ),
        (
            "use std::include as load;\nload!(\"shared.r\\x73\");\n",
            &[],
            "`include` imported as `load`",
        ),
        (
            "pub fn f() { use core::{r#include as load}; }\n",
            &[],
            "`include` imported as `load`",
        ),
        (
            "r#include!(concat!(\"shared\", \".rs\"));\n",
            &[],
            "not a string literal",
        ),
        // Every file a non-test declaration can select must exist.
        (
            "#[path = \"absent.rs\"] mod imp;\n",
            &[],
            "which does not exist",
        ),
        (
            "#[cfg_attr(unix, path = \"absent.rs\")] mod imp;\n",
            &[("src/imp.rs", "")],
            "which does not exist",
        ),
        (
            "#[cfg_attr(windows, path = \"win.rs\")] mod imp;\n",
            &[("src/win.rs", "")],
            "resolves to neither",
        ),
        (
            "include!(\"nested/inc.rs\");\n",
            &[("src/nested/inc.rs", "#[path = \"gone.rs\"] mod imp;\n")],
            "which does not exist",
        ),
    ];
    for (i, (lib, extra, expected)) in cases.into_iter().enumerate() {
        let mut files = vec![
            ("Cargo.toml", "[package]\nname = \"probe\"\n"),
            ("src/lib.rs", lib),
        ];
        files.extend_from_slice(extra);
        let dir = probe_crate(&format!("refuse{i}"), &files);
        let result = walk_sources(&dir).map(|_| ());
        let _ = fs::remove_dir_all(&dir);
        match result {
            Err(e) => assert!(e.contains(expected), "refused for the wrong reason: {e}"),
            Ok(()) => panic!("the walk accepted what it cannot read: {lib}"),
        }
    }

    // A module file reached through a symbolic link: before its target and
    // after it (the refusal must come before the canonical visit key merges
    // the two), and through a linked directory. Unix only: making a link on
    // Windows needs a privilege CI does not have.
    #[cfg(unix)]
    for (i, lib) in [
        "#[path = \"alias/common.rs\"] mod first;\n#[path = \"actual/common.rs\"] mod second;\n",
        "#[path = \"actual/common.rs\"] mod first;\n#[path = \"alias/common.rs\"] mod second;\n",
        "#[path = \"linked/common.rs\"] mod first;\n",
    ]
    .into_iter()
    .enumerate()
    {
        let dir = probe_crate(
            &format!("link{i}"),
            &[
                ("Cargo.toml", "[package]\nname = \"probe\"\n"),
                ("src/lib.rs", lib),
                ("src/actual/common.rs", "#[path = \"shared.rs\"] mod imp;\n"),
                ("src/actual/shared.rs", ""),
                ("src/alias/shared.rs", ""),
            ],
        );
        std::os::unix::fs::symlink("../actual/common.rs", dir.join("src/alias/common.rs"))
            .expect("make a file link");
        std::os::unix::fs::symlink("actual", dir.join("src/linked"))
            .expect("make a directory link");
        let result = walk_sources(&dir).map(|_| ());
        let _ = fs::remove_dir_all(&dir);
        match result {
            Err(e) => assert!(
                e.contains("symbolic link"),
                "refused for the wrong reason: {e}"
            ),
            Ok(()) => panic!("the walk accepted a module reached through a link: {lib}"),
        }
    }
}

// ---------------------------------------------------------------------------
// The library pin.
// ---------------------------------------------------------------------------

/// The package a dependency entry resolves to: its `package` key when it
/// renames one, else its key.
fn package_name<'a>(key: &'a str, value: &'a toml::Value) -> &'a str {
    value.get("package").and_then(|v| v.as_str()).unwrap_or(key)
}

fn dependency_tables(manifest: &toml::Table) -> Vec<(String, &toml::Table)> {
    let mut out = Vec::new();
    for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(t) = manifest.get(section).and_then(|v| v.as_table()) {
            out.push((section.to_string(), t));
        }
    }
    if let Some(targets) = manifest.get("target").and_then(|v| v.as_table()) {
        for (cfg, body) in targets {
            for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
                if let Some(t) = body.get(section).and_then(|v| v.as_table()) {
                    out.push((format!("target.{cfg}.{section}"), t));
                }
            }
        }
    }
    out
}

/// The library comes from [`LIBRARY_GIT`] at an exact commit, with
/// `default-features = false` and exactly `native` and `mesh-https`, and is
/// never patched, replaced or path-overridden.
/// Upgrading it is a change to `rev` and to nothing else. (deny.toml also
/// refuses `raw-backend` in the resolved graph, whatever route turns it on.)
#[test]
fn library_is_pinned_by_full_commit_hash_and_never_patched() {
    let root = workspace_root();
    let manifest = read_toml(&root.join("Cargo.toml"));

    assert!(
        !manifest.contains_key("patch") && !manifest.contains_key("replace"),
        "the workspace manifest has a [patch] or [replace] section; this repository never \
         modifies the library"
    );

    let workspace_deps = manifest["workspace"]["dependencies"]
        .as_table()
        .expect("[workspace.dependencies] exists");
    let library_lines: Vec<&String> = workspace_deps
        .iter()
        .filter(|(k, v)| package_name(k, v) == LIBRARY)
        .map(|(k, _)| k)
        .collect();
    assert_eq!(
        library_lines,
        vec![LIBRARY],
        "[workspace.dependencies] must name {LIBRARY} exactly once, under its own name"
    );
    let dep = workspace_deps[LIBRARY]
        .as_table()
        .expect("mochimo-crypto is a table in [workspace.dependencies]");
    assert_eq!(
        dep.get("git").and_then(|v| v.as_str()),
        Some(LIBRARY_GIT),
        "mochimo-crypto must come from the library's pinned repository"
    );
    let rev = dep.get("rev").and_then(|v| v.as_str()).unwrap_or("");
    assert!(
        rev.len() == 40
            && rev
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "mochimo-crypto must be pinned by a full 40-character lowercase commit hash, not {rev:?}"
    );
    assert_eq!(
        tawara_wallet_core::LIBRARY_REV,
        rev,
        "wallet-core's LIBRARY_REV, which Settings shows, must name the pinned commit"
    );
    let allowed_keys = BTreeSet::from(["git", "rev", "default-features", "features"]);
    for key in dep.keys() {
        assert!(
            allowed_keys.contains(key.as_str()),
            "mochimo-crypto's line takes the pinned form (git, rev, default-features, features); \
             it also names `{key}`"
        );
    }
    assert_eq!(
        dep.get("default-features").and_then(|v| v.as_bool()),
        Some(false),
        "mochimo-crypto must be taken with default-features = false"
    );
    let features: BTreeSet<&str> = dep
        .get("features")
        .and_then(|v| v.as_array())
        .expect("mochimo-crypto names its features")
        .iter()
        .map(|v| v.as_str().expect("feature names are strings"))
        .collect();
    assert_eq!(
        features,
        BTreeSet::from(["native", "mesh-https"]),
        "mochimo-crypto's features are exactly native and mesh-https; raw-backend is the \
         library's test-only feature and must never be named here"
    );

    // Every crate takes the library from the workspace line, under its own
    // name, adds nothing to it, and forwards none of its features; and no
    // crate manifest carries a [patch] of its own.
    for dir in crate_dirs() {
        let manifest_path = dir.join("Cargo.toml");
        let crate_manifest = read_toml(&manifest_path);
        assert!(
            !crate_manifest.contains_key("patch") && !crate_manifest.contains_key("replace"),
            "{} has a [patch] or [replace] section",
            manifest_path.display()
        );
        for (section, deps) in dependency_tables(&crate_manifest) {
            for (key, d) in deps.iter().filter(|(k, v)| package_name(k, v) == LIBRARY) {
                assert_eq!(
                    key,
                    LIBRARY,
                    "{} renames mochimo-crypto to `{key}` in [{section}]",
                    manifest_path.display()
                );
                let keys: Vec<&String> =
                    d.as_table().map(|t| t.keys().collect()).unwrap_or_default();
                assert!(
                    keys == vec!["workspace"]
                        && d.get("workspace").and_then(|v| v.as_bool()) == Some(true),
                    "{} names mochimo-crypto in [{section}] with a line of its own ({keys:?}); \
                     it must be `mochimo-crypto.workspace = true`, with features decided once, \
                     in the workspace line",
                    manifest_path.display()
                );
            }
        }
        if let Some(feats) = crate_manifest.get("features").and_then(|v| v.as_table()) {
            for (name, list) in feats {
                for entry in list
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|v| v.as_str())
                {
                    assert!(
                        !entry.starts_with("mochimo-crypto/")
                            && !entry.starts_with("mochimo-crypto?/"),
                        "{} feature `{name}` enables `{entry}`; mochimo-crypto's features are \
                         decided once, in the workspace line",
                        manifest_path.display()
                    );
                }
            }
        }
    }

    // Cargo's own configuration can patch, replace or path-override a source
    // too, from any `.cargo` directory cargo would read in this repository.
    for path in cargo_configs() {
        let config = read_toml(&path);
        for key in ["patch", "source", "paths"] {
            assert!(
                !config.contains_key(key),
                "{} sets `{key}`, which patches, replaces or path-overrides a source",
                path.display()
            );
        }
    }
    assert!(
        !root.join("vendor").exists(),
        "a vendor/ directory exists; this repository never carries a copy of the library"
    );

    // And the lockfile resolved the library at that commit.
    let lock = read_toml(&root.join("Cargo.lock"));
    let locked: Vec<&toml::Table> = lock["package"]
        .as_array()
        .expect("Cargo.lock lists packages")
        .iter()
        .filter_map(|p| p.as_table())
        .filter(|p| p.get("name").and_then(|v| v.as_str()) == Some(LIBRARY))
        .collect();
    assert_eq!(
        locked.len(),
        1,
        "Cargo.lock must hold exactly one mochimo-crypto"
    );
    let source = locked[0]
        .get("source")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert_eq!(
        source,
        format!("git+{LIBRARY_GIT}?rev={rev}#{rev}"),
        "Cargo.lock resolved mochimo-crypto from somewhere other than the pinned commit"
    );
}

/// Whether `name` is one of iced's own packages.
fn is_iced(name: &str) -> bool {
    name == "iced" || name.starts_with("iced_")
}

/// Every entry of `manifest`'s `[features]` table that switches on a feature of
/// an iced crate (`iced/..`, `iced?/..`, `iced_runtime/..`), as the feature's
/// name and the entry. iced's features are decided once, in the workspace line,
/// and a crate's own feature would add to them in every build that turns it
/// on, `default` included.
fn iced_feature_forwards(manifest: &toml::Table) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Some(feats) = manifest.get("features").and_then(|v| v.as_table()) {
        for (name, list) in feats {
            for entry in list
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str())
            {
                if let Some((dep, _)) = entry.split_once('/')
                    && is_iced(dep.strip_suffix('?').unwrap_or(dep))
                {
                    out.push((name.clone(), entry.to_owned()));
                }
            }
        }
    }
    out
}

/// iced comes from [`ICED_GIT`], every crate of it from one commit named by
/// its full hash: in the workspace's lines, in every crate's, and in the
/// lockfile. No crate switches on an iced feature from its own `[features]`
/// either, so the workspace line's list is the whole of what ships.
#[test]
fn iced_comes_from_the_fork_pinned_by_full_commit_hash() {
    let root = workspace_root();
    let manifest = read_toml(&root.join("Cargo.toml"));
    let workspace_deps = manifest["workspace"]["dependencies"]
        .as_table()
        .expect("[workspace.dependencies] exists");
    let mut revs = BTreeSet::new();
    let mut lines = 0;
    for (key, dep) in workspace_deps
        .iter()
        .filter(|(k, v)| is_iced(package_name(k, v)))
    {
        lines += 1;
        let dep = dep
            .as_table()
            .unwrap_or_else(|| panic!("`{key}` must be a table naming the fork and its commit"));
        assert_eq!(
            dep.get("git").and_then(|v| v.as_str()),
            Some(ICED_GIT),
            "`{key}` must come from the pinned fork of iced"
        );
        let rev = dep.get("rev").and_then(|v| v.as_str()).unwrap_or("");
        assert!(
            rev.len() == 40
                && rev
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "`{key}` must be pinned by a full 40-character lowercase commit hash, not {rev:?}"
        );
        revs.insert(rev.to_owned());
        let allowed = BTreeSet::from(["git", "rev", "default-features", "features"]);
        for k in dep.keys() {
            assert!(
                allowed.contains(k.as_str()),
                "`{key}` is chosen by its commit alone; it also names `{k}`"
            );
        }
    }
    assert!(lines >= 1, "[workspace.dependencies] names no iced crate");
    assert_eq!(
        revs.len(),
        1,
        "every iced line names the same commit, so one copy of each crate is built: {revs:?}"
    );
    let rev = revs.pop_first().unwrap_or_default();

    // Every crate takes iced from the workspace's lines.
    for dir in crate_dirs() {
        let manifest_path = dir.join("Cargo.toml");
        let crate_manifest = read_toml(&manifest_path);
        for (section, deps) in dependency_tables(&crate_manifest) {
            for (key, d) in deps.iter().filter(|(k, v)| is_iced(package_name(k, v))) {
                let keys: Vec<&String> =
                    d.as_table().map(|t| t.keys().collect()).unwrap_or_default();
                assert!(
                    keys == vec!["workspace"]
                        && d.get("workspace").and_then(|v| v.as_bool()) == Some(true),
                    "{} names `{key}` in [{section}] with a line of its own ({keys:?}); it must \
                     be `{key}.workspace = true`",
                    manifest_path.display()
                );
            }
        }
        let forwards = iced_feature_forwards(&crate_manifest);
        assert!(
            forwards.is_empty(),
            "{} switches on iced features from its own [features] ({forwards:?}); iced's \
             features are decided once, in the workspace line",
            manifest_path.display()
        );
    }

    // And the lockfile resolved every iced crate at that commit.
    let lock = read_toml(&root.join("Cargo.lock"));
    let iced: Vec<(&str, &str)> = lock["package"]
        .as_array()
        .expect("Cargo.lock lists packages")
        .iter()
        .filter_map(|p| p.as_table())
        .filter_map(|p| {
            let name = p.get("name")?.as_str()?;
            is_iced(name).then(|| (name, p.get("source").and_then(|v| v.as_str()).unwrap_or("")))
        })
        .collect();
    assert!(!iced.is_empty(), "Cargo.lock holds no iced crate");
    let expected = fork_source(&rev);
    for (name, source) in iced {
        assert_eq!(
            source, expected,
            "Cargo.lock resolved {name} from somewhere other than the pinned commit"
        );
    }
}

/// The forwarding check above, held to fixed manifests, so that a check that
/// sees nothing cannot pass every crate.
#[test]
fn the_iced_forwarding_check_sees_what_it_must() {
    let flagged = [
        "[features]\ndefault = [\"iced/default\"]",
        "[features]\npicture = [\"iced?/canvas\"]",
        "[features]\nselect = [\"iced_runtime/selector\"]",
        "[features]\nphone = [\"serde/std\", \"iced_winit/android-native-activity\"]",
    ];
    for source in flagged {
        let manifest = source.parse::<toml::Table>().expect("test manifest parses");
        assert_eq!(
            iced_feature_forwards(&manifest).len(),
            1,
            "the check missed an iced feature switched on by a crate: {source}"
        );
    }
    let allowed = [
        "[package]\nname = \"x\"",
        "[features]",
        "[features]\ndefault = []",
        "[features]\nfast = [\"serde/std\", \"dep:chrono\", \"icedtea/x\"]",
    ];
    for source in allowed {
        let manifest = source.parse::<toml::Table>().expect("test manifest parses");
        assert!(
            iced_feature_forwards(&manifest).is_empty(),
            "the check flagged something that switches on no iced feature: {source}"
        );
    }
}

/// The source Cargo.lock records for a package taken from the fork at `rev`.
fn fork_source(rev: &str) -> String {
    format!("git+{ICED_GIT}?rev={rev}#{rev}")
}

/// The commit the `iced` line of the workspace manifest pins.
fn iced_rev() -> String {
    let manifest = read_toml(&workspace_root().join("Cargo.toml"));
    manifest["workspace"]["dependencies"]["iced"]["rev"]
        .as_str()
        .expect("[workspace.dependencies] pins iced by rev")
        .to_owned()
}

/// Every package Cargo.lock lists, as its name and its source (empty for a
/// package with no source, which is a path package).
fn locked_packages() -> Vec<(String, String)> {
    let lock = read_toml(&workspace_root().join("Cargo.lock"));
    lock["package"]
        .as_array()
        .expect("Cargo.lock lists packages")
        .iter()
        .filter_map(|p| p.as_table())
        .map(|p| {
            let name = p["name"].as_str().expect("a package has a name");
            let source = p.get("source").and_then(|v| v.as_str()).unwrap_or("");
            (name.to_owned(), source.to_owned())
        })
        .collect()
}

/// Every package Cargo.lock takes from the fork is at the pinned commit, and is
/// the only package of its name in the lock: the build holds one copy of
/// each. This reaches winit and dpi, which no manifest names, and every other
/// package the fork ships.
#[test]
fn every_fork_package_is_at_the_pinned_commit_once() {
    let expected = fork_source(&iced_rev());
    let fork_prefix = format!("git+{ICED_GIT}");
    let packages = locked_packages();
    let from_fork: Vec<&(String, String)> = packages
        .iter()
        .filter(|(_, source)| source.starts_with(&fork_prefix))
        .collect();
    assert!(
        !from_fork.is_empty(),
        "Cargo.lock holds no package from the fork; a check over nothing passes vacuously"
    );
    for (name, source) in from_fork {
        assert_eq!(
            source, &expected,
            "Cargo.lock resolved {name} from the fork at another commit than the pinned one"
        );
        let copies = packages.iter().filter(|(n, _)| n == name).count();
        assert_eq!(
            copies, 1,
            "Cargo.lock holds {copies} packages named {name}; the build holds one copy of each"
        );
    }
}

/// Cargo.lock holds exactly one winit, and it is the fork's, at the pinned
/// commit. A second copy would register the same Objective-C classes twice on
/// iOS, and objc2 panics on the duplicate name.
#[test]
fn winit_is_one_package_from_the_fork() {
    let sources: Vec<String> = locked_packages()
        .into_iter()
        .filter(|(name, _)| name == "winit")
        .map(|(_, source)| source)
        .collect();
    assert_eq!(
        sources,
        vec![fork_source(&iced_rev())],
        "Cargo.lock must hold exactly one winit, from the fork at the pinned commit"
    );
}

/// No manifest of the wallet workspace names winit as a dependency, under any
/// name and in any table. winit comes to the build through iced_winit, from
/// the fork, and a line of its own would be a second route to a copy of it.
/// The check reads the workspace manifest and the members under `crates/`.
#[test]
fn no_manifest_names_winit() {
    let root_manifest = workspace_root().join("Cargo.toml");
    let workspace = read_toml(&root_manifest);
    for (key, value) in workspace["workspace"]["dependencies"]
        .as_table()
        .expect("[workspace.dependencies] exists")
    {
        assert!(
            package_name(key, value) != "winit",
            "{} names winit as `{key}` in [workspace.dependencies]; winit comes from the fork \
             through iced_winit alone",
            root_manifest.display()
        );
    }
    for dir in crate_dirs() {
        let manifest_path = dir.join("Cargo.toml");
        let crate_manifest = read_toml(&manifest_path);
        for (section, deps) in dependency_tables(&crate_manifest) {
            for (key, value) in deps {
                assert!(
                    package_name(key, value) != "winit",
                    "{} names winit as `{key}` in [{section}]; winit comes from the fork \
                     through iced_winit alone",
                    manifest_path.display()
                );
            }
        }
    }
}

/// The `iced` line is taken with `default-features = false` and exactly the
/// features ICED_FEATURES lists, each once.
/// Defaults would turn on features nobody chose for this build, and a feature
/// added or dropped changes what ships.
#[test]
fn iced_line_keeps_default_features_off_and_its_listed_features() {
    let manifest = read_toml(&workspace_root().join("Cargo.toml"));
    let iced = manifest["workspace"]["dependencies"]["iced"]
        .as_table()
        .expect("[workspace.dependencies] names iced as a table");
    assert_eq!(
        iced.get("default-features").and_then(|v| v.as_bool()),
        Some(false),
        "the iced line must keep default-features = false"
    );
    let mut features: Vec<&str> = iced
        .get("features")
        .and_then(|v| v.as_array())
        .expect("the iced line names its features")
        .iter()
        .map(|v| v.as_str().expect("feature names are strings"))
        .collect();
    features.sort_unstable();
    let mut listed = ICED_FEATURES.to_vec();
    listed.sort_unstable();
    assert_eq!(
        features, listed,
        "the iced line's features differ from ICED_FEATURES; change both together, and only \
         as a deliberate change to what ships"
    );
}

// ---------------------------------------------------------------------------
// The release profile.
// ---------------------------------------------------------------------------

/// Every table under `table`, at any depth, that turns overflow checks off or
/// sets `panic = "abort"`: a profile, a `package.<name>` or `package."*"`
/// override, or a `build-override`.
fn profile_offenders(path: &str, table: &toml::Table, out: &mut Vec<String>) {
    if table.get("overflow-checks").and_then(|v| v.as_bool()) == Some(false) {
        out.push(format!("[{path}] sets overflow-checks = false"));
    }
    if table.get("panic").and_then(|v| v.as_str()) == Some("abort") {
        out.push(format!("[{path}] sets panic = \"abort\""));
    }
    for (key, value) in table {
        if let Some(sub) = value.as_table() {
            profile_offenders(&format!("{path}.{key}"), sub, out);
        }
    }
}

/// Every rustflags value in a cargo configuration, as strings.
fn rustflags(config: &toml::Table) -> Vec<String> {
    let mut out = Vec::new();
    let mut push = |v: &toml::Value| match v {
        toml::Value::String(s) => out.push(s.clone()),
        toml::Value::Array(a) => out.extend(a.iter().filter_map(|x| x.as_str()).map(String::from)),
        _ => {}
    };
    if let Some(v) = config.get("build").and_then(|b| b.get("rustflags")) {
        push(v);
    }
    if let Some(targets) = config.get("target").and_then(|t| t.as_table()) {
        for body in targets.values() {
            if let Some(v) = body.get("rustflags") {
                push(v);
            }
        }
    }
    out
}

/// Cargo reads `[profile.*]` only from the root of the workspace being built,
/// so the library's own release profile does not reach this build. Its key
/// scrubs run through `Drop`, which `panic = "abort"` skips, and a release
/// build turns overflow checks off unless told otherwise. Both are restated in
/// the root manifest, and this holds them there and holds every other place
/// in the repository that could undo them: any profile or per-package
/// override in the root manifest, the profiles in cargo's configuration
/// files, and the rustflags those files set.
#[test]
fn release_profile_keeps_the_library_scrubs() {
    let manifest = read_toml(&workspace_root().join("Cargo.toml"));
    let profiles = manifest
        .get("profile")
        .and_then(|v| v.as_table())
        .expect("the workspace manifest has a [profile] table");
    let release = profiles
        .get("release")
        .and_then(|v| v.as_table())
        .expect("the workspace manifest has [profile.release]");
    assert_eq!(
        release.get("panic").and_then(|v| v.as_str()),
        Some("unwind"),
        "[profile.release] must keep panic = \"unwind\": under abort, Drop does not run and the \
         library's scrubs of key material are skipped"
    );
    assert_eq!(
        release.get("overflow-checks").and_then(|v| v.as_bool()),
        Some(true),
        "[profile.release] must keep overflow-checks = true"
    );

    let mut offenders = Vec::new();
    profile_offenders("profile", profiles, &mut offenders);
    for path in cargo_configs() {
        let config = read_toml(&path);
        if let Some(p) = config.get("profile").and_then(|v| v.as_table()) {
            profile_offenders(&format!("{}: profile", path.display()), p, &mut offenders);
        }
        for flags in rustflags(&config) {
            let flat: String = flags
                .to_ascii_lowercase()
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect();
            for bad in [
                "overflow-checks=off",
                "overflow-checks=no",
                "overflow-checks=n",
                "overflow-checks=false",
                "panic=abort",
            ] {
                if flat.contains(bad) {
                    offenders.push(format!("{}: rustflags `{flags}`", path.display()));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "the library's scrubs need panic = \"unwind\" and overflow checks on: {offenders:?}"
    );
}

// ---------------------------------------------------------------------------
// wallet-core and the interface.
// ---------------------------------------------------------------------------

/// Every package in `start`'s dependency closure, as Cargo.lock resolved it.
fn lock_closure(start: &str) -> BTreeSet<String> {
    let lock = read_toml(&workspace_root().join("Cargo.lock"));
    let packages: Vec<&toml::Table> = lock["package"]
        .as_array()
        .expect("Cargo.lock lists packages")
        .iter()
        .filter_map(|p| p.as_table())
        .collect();
    let mut by_name: BTreeMap<&str, Vec<&toml::Table>> = BTreeMap::new();
    for p in &packages {
        by_name
            .entry(p["name"].as_str().expect("a package has a name"))
            .or_default()
            .push(p);
    }
    // A dependency entry is `name`, `name version` or `name version (source)`.
    let find = |spec: &str| -> &toml::Table {
        let mut parts = spec.split(' ');
        let name = parts.next().expect("a dependency entry has a name");
        let version = parts.next();
        let candidates: Vec<&toml::Table> = by_name
            .get(name)
            .into_iter()
            .flatten()
            .copied()
            .filter(|p| version.is_none_or(|v| p["version"].as_str() == Some(v)))
            .collect();
        assert_eq!(
            candidates.len(),
            1,
            "Cargo.lock entry `{spec}` resolves to {} packages",
            candidates.len()
        );
        candidates[0]
    };

    let mut seen = BTreeSet::new();
    let mut stack = vec![start.to_string()];
    while let Some(spec) = stack.pop() {
        let package = find(&spec);
        let id = format!(
            "{} {}",
            package["name"].as_str().unwrap(),
            package["version"].as_str().unwrap()
        );
        if !seen.insert(id) {
            continue;
        }
        for dep in package
            .get("dependencies")
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str())
        {
            stack.push(dep.to_string());
        }
    }
    seen.into_iter()
        .map(|id| id.split(' ').next().unwrap().to_string())
        .collect()
}

/// `pub use mochimo_crypto;`, `pub use mochimo_crypto as x;`, and `self` or
/// `*` directly under `mochimo_crypto`, at any depth of a use tree.
fn use_tree_is_whole_library(tree: &syn::UseTree, under_lib: bool) -> bool {
    match tree {
        syn::UseTree::Name(n) => {
            n.ident.unraw() == LIBRARY_IDENT || (under_lib && n.ident.unraw() == "self")
        }
        syn::UseTree::Rename(r) => {
            r.ident.unraw() == LIBRARY_IDENT || (under_lib && r.ident.unraw() == "self")
        }
        syn::UseTree::Glob(_) => under_lib,
        syn::UseTree::Path(p) => {
            use_tree_is_whole_library(&p.tree, p.ident.unraw() == LIBRARY_IDENT)
        }
        syn::UseTree::Group(g) => g
            .items
            .iter()
            .any(|t| use_tree_is_whole_library(t, under_lib)),
    }
}

fn reexports_library(items: &[syn::Item]) -> bool {
    items.iter().any(|item| match item {
        syn::Item::Use(u) if matches!(u.vis, syn::Visibility::Public(_)) => {
            use_tree_is_whole_library(&u.tree, false)
        }
        syn::Item::ExternCrate(e) if matches!(e.vis, syn::Visibility::Public(_)) => {
            e.ident.unraw() == LIBRARY_IDENT
        }
        syn::Item::Mod(m) => m
            .content
            .as_ref()
            .is_some_and(|(_, inner)| reexports_library(inner)),
        _ => false,
    })
}

/// `wallet-core` has no interface dependency, so the platform shells can be
/// replaced without touching it; and the interface crates talk to
/// `wallet-core` only, never to the library.
#[test]
fn wallet_core_and_the_interface_stay_apart() {
    let crates_dir = workspace_root().join("crates");

    // Nothing in wallet-core's dependency closure, as Cargo.lock resolved it,
    // is an interface toolkit, a windowing layer, a renderer or the
    // interface itself.
    let closure = lock_closure("tawara-wallet-core");
    assert!(
        closure.contains(LIBRARY),
        "the closure walk did not reach mochimo-crypto from wallet-core; a walk over nothing \
         passes vacuously"
    );
    for name in &closure {
        assert!(
            !INTERFACE_DEPENDENCY_PREFIXES
                .iter()
                .any(|p| name.starts_with(p)),
            "wallet-core's dependency closure holds `{name}`; it must have no interface dependency"
        );
    }

    for krate in INTERFACE_CRATES {
        let dir = crates_dir.join(krate);
        let manifest = read_toml(&dir.join("Cargo.toml"));
        for (section, deps) in dependency_tables(&manifest) {
            for (key, value) in deps {
                assert_ne!(
                    package_name(key, value),
                    LIBRARY,
                    "crates/{krate} depends on mochimo-crypto (as `{key}`) in [{section}]; the \
                     interface talks to wallet-core only"
                );
            }
        }
        for path in rust_files(&dir) {
            let source = read(&path);
            let mut found = Vec::new();
            idents(
                source
                    .parse::<proc_macro2::TokenStream>()
                    .unwrap_or_else(|e| panic!("{} does not lex: {e}", path.display())),
                &mut found,
            );
            assert!(
                !found.iter().any(|i| i == LIBRARY_IDENT),
                "{} names mochimo_crypto; the interface talks to wallet-core only",
                path.display()
            );
        }
    }

    // wallet-core must not hand the whole library through to the interface,
    // from any file the source scan reads.
    for (path, parsed) in scanned_sources(&crates_dir.join("wallet-core")) {
        let Parsed::File(file) = parsed else { continue };
        assert!(
            !reexports_library(&file.items),
            "{} re-exports mochimo_crypto wholesale; the interface would then reach the library \
             around the worker",
            path.display()
        );
    }
}

/// The re-export detector above, held to fixed cases.
#[test]
fn the_reexport_detector_sees_what_it_must() {
    let flagged = [
        "pub use mochimo_crypto;",
        "pub use mochimo_crypto as lib;",
        "pub use mochimo_crypto::*;",
        "pub use ::mochimo_crypto;",
        "pub use mochimo_crypto::{self};",
        "pub use mochimo_crypto::{self as lib, keystore};",
        "pub use mochimo_crypto::{*};",
        "pub use r#mochimo_crypto;",
        "pub extern crate mochimo_crypto;",
        "pub mod inner { pub use mochimo_crypto; }",
    ];
    for source in flagged {
        let file = syn::parse_file(source).expect("test source parses");
        assert!(
            reexports_library(&file.items),
            "the detector missed a wholesale re-export: {source}"
        );
    }
    let allowed = [
        "use mochimo_crypto;",
        "pub(crate) use mochimo_crypto;",
        "pub use mochimo_crypto::keystore::Kdf;",
        "pub use mochimo_crypto::{keystore, wallet};",
    ];
    for source in allowed {
        let file = syn::parse_file(source).expect("test source parses");
        assert!(
            !reexports_library(&file.items),
            "the detector flagged something that is not a wholesale re-export: {source}"
        );
    }
}

/// The strings of the TOML array at `value`, in order. `what` names the array
/// in a failure.
fn string_list(value: Option<&toml::Value>, what: &str) -> Vec<String> {
    value
        .and_then(|v| v.as_array())
        .unwrap_or_else(|| panic!("{what} is not an array"))
        .iter()
        .map(|v| {
            v.as_str()
                .unwrap_or_else(|| panic!("{what} holds a value that is not a string"))
                .to_owned()
        })
        .collect()
}

/// The licences the third-party notices may name, beyond deny.toml's allow
/// list, and the crates each is accepted for: the Mochimo agreement for the
/// library and Tawara's crates, and OFL-1.1 for the fonts tawara-app bundles.
/// tawara-app's code is under the Mochimo agreement too, which deny.toml
/// reads for it; about.toml names the crate under OFL-1.1 so that the font
/// licences are printed.
const NOTICES_EXCEPTIONS: [(&str, &str); 4] = [
    ("mochimo-crypto", "LicenseRef-Mochimo-Engine-1.0"),
    ("tawara-wallet-core", "LicenseRef-Mochimo-Engine-1.0"),
    ("tawara-desktop", "LicenseRef-Mochimo-Engine-1.0"),
    ("tawara-app", "OFL-1.1"),
];

/// The notices accept exactly the licences the policy gate admits, and the
/// named exceptions above, each for its own crate.
/// `about.toml` is checked against `deny.toml` directly: a licence accepted
/// there and refused here would reach the notices without the gate having
/// seen it, and one admitted here and not there would stop the generator for
/// a crate the gate has cleared. A per-crate `accepted` list is a widening of
/// its own, so every one of them is held to the table, and a new one fails
/// until the table names it.
#[test]
fn the_notices_accept_exactly_the_licences_the_policy_admits() {
    let root = workspace_root();
    let deny = read_toml(&root.join("deny.toml"));
    let about = read_toml(&root.join("about.toml"));

    let mut allowed = string_list(
        deny.get("licenses").and_then(|l| l.get("allow")),
        "deny.toml's [licenses] allow",
    );
    let mut accepted = string_list(about.get("accepted"), "about.toml's accepted");
    allowed.sort();
    accepted.sort();
    assert_eq!(
        accepted, allowed,
        "about.toml accepts {accepted:?} and deny.toml admits {allowed:?}; the two lists must \
         be the same"
    );

    let mut per_crate: Vec<(String, Vec<String>)> = about
        .iter()
        .filter_map(|(name, value)| {
            let accepted = value.as_table()?.get("accepted")?;
            Some((
                name.clone(),
                string_list(Some(accepted), &format!("about.toml's [{name}] accepted")),
            ))
        })
        .collect();
    per_crate.sort();
    let mut expected: Vec<(String, Vec<String>)> = NOTICES_EXCEPTIONS
        .iter()
        .map(|(name, licence)| ((*name).to_owned(), vec![(*licence).to_owned()]))
        .collect();
    expected.sort();
    assert_eq!(
        per_crate, expected,
        "about.toml's per-crate accepted lists must be exactly NOTICES_EXCEPTIONS; a new \
         exception has to be added to NOTICES_EXCEPTIONS deliberately"
    );
}

/// The committed notices end with LICENSE.md, byte for byte, as ci/notices.sh
/// appends it, since cargo-about 0.9.2 prints no text for a `LicenseRef-`
/// licence. They hold no licence template's placeholder, which would read as
/// a copyright notice naming nobody, and no path of the machine that
/// generated them.
#[test]
fn the_committed_notices_end_with_the_agreement_and_hold_no_placeholder() {
    let root = workspace_root();
    let notices = fs::read(root.join("THIRD-PARTY-NOTICES.txt"))
        .expect("THIRD-PARTY-NOTICES.txt is committed at the root");
    let licence = fs::read(root.join("LICENSE.md")).expect("LICENSE.md is committed at the root");
    assert!(
        notices.ends_with(&licence),
        "THIRD-PARTY-NOTICES.txt does not end with LICENSE.md, byte for byte; run \
         bash ci/notices.sh"
    );
    let text = String::from_utf8_lossy(&notices).to_lowercase();
    for marker in [
        "<year>",
        "<copyright holders>",
        "<owner>",
        "/users/",
        "/home/",
        "/.cargo/",
    ] {
        assert!(
            !text.contains(marker),
            "THIRD-PARTY-NOTICES.txt contains {marker:?}"
        );
    }
    assert!(
        !notices.contains(&b'\r'),
        "THIRD-PARTY-NOTICES.txt holds a carriage return; ci/notices.sh removes them"
    );
}

/// The crate versions that publish no text of their MIT licence, in their
/// package or in their repository at the commit they were published from, and
/// so are given the standard text without its template line for the holder
/// (about.toml names them). A crate added to that entry, or a new version of
/// one of these, has to be looked up again before it joins this list: it may
/// publish a text that names its holder, which about.toml then has to take.
const STANDARD_MIT_TEXT: [&str; 11] = [
    "block 0.1.6",
    "block2 0.6.2",
    "dispatch 0.2.0",
    "dispatch2 0.3.1",
    "objc2 0.6.4",
    "objc2-app-kit 0.3.2",
    "objc2-core-foundation 0.3.2",
    "objc2-core-graphics 0.3.2",
    "objc2-encode 4.1.0",
    "objc2-foundation 0.3.2",
    "objc2-quartz-core 0.3.2",
];

/// The committed notices give the standard MIT text to the crates in
/// STANDARD_MIT_TEXT and to no others.
#[test]
fn only_the_listed_crates_get_the_standard_mit_text() {
    let notices = read(&workspace_root().join("THIRD-PARTY-NOTICES.txt"));
    let marker = "They publish none in their repository either";
    let entries: Vec<&str> = notices
        .split(
            "\n================================================================================\n",
        )
        .filter(|entry| entry.contains(marker))
        .collect();
    assert_eq!(
        entries.len(),
        1,
        "THIRD-PARTY-NOTICES.txt should have one standard MIT entry, and has {}",
        entries.len()
    );
    let mut named: Vec<String> = entries[0]
        .lines()
        .skip_while(|line| *line != "Used by:")
        .skip(1)
        .take_while(|line| line.starts_with("  "))
        .map(|line| {
            line.split_whitespace()
                .take(2)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect();
    named.sort();
    let mut expected: Vec<String> = STANDARD_MIT_TEXT.iter().map(|s| (*s).to_owned()).collect();
    expected.sort();
    assert_eq!(
        named, expected,
        "the crates given the standard MIT text are not STANDARD_MIT_TEXT; look each new one \
         up in its repository first"
    );
}
