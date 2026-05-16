; PaddleBoard: dropped the upstream test-prefix capture block — it produced
; a capture name that PaddleBoard's textobjects loader rejects as unknown
; (it only recognizes function/class/comment text objects), and the
; same matching logic already lives in runnables.scm where it belongs.

(function_declaration
  body: (_) @function.inside) @function.around
