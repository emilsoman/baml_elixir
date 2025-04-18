defmodule BamlElixir.Native do
  version = Mix.Project.config()[:version]

  use RustlerPrecompiled,
    otp_app: :baml_elixir,
    base_url: "https://github.com/emilsoman/baml_elixir/releases/download/v#{version}/",
    force_build: System.get_env("BAML_ELIXIR_BUILD") in ["1", "true"],
    version: version,
    targets: [
      "aarch64-apple-darwin",
      "x86_64-unknown-linux-gnu"
    ]

  def call(_client, _function_name, _args), do: :erlang.nif_error(:nif_not_loaded)

  def stream(_client, _pid, _function_name, _args), do: :erlang.nif_error(:nif_not_loaded)
end
