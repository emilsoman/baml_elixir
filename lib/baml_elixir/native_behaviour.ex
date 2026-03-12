defmodule BamlElixir.NativeBehaviour do
  @moduledoc """
  Behaviour for the Native NIF module, enabling mocking in tests.
  """

  @callback create_tripwire() :: reference()
  @callback abort_tripwire(reference()) :: :ok
  @callback stream(
              pid(),
              reference(),
              reference(),
              String.t(),
              map(),
              String.t(),
              list(),
              map() | nil,
              list() | nil
            ) :: any()
  @callback call(String.t(), map(), String.t(), list(), map() | nil, list() | nil) ::
              {:ok, any()} | {:error, String.t()}
end
