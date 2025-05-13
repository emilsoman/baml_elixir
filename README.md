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
  use BamlElixir.Client, path: Application.app_dir(:my_app, "priv/baml_src")
end
```

Now call the BAML function:

```elixir
MyApp.BamlClient.ExtractResume.call(%{resume: "John Doe is the CTO of Acme Inc."})
```

### Stream results

```elixir
MyApp.BamlClient.ExtractResume.stream(%{resume: "John Doe is the CTO of Acme Inc."}, fn
  {:ok, result} ->
    IO.inspect(result)

  {:error, error} ->
    IO.inspect(error)

  :done ->
    IO.inspect("Done")
end)
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

## Installation

Add baml_elixir to your mix.exs:

```elixir
def deps do
  [
    {:baml_elixir, "~> 1.0.0-pre.12"}
  ]
end
```

This also downloads the pre built NIFs for these targets:

- aarch64-apple-darwin (Apple Silicon)
- x86_64-unknown-linux-gnu

If you need to build the NIFs for other targets, you need to clone the repo and build it locally as documented below.

### TODO

- Type aliases
- Dynamic types
- Partial types for streaming

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
