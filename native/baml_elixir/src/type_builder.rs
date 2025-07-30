use crate::Error;
use baml_runtime::type_builder::TypeBuilder;
use baml_types::{ir_type::UnionConstructor, LiteralValue, TypeIR};
use rustler::{Encoder, Env, MapIterator, Term};

pub fn parse_type_builder_spec<'a>(
    env: Env<'a>,
    term: Term<'a>,
    builder: &TypeBuilder,
) -> Result<(), Error> {
    if term.is_map() {
        let iter = MapIterator::new(term).ok_or(Error::Term(Box::new("Invalid map")))?;
        for (key_term, value_term) in iter {
            if !key_term.is_tuple() {
                return Err(Error::Term(Box::new("Type builder keys must be tuples: {:class, \"ClassName\"} or {:enum, \"EnumName\"}")));
            }

            let tuple: (rustler::Atom, Term) = key_term.decode()?;
            let atom_str = tuple
                .0
                .encode(env)
                .atom_to_string()
                .map_err(|_| Error::Term(Box::new("Invalid atom")))?;

            match atom_str.as_str() {
                "class" => {
                    let class_name = term_to_string(tuple.1)?;
                    let cls = builder.class(&class_name);
                    let cls = cls.lock().unwrap();

                    if value_term.is_map() {
                        let field_iter = MapIterator::new(value_term)
                            .ok_or(Error::Term(Box::new("Invalid class spec map")))?;
                        for (field_name_term, field_type_term) in field_iter {
                            let field_name = term_to_string(field_name_term)?;
                            let property = cls.property(&field_name);
                            let property = property.lock().unwrap();
                            let field_type = parse_field_type(
                                env,
                                field_type_term,
                                builder,
                                Some(&class_name),
                                Some(&field_name),
                            )?;
                            property.r#type(field_type);
                        }
                    } else {
                        return Err(Error::Term(Box::new(
                            "Class values must be maps of field definitions",
                        )));
                    }
                }
                "enum" => {
                    let enum_name = term_to_string(tuple.1)?;
                    let enum_def = builder.r#enum(&enum_name);
                    let enum_def = enum_def.lock().unwrap();

                    if value_term.is_list() {
                        let variants: Vec<Term> = value_term.decode()?;
                        for variant in variants {
                            let variant_str = term_to_string(variant)?;
                            enum_def.value(&variant_str);
                        }
                    } else {
                        return Err(Error::Term(Box::new(
                            "Enum values must be lists of string variants",
                        )));
                    }
                }
                _ => {
                    return Err(Error::Term(Box::new(format!(
                        "Unsupported type builder key: {}",
                        atom_str
                    ))));
                }
            }
        }
    } else {
        return Err(Error::Term(Box::new(
            "Type builder specification must be a map",
        )));
    }
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
        // For nested objects, create an anonymous class
        let map_iter =
            MapIterator::new(term).ok_or(Error::Term(Box::new("Invalid map for object type")))?;

        // Generate a unique class name for this anonymous class
        let anonymous_class_name = if let (Some(parent), Some(field)) = (parent_class, field_name) {
            format!("{}_{}", parent, field)
        } else {
            "anonymous_class".to_string()
        };

        // Create the anonymous class in the type builder
        let cls = builder.class(&anonymous_class_name);
        let cls = cls.lock().unwrap();

        for (key_term, value_term) in map_iter {
            let fname = term_to_string(key_term)?;
            let property = cls.property(&fname);
            let property = property.lock().unwrap();
            let ftype = parse_field_type(
                env,
                value_term,
                builder,
                Some(&anonymous_class_name),
                Some(&fname),
            )?;
            property.r#type(ftype.clone());
        }

        Ok(TypeIR::class(&anonymous_class_name))
    } else {
        Err(Error::Term(Box::new("Unsupported field type")))
    }
}

// Helper function to convert a Term to a String
fn term_to_string(term: Term) -> Result<String, Error> {
    if term.is_atom() {
        term.atom_to_string()
            .map_err(|_| Error::Term(Box::new("Invalid atom")))
    } else {
        term.decode::<String>()
            .map_err(|_| Error::Term(Box::new("Invalid string")))
    }
}
