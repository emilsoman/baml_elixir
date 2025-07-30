use baml_runtime::client_registry::ClientRegistry;
use baml_runtime::tracingv2::storage::storage::Collector;
use baml_runtime::type_builder::TypeBuilder;
use baml_runtime::{BamlRuntime, FunctionResult, RuntimeContextManager};
use baml_types::ir_type::UnionTypeViewGeneric;
use baml_types::{BamlMap, BamlValue, LiteralValue, TypeIR};

use collector::{FunctionLog, Usage};
use rustler::{
    Encoder, Env, Error, LocalPid, MapIterator, NifResult, NifStruct, ResourceArc, Term,
};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
mod atoms {
    rustler::atoms! {
        ok,
        error,
        nil,
        partial,
        done,
    }
}

mod collector;
mod type_builder;

fn term_to_string(term: Term) -> Result<String, Error> {
    if term.is_atom() {
        term.atom_to_string().map(|s| s.to_owned())
    } else {
        term.decode()
    }
}

fn term_to_baml_value<'a>(term: Term<'a>) -> Result<BamlValue, Error> {
    if term.is_number() {
        if let Ok(int) = term.decode::<i64>() {
            return Ok(BamlValue::Int(int));
        }
        if let Ok(float) = term.decode::<f64>() {
            return Ok(BamlValue::Float(float));
        }
    }

    if let Ok(string) = term.decode::<String>() {
        return Ok(BamlValue::String(string));
    }

    if let Ok(list) = term.decode::<Vec<Term>>() {
        let mut baml_list = Vec::new();
        for item in list {
            baml_list.push(term_to_baml_value(item)?);
        }
        return Ok(BamlValue::List(baml_list));
    }

    if term.is_map() {
        let mut map = BamlMap::new();
        for (key_term, value_term) in
            MapIterator::new(term).ok_or(Error::Term(Box::new("Invalid map")))?
        {
            let key = term_to_string(key_term)?;
            let value = term_to_baml_value(value_term)?;
            map.insert(key, value);
        }
        return Ok(BamlValue::Map(map));
    }

    if term.is_atom() && term.decode::<rustler::Atom>()? == atoms::nil() {
        return Ok(BamlValue::Null);
    }

    Err(Error::Term(Box::new(format!(
        "Unsupported type: {:?}",
        term
    ))))
}

fn baml_value_to_term<'a>(env: Env<'a>, value: &BamlValue) -> NifResult<Term<'a>> {
    match value {
        BamlValue::String(s) => Ok(s.encode(env)),
        BamlValue::Int(i) => Ok(i.encode(env)),
        BamlValue::Float(f) => Ok(f.encode(env)),
        BamlValue::Bool(b) => Ok(b.encode(env)),
        BamlValue::Null => Ok(atoms::nil().encode(env)),
        BamlValue::List(items) => {
            let terms: Result<Vec<Term>, Error> = items
                .iter()
                .map(|item| baml_value_to_term(env, item))
                .collect();
            Ok(terms?.encode(env))
        }
        BamlValue::Map(map) => {
            let mut result_map = Term::map_new(env);
            for (key, value) in map.iter() {
                let value_term = baml_value_to_term(env, value)?;
                result_map = result_map
                    .map_put(key.encode(env), value_term)
                    .map_err(|_| Error::Term(Box::new("Failed to add key to map")))?;
            }
            Ok(result_map)
        }
        BamlValue::Class(class_name, map) => {
            let mut result_map = Term::map_new(env);
            let class_atom = rustler::Atom::from_str(env, "__baml_class__")
                .map_err(|_| Error::Term(Box::new("Failed to create atom")))?;
            result_map = result_map
                .map_put(class_atom.encode(env), class_name.encode(env))
                .map_err(|_| Error::Term(Box::new("Failed to add class name")))?;
            for (key, value) in map.iter() {
                let key_atom = rustler::Atom::from_str(env, key)
                    .map_err(|_| Error::Term(Box::new("Failed to create key atom")))?;
                let value_term = baml_value_to_term(env, value)?;
                result_map = result_map
                    .map_put(key_atom.encode(env), value_term)
                    .map_err(|_| Error::Term(Box::new("Failed to add key to map")))?;
            }
            Ok(result_map)
        }
        BamlValue::Media(_media) => {
            // For now, return an error since we need to check the actual BamlMedia structure
            Err(Error::Term(Box::new("Media type not yet supported")))
        }
        BamlValue::Enum(enum_type, variant) => {
            // Convert enum to a map with __baml_enum__ and value
            let mut result_map = Term::map_new(env);
            let enum_atom = rustler::Atom::from_str(env, "__baml_enum__")
                .map_err(|_| Error::Term(Box::new("Failed to create enum atom")))?;
            let value_atom = rustler::Atom::from_str(env, "value")
                .map_err(|_| Error::Term(Box::new("Failed to create value atom")))?;
            result_map = result_map
                .map_put(enum_atom.encode(env), enum_type.encode(env))
                .map_err(|_| Error::Term(Box::new("Failed to add enum type")))?;
            result_map = result_map
                .map_put(value_atom.encode(env), variant.encode(env))
                .map_err(|_| Error::Term(Box::new("Failed to add enum variant")))?;
            Ok(result_map)
        }
    }
}

