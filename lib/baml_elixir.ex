defmodule BamlElixir do
  @baml_version "0.205.0"

  def baml_version, do: @baml_version

  @doc """
  Clears the runtime cache. This forces BAML files to be re-read from disk
  on the next function call. Useful when BAML files are updated and you want
  to ensure the changes are picked up immediately.
  """
  def clear_runtime_cache do
    BamlElixir.Native.clear_runtime_cache()
  end
end
