//! Fixes / overrides for GPUI Kit tree-sitter highlighters.
//!
//! - GraphQL ships in the kit with an empty highlights query, so `.graphql`/`.gql`
//!   parse but paint as plain text. We re-register the same grammar with our query.
//! - TSX ships with `tree_sitter_typescript::HIGHLIGHTS_QUERY` only (a short TS
//!   delta, not a full query). We re-register with Kit's TypeScript highlights
//!   plus JSX captures, and reuse TypeScript's injection languages.

use gpui_kit::component::highlighter::{GrammarConfig, LanguageRegistry};

/// Call once after `gpui_kit::init`.
pub fn init() {
    patch_graphql_highlights();
    patch_tsx_highlights();
}

fn patch_graphql_highlights() {
    let registry = LanguageRegistry::singleton();
    let Some(existing) = registry.language("graphql") else {
        log::warn!("graphql language missing from LanguageRegistry; skip highlight patch");
        return;
    };
    let Some(language) = existing.language.clone() else {
        log::warn!("graphql grammar missing; skip highlight patch");
        return;
    };
    registry.register(
        "graphql",
        &GrammarConfig::new(
            "graphql",
            language,
            existing.injection_languages,
            include_str!("../../assets/highlights/graphql.scm"),
            "",
            "",
        ),
    );
}

fn patch_tsx_highlights() {
    let registry = LanguageRegistry::singleton();
    let Some(tsx) = registry.language("tsx") else {
        log::warn!("tsx language missing from LanguageRegistry; skip highlight patch");
        return;
    };
    let Some(typescript) = registry.language("typescript") else {
        log::warn!("typescript language missing from LanguageRegistry; skip tsx highlight patch");
        return;
    };
    let Some(language) = tsx.language.clone() else {
        log::warn!("tsx grammar missing; skip highlight patch");
        return;
    };

    let highlights = compose_tsx_highlights(typescript.highlights.as_str());
    let locals = if tsx.locals.is_empty() {
        typescript.locals.as_str()
    } else {
        tsx.locals.as_str()
    };

    registry.register(
        "tsx",
        &GrammarConfig::new(
            "tsx",
            language,
            // Kit leaves TSX injection languages empty; reuse TypeScript's
            // (jsdoc / css / html / … template-tag injections).
            typescript.injection_languages.clone(),
            &highlights,
            typescript.injections.as_str(),
            locals,
        ),
    );
}

/// TypeScript highlights first, then JSX tag/attribute captures.
fn compose_tsx_highlights(typescript_highlights: &str) -> String {
    format!(
        "{typescript_highlights}\n{}",
        include_str!("../../assets/highlights/tsx_jsx.scm")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tsx_highlights_include_typescript_body_and_jsx_tags() {
        let q = compose_tsx_highlights("(identifier) @variable\n");
        assert!(
            q.contains("(identifier) @variable"),
            "must keep TypeScript highlights"
        );
        assert!(
            q.contains("jsx_opening_element"),
            "must append JSX captures"
        );
        assert!(
            q.contains("@constructor"),
            "must mark capitalized JSX components"
        );
    }
}
