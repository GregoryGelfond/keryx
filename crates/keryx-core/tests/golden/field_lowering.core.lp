%!camel_field : sample -> string  (total)
#defined camel_field/2.
%!pascal_field : sample -> string  (total)
#defined pascal_field/2.
%!reach_ : sample -> int32  (total)
#defined reach_/2.
%!Field names lower like sorts and enums (§4.2): a camelCase or PascalCase proto field name is
%! `lower_snake`d (`camelField` -> `camel_field`, `PascalField` -> `pascal_field`), and a field
%! colliding with a generated-infrastructure identifier is reserved-word escaped (`reach` ->
%! `reach_`), the escape recorded in the manifest (§13.4).
%!sort sample/1
#defined sample/1.
