use crate::Error;
use baml_runtime::type_builder::TypeBuilder;
use baml_types::{ir_type::UnionConstructor, LiteralValue, TypeIR};
use rustler::{Encoder, Env, MapIterator, Term};

pub fn parse_type_builder_spec<'a>(
    env: Env<'a>,
    term: Term<'a>,
    builder: &TypeBuilder,
) -> Result<(), Error> {
    if !term.is_list() {
        return Err(Error::Term(Box::new(
            "TypeBuilder specification must be a list",
        )));
    }

    // New format: list of TypeBuilder structs
    let list: Vec<Term> = term.decode()?;
    for item in list {
        parse_type_builder_item(env, item, builder)?;
    }
    Ok(())
}

fn parse_type_builder_item<'a>(
    env: Env<'a>,
    term: Term<'a>,
    builder: &TypeBuilder,
) -> Result<(), Error> {
    if !term.is_map() {
        return Err(Error::Term(Box::new("TypeBuilder item must be a map")));
    }

    let iter = MapIterator::new(term).ok_or(Error::Term(Box::new("Invalid map")))?;
    let mut item_type = None;

    for (key_term, value_term) in iter {
        let key = term_to_string(key_term)?;
        match key.as_str() {
            "__struct__" => match term_to_string(value_term) {
                Ok(struct_name) => {
                    item_type = Some(struct_name);
                }
                Err(e) => {
                    return Err(e);
                }
            },
            _ => {
                // Ignore other fields for now
            }
        }
    }

    match item_type.as_deref() {
        Some("Elixir.BamlElixir.TypeBuilder.Class") => {
            parse_class_item(env, term, builder)?;
        }
        Some("Elixir.BamlElixir.TypeBuilder.Enum") => {
            parse_enum_item(env, term, builder)?;
        }
        Some(other) => {
            return Err(Error::Term(Box::new(format!(
                "Unsupported TypeBuilder struct: {}",
                other
            ))));
        }
        None => {
            return Err(Error::Term(Box::new("Missing __struct__ field")));
        }
    }

    Ok(())
}

fn parse_class_item<'a>(
    env: Env<'a>,
    class_term: Term<'a>,
    builder: &TypeBuilder,
) -> Result<(), Error> {
    if !class_term.is_map() {
        return Err(Error::Term(Box::new("Class data must be a map")));
    }

    let iter = MapIterator::new(class_term).ok_or(Error::Term(Box::new("Invalid class map")))?;
    let mut class_name = None;
    let mut fields = None;

    for (key_term, value_term) in iter {
        let key = term_to_string(key_term)?;
        match key.as_str() {
            "name" => {
                class_name = Some(term_to_string(value_term)?);
            }
            "fields" => {
                fields = Some(value_term);
            }
            _ => {}
        }
    }

    let class_name = class_name.ok_or(Error::Term(Box::new("Class missing name field")))?;
    let fields = fields.ok_or(Error::Term(Box::new("Class missing fields")))?;

    // Create the class in the type builder
    let cls = builder.class(&class_name);
    let cls = cls.lock().unwrap();

    if fields.is_list() {
        let field_list: Vec<Term> = fields.decode()?;
        for field_term in field_list {
            parse_field_item(env, field_term, builder, &class_name, &cls)?;
        }
    } else {
        return Err(Error::Term(Box::new("Class fields must be a list")));
    }

    Ok(())
}

fn parse_enum_item<'a>(
    _env: Env<'a>,
    enum_term: Term<'a>,
    builder: &TypeBuilder,
) -> Result<(), Error> {
    if !enum_term.is_map() {
        return Err(Error::Term(Box::new("Enum data must be a map")));
    }

    let iter = MapIterator::new(enum_term).ok_or(Error::Term(Box::new("Invalid enum map")))?;
    let mut enum_name = None;
    let mut values = None;

    for (key_term, value_term) in iter {
        let key = term_to_string(key_term)?;
        match key.as_str() {
            "name" => {
                enum_name = Some(term_to_string(value_term)?);
            }
            "values" => {
                values = Some(value_term);
            }
            _ => {}
        }
    }

    let enum_name = enum_name.ok_or(Error::Term(Box::new("Enum missing name field")))?;
    let values = values.ok_or(Error::Term(Box::new("Enum missing values")))?;

    // Create the enum in the type builder
    let enum_builder = builder.r#enum(&enum_name);
    let enum_builder = enum_builder.lock().unwrap();

    if values.is_list() {
        let value_list: Vec<Term> = values.decode()?;
        for value_term in value_list {
            let value_name = term_to_string(value_term)?;
            enum_builder.value(&value_name);
        }
    } else {
        return Err(Error::Term(Box::new("Enum values must be a list")));
    }

    Ok(())
}