#[derive(NifStruct)]
#[module = "BamlElixir.Client"]
struct Client<'a> {
    from: String,
    client_registry: Term<'a>,
    collectors: Vec<ResourceArc<collector::CollectorResource>>,
}

fn prepare_request<'a>(
    env: Env<'a>,
    args: Term<'a>,
    path: String,
    collectors: Vec<ResourceArc<collector::CollectorResource>>,
    client_registry: Term<'a>,
    tb_elixir: Term<'a>,
) -> Result<
    (
        BamlRuntime,
        BamlMap<String, BamlValue>,
        RuntimeContextManager,
        Option<Vec<Arc<Collector>>>,
        Option<ClientRegistry>,
        Option<TypeBuilder>,
    ),
    Error,
> {
    let runtime = match BamlRuntime::from_directory(&Path::new(&path), std::env::vars().collect()) {
        Ok(r) => r,
        Err(e) => return Err(Error::Term(Box::new(e.to_string()))),
    };

    // Convert args to BamlMap
    let mut params = BamlMap::new();
    if args.is_map() {
        let iter = MapIterator::new(args).ok_or(Error::Term(Box::new("Invalid map")))?;
        for (key_term, value_term) in iter {
            let key = term_to_string(key_term)?;
            let value = term_to_baml_value(value_term)?;
            params.insert(key.clone(), value);
        }
    } else {
        return Err(Error::Term(Box::new("Arguments must be a map")));
    }

    // Create context
    let ctx = runtime.create_ctx_manager(
        BamlValue::String("elixir".to_string()),
        None, // baml source reader
    );

    let collectors = if collectors.is_empty() {
        None
    } else {
        Some(collectors.iter().map(|c| c.inner.clone()).collect())
    };

    let client_registry = if client_registry.is_atom()
        && client_registry.decode::<rustler::Atom>()? == atoms::nil()
    {
        None
    } else if client_registry.is_map() {
        let mut registry = ClientRegistry::new();
        let iter = MapIterator::new(client_registry)
            .ok_or(Error::Term(Box::new("Invalid registry map")))?;
        for (key_term, value_term) in iter {
            let key = term_to_string(key_term)?;
            if key == "primary" {
                let primary = term_to_string(value_term)?;
                registry.set_primary(primary);
            }
        }
        Some(registry)
    } else {
        return Err(Error::Term(Box::new(
            "Client registry must be nil or a map",
        )));
    };

    let tb = if tb_elixir.is_map() {
        let builder = TypeBuilder::new();

        // Use the parse_type_builder_spec function from type_builder module
        if let Err(e) = type_builder::parse_type_builder_spec(env, tb_elixir, &builder) {
            return Err(e);
        }

        Some(builder)
    } else {
        None
    };

    Ok((runtime, params, ctx, collectors, client_registry, tb))
}

fn parse_function_result_call<'a>(env: Env<'a>, result: FunctionResult) -> NifResult<Term<'a>> {
    let parsed_value = result.parsed();
    match parsed_value {
        Some(Ok(response_baml_value)) => {
            let baml_value = response_baml_value.0.clone().value();
            let result_term = baml_value_to_term(env, &baml_value)?;
            Ok((atoms::ok(), result_term).encode(env))
        }
        Some(Err(e)) => Ok((atoms::error(), format!("{:?}", e)).encode(env)),
        None => Ok((atoms::error(), "No parsed value available").encode(env)),
    }
}

