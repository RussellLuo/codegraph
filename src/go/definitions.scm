(source_file
  (type_declaration (
    type_spec
      name: (type_identifier) @definition.class.name
  )) @definition.class
)

(function_declaration
  name: (identifier) @definition.function.name
  parameters: (parameter_list)
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
) @definition.function

(method_declaration
  receiver: (parameter_list (
    parameter_declaration
      type: [
        (type_identifier) @definition.method.receiver_type
        (pointer_type (type_identifier) @definition.method.receiver_type)
      ]
  ))
  name: (field_identifier) @definition.method.name
) @definition.method