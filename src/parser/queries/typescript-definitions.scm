; Pattern 0: Import Statements
(import_statement
  (import_clause [
    (
      namespace_import (identifier) @reference.namespace_import.alias
    )
    (
      named_imports (
        import_specifier [
          name: (identifier) @reference.named_import.name
          alias: (identifier) @reference.named_import.alias
        ]
      )
    )
    (
      (identifier) @reference.default_import.alias
    )
  ])
  source: (
    string (string_fragment) @reference.import.source
  )
)

; Pattern 1: Interface Declarations
(interface_declaration
  name: (type_identifier) @definition.interface.name
  body: (interface_body)
) @definition.interface

; Pattern 2: Class Declarations
(class_declaration
  name: (type_identifier) @definition.class.name
  body: (class_body)
) @definition.class

; Pattern 3: Function Declarations
(function_declaration
  name: (identifier) @definition.function.name
  parameters: (
    (formal_parameters
      [
        (required_parameter
          type: (_) @definition.function.param_type
        )?
        (optional_parameter
          type: (_) @definition.function.param_type
        )?
      ]
    )
  )
  return_type: (
    type_annotation (
      [
        (predefined_type)
        (type_identifier)
        (tuple_type)
        (generic_type)
      ]
    )
  )
  body: (statement_block) @definition.function.body
) @definition.function

; Pattern 4: Enum Declarations
(enum_declaration
  name: (identifier) @definition.enum.name
  body: (enum_body)
) @definition.enum

; Pattern 5: Type Alias Declarations
(type_alias_declaration
  name: (type_identifier) @definition.type_alias.name
) @definition.type_alias