fn parse_function_result_stream<'a>(
    env: Env<'a>,
    result: FunctionResult,
) -> Result<Term<'a>, String> {
    let parsed_value = result.parsed();
    match parsed_value {
        Some(Ok(response_baml_value)) => {
            let baml_value = response_baml_value.0.clone().value();
            let result_term = baml_value_to_term(env, &baml_value)
                .map_err(|e| format!("Failed to convert BAML value to term: {:?}", e))?;
            Ok(result_term)
        }
        Some(Err(e)) => Err(e.to_string()),
        None => Err("No parsed value available".to_string()),
    }
}

#[rustler::nif(schedule = "DirtyIo")]
fn call<'a>(
    env: Env<'a>,
    function_name: String,
    arguments: Term<'a>,
    path: String,
    collectors: Vec<ResourceArc<collector::CollectorResource>>,
    client_registry: Term<'a>,
    tb: Term<'a>,
) -> NifResult<Term<'a>> {
    let (runtime, params, ctx, collectors, client_registry, tb) =
        prepare_request(env, arguments, path, collectors, client_registry, tb)?;

    // Call function synchronously
    let (result, _trace_id) = runtime.call_function_sync(
        function_name,
        &params,
        &ctx,
        tb.as_ref(),              // type builder (optional)
        client_registry.as_ref(), // client registry (optional)
        collectors,
        std::env::vars().collect(),
    );

    // Handle result
    match result {
        Ok(function_result) => parse_function_result_call(env, function_result),
        Err(e) => Ok((atoms::error(), format!("{:?}", e)).encode(env)),
    }
}

#[rustler::nif(schedule = "DirtyIo")]
fn stream<'a>(
    env: Env<'a>,
    pid: Term<'a>,
    reference: Term<'a>,
    function_name: String,
    arguments: Term<'a>,
    path: String,
    collectors: Vec<ResourceArc<collector::CollectorResource>>,
    client_registry: Term<'a>,
    tb: Term<'a>,
) -> NifResult<Term<'a>> {
    let pid = pid.decode::<LocalPid>()?;
    let (runtime, params, ctx, collectors, client_registry, tb) =
        prepare_request(env, arguments, path, collectors, client_registry, tb)?;

    let on_event = |r: FunctionResult| {
        match parse_function_result_stream(env, r) {
            Ok(result_term) => {
                let wrapped_result = (reference, (atoms::partial(), result_term)).encode(env);
                let _ = env.send(&pid, wrapped_result);
            }
            Err(_) => {
                // Do nothing on error because this can happen when
                // the result cannot be coerced to a BAML value.
                // This can happen when the result is incomplete.
                // We'll get the final result and check for a real error then.
                return;
            }
        }
    };

    let result = runtime.stream_function(
        function_name,
        &params,
        &ctx,
        tb.as_ref(),
        client_registry.as_ref(),
        collectors,
        std::env::vars().collect(),
    );

    match result {
        Ok(mut stream) => {
            let (result, _trace_id) = stream.run_sync(
                None::<fn()>,
                Some(on_event),
                &ctx,
                None,
                None,
                std::env::vars().collect(),
            );
            match result {
                Ok(r) => match r.parsed() {
                    Some(Ok(result)) => {
                        let baml_value = result.0.clone().value();
                        let result_term = baml_value_to_term(env, &baml_value)?;
                        Ok((atoms::done(), result_term).encode(env))
                    }
                    Some(Err(e)) => Ok((atoms::error(), format!("{:?}", e)).encode(env)),
                    None => Ok((atoms::error(), "No parsed value available").encode(env)),
                },
                Err(e) => Ok((atoms::error(), format!("{:?}", e)).encode(env)),
            }
        }
        Err(e) => Ok((atoms::error(), format!("{:?}", e)).encode(env)),
    }
}

#[rustler::nif]
fn collector_new(name: Option<String>) -> ResourceArc<collector::CollectorResource> {
    collector::CollectorResource::new(name)
}

