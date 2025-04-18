use baml_runtime::{BamlRuntime, FunctionResult, RuntimeContextManager};
use baml_types::{BamlMap, BamlValue};
use rustler::{Encoder, Env, Error, LocalPid, MapIterator, NifResult, NifStruct, Term};
use std::path::Path;

mod atoms {
    rustler::atoms! {
        ok,
        error,
        nil,
        done,
    }
}

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

    Err(Error::Term(Box::new("Unsupported type")))
}

fn baml_value_to_term<'a>(env: Env<'a>, value: &BamlValue, client: &Client) -> NifResult<Term<'a>> {
    match value {
        BamlValue::String(s) => Ok(s.encode(env)),
        BamlValue::Int(i) => Ok(i.encode(env)),
        BamlValue::Float(f) => Ok(f.encode(env)),
        BamlValue::Bool(b) => Ok(b.encode(env)),
        BamlValue::Null => Ok(atoms::nil().encode(env)),
        BamlValue::List(items) => {
            let terms: Result<Vec<Term>, Error> = items
                .iter()
                .map(|item| baml_value_to_term(env, item, client))
                .collect();
            Ok(terms?.encode(env))
        }
        BamlValue::Map(map) | BamlValue::Class(_, map) => {
            let mut result_map = Term::map_new(env);
            for (key, value) in map.iter() {
                let value_term = baml_value_to_term(env, value, client)?;
                result_map = result_map
                    .map_put(key.encode(env), value_term)
                    .map_err(|_| Error::Term(Box::new("Failed to add key to map")))?;
            }
            Ok(result_map)
        }
        BamlValue::Media(_media) => {
            // For now, return an error since we need to check the actual BamlMedia structure
            Err(Error::Term(Box::new("Media type not yet supported")))
        }
        BamlValue::Enum(enum_type, variant) => {
            // Convert enum to a map with type and variant
            let mut result_map = Term::map_new(env);
            result_map = result_map
                .map_put("type".encode(env), enum_type.encode(env))
                .map_err(|_| Error::Term(Box::new("Failed to add enum type")))?;
            result_map = result_map
                .map_put("variant".encode(env), variant.encode(env))
                .map_err(|_| Error::Term(Box::new("Failed to add enum variant")))?;
            Ok(result_map)
        }
    }
}

#[derive(Debug, NifStruct)]
#[module = "BamlElixir.Client"]
struct Client {
    from: String,
}

fn prepare_runtime_and_params<'a>(
    client: Term<'a>,
    args: Term<'a>,
) -> Result<
    (
        Client,
        BamlRuntime,
        BamlMap<String, BamlValue>,
        RuntimeContextManager,
    ),
    Error,
> {
    let client = client.decode::<Client>()?;

    // Get from client or default to "baml_src"
    let from_directory = if client.from.is_empty() {
        "baml_src".to_string()
    } else {
        client.from.clone()
    };

    // Create runtime
    let runtime = match BamlRuntime::from_directory(
        &Path::new(&from_directory),
        std::env::vars().collect(),
    ) {
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

    Ok((client, runtime, params, ctx))
}

fn parse_function_result<'a>(
    env: Env<'a>,
    result: FunctionResult,
    client: &Client,
) -> NifResult<Term<'a>> {
    let parsed_value = result.parsed();
    match parsed_value {
        Some(Ok(response_baml_value)) => {
            let baml_value = response_baml_value.0.clone().value();
            let result_term = baml_value_to_term(env, &baml_value, client)?;
            Ok((atoms::ok(), result_term).encode(env))
        }
        Some(Err(e)) => Ok((atoms::error(), format!("{:?}", e)).encode(env)),
        None => Ok((atoms::error(), "No parsed value available").encode(env)),
    }
}

#[rustler::nif(schedule = "DirtyIo")]
fn call<'a>(
    env: Env<'a>,
    client: Term<'a>,
    function_name: String,
    args: Term<'a>,
) -> NifResult<Term<'a>> {
    let (client, runtime, params, ctx) = prepare_runtime_and_params(client, args)?;

    // Call function synchronously
    let (result, _trace_id) = runtime.call_function_sync(
        function_name,
        &params,
        &ctx,
        None, // type builder (optional)
        None, // client registry (optional)
        None, // collectors (optional)
    );

    // Handle result
    match result {
        Ok(function_result) => parse_function_result(env, function_result, &client),
        Err(e) => Ok((atoms::error(), format!("{:?}", e)).encode(env)),
    }
}

#[rustler::nif(schedule = "DirtyIo")]
fn stream<'a>(
    env: Env<'a>,
    client: Term<'a>,
    pid: Term<'a>,
    function_name: String,
    args: Term<'a>,
) -> NifResult<Term<'a>> {
    let pid = pid.decode::<LocalPid>()?;
    let (client, runtime, params, ctx) = prepare_runtime_and_params(client, args)?;

    let on_event = |r: FunctionResult| {
        let result_term = parse_function_result(env, r, &client).unwrap();
        let _ = env.send(&pid, result_term);
    };

    let result = runtime.stream_function(function_name, &params, &ctx, None, None, None);

    match result {
        Ok(mut stream) => {
            let (result, _trace_id) = stream.run_sync(Some(on_event), &ctx, None, None);
            match result {
                Ok(_) => Ok(atoms::done().encode(env)),
                Err(e) => Ok((atoms::error(), format!("{:?}", e)).encode(env)),
            }
        }
        Err(e) => Ok((atoms::error(), format!("{:?}", e)).encode(env)),
    }
}

rustler::init!("Elixir.BamlElixir.Native");
