; JSX captures for the TSX grammar (tree-sitter-typescript LANGUAGE_TSX).
; Appended to GPUI Kit's TypeScript highlights query — Kit registers TSX with
; only tree_sitter_typescript::HIGHLIGHTS_QUERY (a TS-keyword delta), so .tsx
; files otherwise paint nearly as plain text.
; Adapted from Helix runtime/queries/_jsx/highlights.scm; captures use names
; HighlightTheme understands (with dotted-prefix fallback).

; Punctuation
; -----------

(jsx_attribute "=" @punctuation.delimiter)

(jsx_opening_element ["<" ">"] @punctuation.bracket)

(jsx_closing_element ["</" ">"] @punctuation.bracket)

(jsx_self_closing_element ["<" "/>"] @punctuation.bracket)

; Attributes
; ----------

(jsx_attribute (property_identifier) @attribute)

; Opening elements
; ----------------

(jsx_opening_element (identifier) @tag)

(jsx_opening_element ((identifier) @constructor
 (#match? @constructor "^[A-Z]")))

; Closing elements
; ----------------

(jsx_closing_element (identifier) @tag)

(jsx_closing_element ((identifier) @constructor
 (#match? @constructor "^[A-Z]")))

; Self-closing elements
; ---------------------

(jsx_self_closing_element (identifier) @tag)

(jsx_self_closing_element ((identifier) @constructor
 (#match? @constructor "^[A-Z]")))