fn parse_field_item<'a>(
    env: Env<'a>,
    field_term: Term<'a>,
    builder: &TypeBuilder,
    parent_class: &str,
    cls: &std::sync::MutexGuard<baml_runtime::type_builder::ClassBuilder>,
) -> Result<(), Error> {
    if !field_term.is_map() {
        return Err(Error::Term(Box::new("Field must be a map")));
    }

    let iter = MapIterator::new(field_term).ok_or(Error::Term(Box::new("Invalid field map")))?;
    let mut field_name = None;
    let mut field_type = None;

    for (key_term, value_term) in iter {
        let key = term_to_string(key_term)?;
        match key.as_str() {
            "name" => {
                field_name = Some(term_to_string(value_term)?);
            }
            "type" => {
                field_type = Some(value_term);
            }
            _ => {}
        }
    }

    let field_name = field_name.ok_or(Error::Term(Box::new("Missing field name")))?;
    let field_type_term = field_type.ok_or(Error::Term(Box::new("Missing field type")))?;

    let type_ir = parse_field_type(
        env,
        field_type_term,
        builder,
        Some(parent_class),
        Some(&field_name),
    )?;

    // Add the field to the class
    let property = cls.property(&field_name);
    let property = property.lock().unwrap();
    property.r#type(type_ir);

    Ok(())
}

