(function_declaration
  body: (_) @function.inside) @function.around

(test_declaration (_) (block) @test.inside) @test.around

; matches all of: struct, enum, union
; this unfortunately cannot be split up because
; of the way struct "container" types are defined

(variable_declaration
  (struct_declaration
    "struct"
    "{"
    _* @class.inside
    "}")) @class.around

(variable_declaration
  (enum_declaration
    "{"
    _* @class.inside
    "}")) @class.around

(variable_declaration
  (union_declaration
    "{"
    _* @class.inside
    "}")) @class.around

(call_expression
  function: (_)
  "("
  (
    (_) @parameter.inside
    .
    ","? @parameter.around) @parameter.around
  ")")

(comment) @comment.inside
(comment)+ @comment.around