#[rustler::nif]
fn collector_usage(collector: ResourceArc<collector::CollectorResource>) -> Usage {
    collector.usage()
}

#[rustler::nif]
fn collector_last_function_log(
    collector: ResourceArc<collector::CollectorResource>,
) -> Option<FunctionLog> {
    collector.last_function_log()
}

#[rustler::nif]
fn parse_baml(env: Env, path: Option<String>) -> NifResult<Term> {
    let path = path.unwrap_or_else(|| "baml_src".to_string());

    // Create runtime
    let runtime = match BamlRuntime::from_directory(&Path::new(&path), std::env::vars().collect()) {
        Ok(r) => r,
        Err(e) => return Err(Error::Term(Box::new(e.to_string()))),
    };

    let ir = runtime.inner.ir.clone();

    // Create a map of the classes and their fields along with their types
    let mut class_fields = HashMap::new();
    let mut class_attributes = HashMap::new();
    for class in ir.walk_classes() {
        let mut fields = HashMap::new();
        for field in class.walk_fields() {
            let field_type = to_elixir_type(env, &field.r#type());
            fields.insert(field.name().to_string(), field_type);
        }
        class_fields.insert(class.name().to_string(), fields);

        // Check if class has @@dynamic attribute
        let is_dynamic = class.item.attributes.get("dynamic_type").is_some();
        class_attributes.insert(class.name().to_string(), is_dynamic);
    }

    // Create a map of the enums and their variants
    let mut enum_variants = HashMap::new();
    for r#enum in ir.walk_enums() {
        let mut variants = Vec::new();
        for variant in r#enum.walk_values() {
            variants.push(variant.name().to_string());
        }
        enum_variants.insert(r#enum.name().to_string(), variants);
    }

    // Create a map of the functions and their parameters
    let mut function_params = HashMap::new();
    for function in ir.walk_functions() {
        let mut params = HashMap::new();

        // Get input parameters
        for (name, field_type) in function.inputs() {
            let param_type = to_elixir_type(env, field_type);
            params.insert(name.to_string(), param_type);
        }

        // Get return type
        let return_type = to_elixir_type(env, &function.output());

        function_params.insert(function.name().to_string(), (params, return_type));
    }

    // convert to elixir map term
    let mut map = Term::map_new(env);

    // Add classes
    let mut classes_map = Term::map_new(env);
    for (class_name, fields) in class_fields {
        let mut class_map = Term::map_new(env);

        // Add fields
        let mut field_map = Term::map_new(env);
        for (field_name, field_type) in fields {
            field_map = field_map.map_put(field_name.encode(env), field_type)?;
        }
        class_map = class_map.map_put("fields".encode(env), field_map)?;

        // Add dynamic attribute
        let is_dynamic = class_attributes.get(&class_name).unwrap_or(&false);
        class_map = class_map.map_put("dynamic".encode(env), is_dynamic.encode(env))?;

        classes_map = classes_map.map_put(class_name.encode(env), class_map)?;
    }
    map = map.map_put(
        rustler::Atom::from_str(env, "classes").unwrap().encode(env),
        classes_map,
    )?;

    // Add enums
    let mut enums_map = Term::map_new(env);
    for (enum_name, variants) in enum_variants {
        let variants_list = variants.encode(env);
        enums_map = enums_map.map_put(enum_name.encode(env), variants_list)?;
    }
    map = map.map_put(
        rustler::Atom::from_str(env, "enums").unwrap().encode(env),
        enums_map,
    )?;

    // Add functions
    let mut functions_map = Term::map_new(env);
    for (function_name, (params, return_type)) in function_params {
        let mut function_map = Term::map_new(env);

        // Add parameters
        let mut params_map = Term::map_new(env);
        for (param_name, param_type) in params {
            params_map = params_map.map_put(param_name.encode(env), param_type)?;
        }
        function_map = function_map.map_put("params".encode(env), params_map)?;

        // Add return type
        function_map = function_map.map_put("return_type".encode(env), return_type)?;

        functions_map = functions_map.map_put(function_name.encode(env), function_map)?;
    }
    map = map.map_put(
        rustler::Atom::from_str(env, "functions")
            .unwrap()
            .encode(env),
        functions_map,
    )?;

    Ok(map)
}

