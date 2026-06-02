(package_header
  "package" @context
  (identifier) @name) @item

(class_declaration
  (modifiers)? @context
  (type_identifier) @name) @item

(object_declaration
  "object" @context
  (type_identifier) @name) @item

(type_alias
  "typealias" @context
  (type_identifier) @name) @item

(enum_entry
  (simple_identifier) @name) @item

(function_declaration
  "fun" @context
  (simple_identifier) @name) @item

; PaddleBoard: dropped the `["val" "var"] @context` matcher — the
; codanna grammar models `property_declaration` with only named
; children (no anonymous `val`/`var` tokens visible to queries), so the
; original pattern is "impossible" and fails query loading. Property
; names still appear in the outline, just without the val/var prefix.
(property_declaration
  (variable_declaration
    (simple_identifier) @name)) @item

(property_declaration
  (multi_variable_declaration
    (variable_declaration
      (simple_identifier) @name) @item))

(companion_object
  "companion" @context
  "object" @context
  (type_identifier)? @name) @item

(secondary_constructor
  "constructor" @name) @item

(anonymous_initializer
  "init" @name) @item
