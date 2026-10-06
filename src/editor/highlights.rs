//! Fixes / overrides for GPUI Kit tree-sitter highlighters.
//!
//! GraphQL ships in the kit with an empty highlights query, so `.graphql`/`.gql`
//! parse but paint as plain text. We re-register the same grammar with our query.

use gpui_kit::component::highlighter::{GrammarConfig, LanguageRegistry};

/// Call once after `gpui_kit::init`.
pub fn init() {
    patch_graphql_highlights();
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