fn to_elixir_type<'a>(env: Env<'a>, field_type: &TypeIR) -> Term<'a> {
    match field_type {
        TypeIR::Enum { name, .. } => {
            // Return {:enum, name}
            (rustler::Atom::from_str(env, "enum").unwrap(), name).encode(env)
        }
        TypeIR::Class { name, .. } => {
            // Return {:class, name}
            (rustler::Atom::from_str(env, "class").unwrap(), name).encode(env)
        }
        TypeIR::List(inner, _) => {
            // Return {:list, inner_type}
            let inner_type = to_elixir_type(env, inner);
            (rustler::Atom::from_str(env, "list").unwrap(), inner_type).encode(env)
        }
        TypeIR::Map(key, value, _) => {
            // Return {:map, key_type, value_type}
            let key_type = to_elixir_type(env, key);
            let value_type = to_elixir_type(env, value);
            (
                rustler::Atom::from_str(env, "map").unwrap(),
                key_type,
                value_type,
            )
                .encode(env)
        }
        TypeIR::Primitive(r#type, _) => {
            // Return {:primitive, primitive_value}
            let primitive_value = match r#type {
                baml_types::TypeValue::String => rustler::Atom::from_str(env, "string").unwrap(),
                baml_types::TypeValue::Int => rustler::Atom::from_str(env, "integer").unwrap(),
                baml_types::TypeValue::Float => rustler::Atom::from_str(env, "float").unwrap(),
                baml_types::TypeValue::Bool => rustler::Atom::from_str(env, "boolean").unwrap(),
                baml_types::TypeValue::Null => atoms::nil(),
                baml_types::TypeValue::Media(_) => rustler::Atom::from_str(env, "media").unwrap(),
            };
            (
                rustler::Atom::from_str(env, "primitive").unwrap(),
                primitive_value,
            )
                .encode(env)
        }
        TypeIR::Literal(value, _) => {
            // Return {:literal, value}
            let literal_value = match value {
                LiteralValue::String(s) => rustler::Atom::from_str(env, &s).unwrap().encode(env),
                LiteralValue::Int(i) => i.encode(env),
                LiteralValue::Bool(b) => b.encode(env),
            };
            (
                rustler::Atom::from_str(env, "literal").unwrap(),
                literal_value,
            )
                .encode(env)
        }
        TypeIR::Union(inner, _) => match inner.view() {
            UnionTypeViewGeneric::Null => (atoms::nil()).encode(env),
            UnionTypeViewGeneric::Optional(inner) => {
                // Return {:optional, type}
                let inner_type = to_elixir_type(env, inner);
                (
                    rustler::Atom::from_str(env, "optional").unwrap(),
                    inner_type,
                )
                    .encode(env)
            }
            UnionTypeViewGeneric::OneOf(inner) => {
                // Return {:union, list_of_types}
                let types: Vec<Term> = inner.iter().map(|t| to_elixir_type(env, t)).collect();
                (rustler::Atom::from_str(env, "union").unwrap(), types).encode(env)
            }
            UnionTypeViewGeneric::OneOfOptional(inner) => {
                // Return {:optional, {:union, list_of_types}}
                let types: Vec<Term> = inner.iter().map(|t| to_elixir_type(env, t)).collect();
                (
                    rustler::Atom::from_str(env, "optional").unwrap(),
                    (rustler::Atom::from_str(env, "union").unwrap(), types),
                )
                    .encode(env)
            }
        },
        TypeIR::Tuple(inner, _) => {
            // Return {:tuple, list_of_types}
            let types: Vec<Term> = inner.iter().map(|t| to_elixir_type(env, t)).collect();
            (rustler::Atom::from_str(env, "tuple").unwrap(), types).encode(env)
        }
        TypeIR::RecursiveTypeAlias { name, .. } => {
            // Return {:alias, name}
            (rustler::Atom::from_str(env, "alias").unwrap(), name).encode(env)
        }
        TypeIR::Arrow(..) => {
            // Arrow types are not supported in Elixir type specs
            panic!("Arrow types are not supported in Elixir")
        }
    }
}

rustler::init!("Elixir.BamlElixir.Native");
