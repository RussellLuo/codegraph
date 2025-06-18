; Pattern 0: Import Declarations
(import_declaration
  [
    (import_spec_list
      (import_spec) @reference.import.path
    )
    (import_spec) @reference.import.path
  ]
)

; Pattern 1: Interface Declarations
(type_declaration (
  (type_spec
    name: (type_identifier) @definition.interface.name
    type: (interface_type (_))
  ) @definition.interface
))

; Pattern 2: Class Declarations (i.e. Go Structs)
(type_declaration (
  (type_spec
    name: (type_identifier) @definition.class.name
    type: (struct_type (_))
  ) @definition.class
))

; Pattern 3: Function Declarations
(function_declaration
  name: (identifier) @definition.function.name
  parameters: (
    (parameter_list
      [
        (parameter_declaration
          type: (_) @definition.function.param_type
        )?
        (variadic_parameter_declaration
          type: (_) @definition.function.param_type
        )?
      ]
    )
  )
  result: [
    (parameter_list
      (parameter_declaration
        type: [
          (type_identifier) @definition.function.first_return_type
          (pointer_type (type_identifier) @definition.function.first_return_type)
        ]
      )+
    )
    (
      [
        (type_identifier) @definition.function.first_return_type
        (pointer_type (type_identifier) @definition.function.first_return_type)
      ]*
    )
  ]
  body: (block) @definition.function.body
) @definition.function

; Pattern 4: Method Declarations
(method_declaration
  receiver: (parameter_list (
    parameter_declaration
      type: [
        (type_identifier) @definition.method.receiver_type
        (pointer_type (type_identifier) @definition.method.receiver_type)
      ]
  ))
  name: (field_identifier) @definition.method.name
  parameters: (
    (parameter_list
      [
        (parameter_declaration
          type: (_) @definition.method.param_type
        )?
        (variadic_parameter_declaration
          type: (_) @definition.method.param_type
        )?
      ]
    )
  )
  body: (block) @definition.method.body
) @definition.method