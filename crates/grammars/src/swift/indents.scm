; PaddleBoard: rewritten in Zed's @indent / @end capture model.
; The upstream zed-extensions/swift extension used Helix-style
; capture names (@indent.begin, @indent.branch, @indent.dedent,
; @indent.end, @indent.auto, @indent.ignore) which this fork's
; tree-sitter query layer in `language_core::grammar` does not
; understand — it warns on unknown captures and falls back to no
; indentation rules at all. The patterns below cover the common
; cases (anything bracket-delimited) the same way rust/indents.scm
; does.

(_
  "{"
  "}" @end) @indent

(_
  "("
  ")" @end) @indent

(_
  "["
  "]" @end) @indent

(_
  "<"
  ">" @end) @indent
