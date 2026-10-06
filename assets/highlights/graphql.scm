; GraphQL highlights for joowani/tree-sitter-graphql (GPUI Kit's grammar).
; Captures use names HighlightTheme understands (with dotted-prefix fallback).
; Adapted from nvim-treesitter / Helix GraphQL queries.

; Types
(scalar_type_definition (name) @type)
(object_type_definition (name) @type)
(interface_type_definition (name) @type)
(union_type_definition (name) @type)
(enum_type_definition (name) @type)
(input_object_type_definition (name) @type)
(scalar_type_extension (name) @type)
(object_type_extension (name) @type)
(interface_type_extension (name) @type)
(union_type_extension (name) @type)
(enum_type_extension (name) @type)
(input_object_type_extension (name) @type)
(named_type (name) @type)

; Directives
(directive_definition "@" @attribute (name) @attribute)
(directive) @attribute

; Fields / properties
(field (name) @property)
(field (alias (name) @property))
(field_definition (name) @property)
(object_value (object_field (name) @property))
(enum_value (name) @property)

; Variables / arguments
(operation_definition (name) @variable)
(fragment_name (name) @variable)
(input_fields_definition (input_value_definition (name) @variable.special))
(argument (name) @variable.special)
(arguments_definition (input_value_definition (name) @variable.special))
(variable_definition (variable) @variable.special)
(argument (value (variable) @variable))

; Literals
(string_value) @string
(int_value) @number
(float_value) @number
(boolean_value) @boolean
(description (string_value) @comment.doc)
(comment) @comment

; Keywords
[
  "query"
  "mutation"
  "subscription"
  "fragment"
  "scalar"
  "input"
  "extend"
  "directive"
  "schema"
  "on"
  "repeatable"
  "implements"
  "enum"
  "union"
  "type"
  "interface"
] @keyword

; Punctuation / operators
["(" ")" "[" "]" "{" "}"] @punctuation.bracket
"=" @operator
["|" "&" ":"] @punctuation.delimiter
["..." "!"] @punctuation.special
