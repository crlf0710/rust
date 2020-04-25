use crate::{EarlyContext, EarlyLintPass, LintContext};
use rustc_ast::ast;
use rustc_data_structures::fx::FxHashMap;

declare_lint! {
    pub NON_ASCII_IDENTS,
    Allow,
    "detects non-ASCII identifiers"
}

declare_lint! {
    pub UNCOMMON_CODEPOINTS,
    Warn,
    "detects uncommon Unicode codepoints in identifiers"
}

declare_lint! {
    pub CONFUSABLE_IDENTS,
    Warn,
    "detects uncommon Unicode codepoints in identifiers"
}

declare_lint_pass!(NonAsciiIdents => [NON_ASCII_IDENTS, UNCOMMON_CODEPOINTS, CONFUSABLE_IDENTS]);

fn calc_skeleton(symbol_str: &str) -> String {
    use unicode_security::confusable_detection::skeleton;
    skeleton(symbol_str).collect()
}

impl EarlyLintPass for NonAsciiIdents {
    fn check_crate(&mut self, cx: &EarlyContext<'_>, _: &ast::Crate) {
        let mut symbol_strs_and_spans = Vec::new();
        let symbols = cx.sess.parse_sess.symbol_gallery.symbols.lock();
        for (symbol, sp) in symbols.iter() {
            let symbol_str = symbol.as_str();
            symbol_strs_and_spans.push((symbol_str, *sp));
        }
        drop(symbols);
        symbol_strs_and_spans.sort_by_key(|x| x.0.clone());
        let mut skeleton_map = FxHashMap::default();
        for (symbol_str, sp) in symbol_strs_and_spans {
            let skeleton = calc_skeleton(&symbol_str);
            skeleton_map
                .entry(skeleton)
                .and_modify(|(existing_symbolstr, existing_span)| {
                    cx.struct_span_lint(CONFUSABLE_IDENTS, sp, |lint| {
                        lint.build(&format!(
                            "identifier pair considered confusable between `{}` and `{}`",
                            existing_symbolstr, symbol_str
                        ))
                        .span_label(
                            *existing_span,
                            "this is where the previous identifier occurred",
                        )
                        .emit();
                    });
                })
                .or_insert((symbol_str, sp));
        }
    }
    fn check_ident(&mut self, cx: &EarlyContext<'_>, ident: ast::Ident) {
        use unicode_security::GeneralSecurityProfile;
        let name_str = ident.name.as_str();
        if name_str.is_ascii() {
            return;
        }
        cx.struct_span_lint(NON_ASCII_IDENTS, ident.span, |lint| {
            lint.build("identifier contains non-ASCII characters").emit()
        });
        if !name_str.chars().all(GeneralSecurityProfile::identifier_allowed) {
            cx.struct_span_lint(UNCOMMON_CODEPOINTS, ident.span, |lint| {
                lint.build("identifier contains uncommon Unicode codepoints").emit()
            })
        }
    }
}
