defmodule BamlElixir.Client do
  @moduledoc """
  A client for interacting with BAML functions.

  This module provides functionality to call BAML functions either synchronously or as a stream.

  ## Examples

      # Create a client
      client = %BamlElixir.Client{}

      # Call a function synchronously
      {:ok, result} = BamlElixir.Client.call(client, "MyFunction", %{arg1: "value"})

      # Stream function results
      stream = BamlElixir.Client.stream!(client, "MyFunction", %{arg1: "value"})
      Enum.each(stream, fn result -> IO.inspect(result) end)
  """

  defstruct from: "baml_src"

  @doc """
  Calls a BAML function synchronously.

  ## Parameters
    - `client`: The BAML client struct
    - `function_name`: The name of the BAML function to call
    - `args`: A map of arguments to pass to the function

  ## Returns
    - `{:ok, term()}` on success, where the term is the function's return value
    - `{:error, String.t()}` on failure, with an error message

  ## Examples
      {:ok, result} = BamlElixir.Client.call(client, "MyFunction", %{arg1: "value"})
  """
  @spec call(%__MODULE__{}, String.t(), map()) :: {:ok, term()} | {:error, String.t()}
  def call(%__MODULE__{} = client, function_name, args) do
    BamlElixir.Native.call(client, function_name, args)
  end

  @doc """
  Calls a BAML function and returns a stream of results as tokens are generated by an LLM.

  ## Parameters
    - `client`: The BAML client struct
    - `function_name`: The name of the BAML function to call
    - `args`: A map of arguments to pass to the function

  ## Returns
    - A stream of results

  ## Examples
      stream = BamlElixir.Client.stream!(client, "MyFunction", %{arg1: "value"})
      Enum.each(stream, fn result -> IO.inspect(result) end)
  """
  @spec stream!(%__MODULE__{}, String.t(), map()) :: Enumerable.t()
  def stream!(client, function_name, args) do
    Stream.resource(
      fn ->
        pid = self()

        spawn_link(fn ->
          send(pid, BamlElixir.Native.stream(client, pid, function_name, args))
        end)
      end,
      fn _ ->
        receive do
          {:ok, result} -> {[result], nil}
          :done -> {:halt, :done}
          {:error, reason} -> raise reason
        end
      end,
      fn _ -> :ok end
    )
  end
end
