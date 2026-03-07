# BamlElixir

Call BAML functions from Elixir, using a Rust NIF.

## First of all, can this be used in production?

Well, I use it in production. But it's way too early for you if you expect stable APIs
and things to not break at all. If you're okay with debugging issues with me when things go wrong,
please go ahead!

What this library does:

- Generates Elixir structs, types and functions from BAML files.
- Gives you autocomplete and dialyzer type checking.
- Parses BAML results into Elixir structs.
- Switch between different LLM clients.
- Get usage data using collectors.

What this library does not do:

- Generate Elixir `baml_client` files from BAML files. Codegen happens at compile time.
- Automatically parse BAML results into Elixir structs.

## Usage

Create a baml_src directory in priv and add a BAML file in there:

```baml
client GPT4 {
    provider openai
    options {
        model gpt-4o-mini
        api_key env.OPENAI_API_KEY
    }
}

class Resume {
    name string
    job_title string
    company string
}

function ExtractResume(resume: string) -> Resume {
    client GPT4
    prompt #"
        {{ _.role('system') }}

        Extract the following information from the resume:

        Resume:
        <<<<
        {{ resume }}
        <<<<

        Output JSON schema:
        {{ ctx.output_format }}

        JSON:
    "#
}
```

Now create a BAML client module:

```elixir
defmodule MyApp.BamlClient do
  # {:my_app, "priv/baml_src"} Will be expanded to Application.app_dir(:my_app, "priv/baml_src")
  use BamlElixir.Client, path: {:my_app, "priv/baml_src"}
end
```

Now call the BAML function:

```elixir
MyApp.BamlClient.ExtractResume.call(%{resume: "John Doe is the CTO of Acme Inc."})
```

### Stream results

```elixir
MyApp.BamlClient.ExtractResume.stream(%{resume: "John Doe is the CTO of Acme Inc."}, fn
  {:partial, result} ->
    IO.inspect(result)

  {:done, result} ->
    IO.inspect(result)

  {:error, error} ->
    IO.inspect(error)
end)
```

You can also use `sync_stream` to get partial results and block until the function is done.

```elixir
case MyApp.BamlClient.ExtractResume.sync_stream(
        %{resume: "John Doe is the CTO of Acme Inc."},
        fn result ->
          IO.inspect(result)
        end
      ) do
  {:ok, result} ->
    IO.inspect(result)

  {:error, error} ->
    IO.inspect(error)
end
```

### Images

Send an image URL:

```elixir
MyApp.BamlClient.DescribeImage.call(%{
  myImg: %{
    url: "https://upload.wikimedia.org/wikipedia/en/4/4d/Shrek_%28character%29.png"
  }
})
|> IO.inspect()
```

Or send base64 encoded image data:

```elixir
MyApp.BamlClient.DescribeImage.stream(%{
  myImg: %{
    base64: "data:image/png;base64,..."
  }
}, fn result ->
  IO.inspect(result)
end)
```

### Collect usage data

```elixir
collector = BamlElixir.Collector.new("my_collector")

MyApp.BamlClient.ExtractResume.call(%{resume: "John Doe is the CTO of Acme Inc."}, %{
  collectors: [collector]
})

BamlElixir.Collector.usage(collector)
```

When streaming, you can get the usage after :done message is received.

### Switch LLM clients

From the existing list of LLM clients, you can switch to a different one by calling `Client.use_llm_client/2`.

```elixir
MyApp.BamlClient.WhichModel.call(%{}, %{
  llm_client: "GPT4oMini"
})
|> IO.inspect()
# => "gpt-4o-mini"

MyApp.BamlClient.WhichModel.call(%{}, %{
  llm_client: "DeepSeekR1"
})
|> IO.inspect()
# => "deepseek-r1"
```

### Type Builder

You can provide a type builder to dynamically define types at runtime. This is useful for classes with `@@dynamic` attributes or when you need to create types that aren't defined in your BAML files.

The type builder is a list of TypeBuilder structs:

#### Example

Given this BAML file:

```baml
class DynamicEmployee {
    employee_id string
    @@dynamic // allows adding fields dynamically at runtime
}
```

```elixir
{:ok,
%{
  __baml_class__: "DynamicEmployee",
  employee_id: _,
  person: %{
    name: "Foobar123",
    age: _,
    children_count: _,
    favorite_day: _,
    favorite_color: :RED,
    __baml_class__: "TestPerson"
  }
}} =
  BamlElixirTest.CreateEmployee.call(%{}, %{
    tb: [
      %TypeBuilder.Class{
        name: "TestPerson",
        fields: [
          %TypeBuilder.Field{
            name: "name",
            type: :string,
            description: "The name of the person - this should always be Foobar123"
          },
          %TypeBuilder.Field{name: "age", type: :int},
          %TypeBuilder.Field{name: "children_count", type: 1},
          %TypeBuilder.Field{name: "favorite_day", type: %TypeBuilder.Union{types: ["sunday", "monday"]}},
          %TypeBuilder.Field{name: "favorite_color", type: %TypeBuilder.Enum{name: "FavoriteColor"}}
        ]
      },
      %TypeBuilder.Class{
        name: "DynamicEmployee",
        fields: [
          %TypeBuilder.Field{name: "person", type: %TypeBuilder.Class{name: "TestPerson"}}
        ]
      },
      %TypeBuilder.Enum{
        name: "FavoriteColor",
        values: [
          %TypeBuilder.EnumValue{value: "RED", description: "Pick this always"},
          %TypeBuilder.EnumValue{value: "GREEN"},
          %TypeBuilder.EnumValue{value: "BLUE"}
        ]
      }
    ]
  })
```

**Note**: Classes with dynamic fields are not parsed into structs. They return a map with a `__baml_class__` key which can be used for pattern matching.

## Installation

Add baml_elixir to your mix.exs:

```elixir
def deps do
  [
    {:baml_elixir, "~> 1.0.0-pre.24"}
  ]
end
```

This also downloads the pre built NIFs for these targets:

- aarch64-apple-darwin (Apple Silicon)
- x86_64-unknown-linux-gnu (Linux x86_64)
- aarch64-unknown-linux-gnu (Linux ARM64)
- x86_64-unknown-linux-musl (Alpine Linux x86_64)
- aarch64-unknown-linux-musl (Alpine Linux ARM64)

If you need to build the NIFs for other targets, you need to clone the repo and build it locally as documented below.

### TODO

- Type aliases
- Dynamic types (WIP, works partially)
- Stream cancellation
- Add support for audio, PDF, and video output types
- Stream metadata exposure (`@stream.done`, `@stream.not_null`, `@stream.with_state`)
- OnTick callbacks
- Structured error types (replace `{:error, String.t()}` with structured error types like `BamlValidationError`)
- Runtime strategy configuration via Elixir

### Development

This project includes Git submodules. To clone the repository with all its submodules, use:

```bash
git clone --recurse-submodules <repository-url>
```

If you've already cloned the repository without submodules, initialize them with:

```bash
git submodule init
git submodule update
```

The project includes Rust code in the `native/` directory:

- `native/baml_elixir/` - Main Rust NIF code
- `native/baml_elixir/baml/` - Submodule containing baml which is a dependency of the NIF

### Building

1. Ensure you have Rust installed (https://rustup.rs/). Can use asdf to install it.
2. Build the project:

```bash
mix deps.get
mix compile
```