fn parse_field_type<'a>(
    env: Env<'a>,
    term: Term<'a>,
    builder: &TypeBuilder,
    parent_class: Option<&str>,
    field_name: Option<&str>,
) -> Result<TypeIR, Error> {
    if term.is_atom() {
        let atom_str = term
            .atom_to_string()
            .map_err(|_| Error::Term(Box::new("Invalid atom")))?;

        match atom_str.as_str() {
            "string" => Ok(TypeIR::string()),
            "int" => Ok(TypeIR::int()),
            "float" => Ok(TypeIR::float()),
            "bool" => Ok(TypeIR::bool()),
            _ => Ok(TypeIR::class(&atom_str)),
        }
    } else if let Ok(string_value) = term.decode::<String>() {
        // Handle string literals like "1", "hello", etc.
        Ok(TypeIR::literal(LiteralValue::String(string_value)))
    } else if let Ok(int_value) = term.decode::<i64>() {
        // Handle integer literals like 1, 42, etc.
        Ok(TypeIR::literal(LiteralValue::Int(int_value)))
    } else if let Ok(bool_value) = term.decode::<bool>() {
        // Handle boolean literals like true, false
        Ok(TypeIR::literal(LiteralValue::Bool(bool_value)))
    } else if term.is_tuple() {
        // Handle tuple-based types like {:class, "TestPerson"}, {:union, ["alive", "dead"]}, {:enum, "FavoriteColor"}
        // First, try to decode as a 3-tuple for map types
        if let Ok((atom, key_type_term, value_type_term)) =
            term.decode::<(rustler::Atom, Term, Term)>()
        {
            let atom_str = atom
                .encode(env)
                .atom_to_string()
                .map_err(|_| Error::Term(Box::new("Invalid atom")))?;

            if atom_str == "map" {
                let key_type =
                    parse_field_type(env, key_type_term, builder, parent_class, field_name)?;
                let value_type =
                    parse_field_type(env, value_type_term, builder, parent_class, field_name)?;
                return Ok(TypeIR::map(key_type, value_type));
            }
        }

        // Handle 2-tuple types like {:class, "TestPerson"}, {:union, ["alive", "dead"]}, {:enum, "FavoriteColor"}
        let tuple: (rustler::Atom, Term) = term.decode()?;
        let atom_str = tuple
            .0
            .encode(env)
            .atom_to_string()
            .map_err(|_| Error::Term(Box::new("Invalid atom")))?;
        match atom_str.as_str() {
            "class" => {
                let class_name = term_to_string(tuple.1)?;
                Ok(TypeIR::class(&class_name))
            }
            "union" => {
                let variants: Vec<Term> = tuple.1.decode()?;
                let mut union_types = Vec::new();
                for variant in variants {
                    if variant.is_atom() {
                        let variant_str = variant.atom_to_string()?;
                        union_types.push(TypeIR::literal(LiteralValue::String(variant_str)));
                    } else if let Ok(variant_str) = variant.decode::<String>() {
                        union_types.push(TypeIR::literal(LiteralValue::String(variant_str)));
                    } else {
                        return Err(Error::Term(Box::new(
                            "Union variants must be atoms or strings",
                        )));
                    }
                }
                Ok(TypeIR::union(union_types))
            }
            "enum" => {
                let enum_name = term_to_string(tuple.1)?;
                Ok(TypeIR::r#enum(&enum_name))
            }
            _ => Err(Error::Term(Box::new(format!(
                "Unsupported tuple type: {}",
                atom_str
            )))),
        }
    } else if term.is_list() {
        let list: Vec<Term> = term.decode()?;
        if list.is_empty() {
            return Err(Error::Term(Box::new("Empty list type not supported")));
        }
        let first_item = &list[0];
        // Recursively parse the first item to determine the list element type
        let inner_type = parse_field_type(env, *first_item, builder, parent_class, field_name)?;
        Ok(TypeIR::list(inner_type))
    } else if term.is_map() {
        // Check if this is a TypeBuilder struct
        let iter =
            MapIterator::new(term).ok_or(Error::Term(Box::new("Invalid map for object type")))?;
        let mut struct_type = None;

        for (key_term, value_term) in iter {
            let key = term_to_string(key_term)?;
            match key.as_str() {
                "__struct__" => {
                    let struct_name = term_to_string(value_term)?;
                    struct_type = Some(struct_name);
                }
                _ => {}
            }
        }

        match struct_type.as_deref() {
            Some("Elixir.BamlElixir.TypeBuilder.Class") => {
                parse_class_item(env, term, builder)?;
                // Return the class type
                let iter =
                    MapIterator::new(term).ok_or(Error::Term(Box::new("Invalid class map")))?;
                for (key_term, value_term) in iter {
                    let key = term_to_string(key_term)?;
                    if key == "name" {
                        let class_name = term_to_string(value_term)?;
                        return Ok(TypeIR::class(&class_name));
                    }
                }
                Err(Error::Term(Box::new("Could not extract class name")))
            }
            Some("Elixir.BamlElixir.TypeBuilder.List") => {
                // Extract the inner type from the list
                let iter =
                    MapIterator::new(term).ok_or(Error::Term(Box::new("Invalid list map")))?;
                for (key_term, value_term) in iter {
                    let key = term_to_string(key_term)?;
                    if key == "type" {
                        let inner_type =
                            parse_field_type(env, value_term, builder, parent_class, field_name)?;
                        return Ok(TypeIR::list(inner_type));
                    }
                }
                Err(Error::Term(Box::new("Could not extract list inner type")))
            }
            Some("Elixir.BamlElixir.TypeBuilder.Map") => {
                // Extract key and value types from the map
                let mut key_type = None;
                let mut value_type = None;

                let iter =
                    MapIterator::new(term).ok_or(Error::Term(Box::new("Invalid map map")))?;
                for (key_term, value_term) in iter {
                    let key = term_to_string(key_term)?;
                    match key.as_str() {
                        "key_type" => {
                            key_type = Some(parse_field_type(
                                env,
                                value_term,
                                builder,
                                parent_class,
                                field_name,
                            )?);
                        }
                        "value_type" => {
                            value_type = Some(parse_field_type(
                                env,
                                value_term,
                                builder,
                                parent_class,
                                field_name,
                            )?);
                        }
                        _ => {}
                    }
                }

                if let (Some(key), Some(value)) = (key_type, value_type) {
                    return Ok(TypeIR::map(key, value));
                }
                Err(Error::Term(Box::new(
                    "Could not extract map key and value types",
                )))
            }
            Some("Elixir.BamlElixir.TypeBuilder.Enum") => {
                parse_enum_item(env, term, builder)?;
                // Return the enum type
                let iter =
                    MapIterator::new(term).ok_or(Error::Term(Box::new("Invalid enum map")))?;
                for (key_term, value_term) in iter {
                    let key = term_to_string(key_term)?;
                    if key == "name" {
                        let enum_name = term_to_string(value_term)?;
                        return Ok(TypeIR::r#enum(&enum_name));
                    }
                }
                Err(Error::Term(Box::new("Could not extract enum name")))
            }
            _ => Err(Error::Term(Box::new(format!(
                "Unsupported TypeBuilder struct: {:?}",
                struct_type
            )))),
        }
    } else {
        Err(Error::Term(Box::new("Unsupported field type")))
    }
}

// Helper function to convert a Term to a String
fn term_to_string(term: Term) -> Result<String, Error> {
    if term.is_atom() {
        term.atom_to_string()
            .map_err(|_| Error::Term(Box::new("Invalid atom")))
    } else if let Ok(string_value) = term.decode::<String>() {
        Ok(string_value)
    } else {
        Err(Error::Term(Box::new("Term is not a string or atom")))
    }
}
