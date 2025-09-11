defmodule BamlElixir.Client do
  @moduledoc """
  A client for interacting with BAML functions.
  Data structures and functions are generated from BAML source files.

  > #### `use BamlElixir.Client, path: "priv/baml_src"` {: .info}
  >
  > When you `use BamlElixir.Client`, it will define:
  > - A module for each function in the BAML source files with `call/2` and `stream/3` functions along with the types.
  > - A module with `defstruct/1` and `@type t/0` for each class in the BAML source file.
  > - A module with `@type t/0` for each enum in the BAML source file.
  >
  > The `path` option is optional and defaults to `"baml_src"`, you may want to set it to `"priv/baml_src"`.

  This module also provides functionality to call BAML functions either sync/async.
  """

  defmacro __using__(opts) do
    path = Keyword.get(opts, :path, "baml_src")
    {baml_src_path, _} = Code.eval_quoted(app_path(path), [], __CALLER__)

    # Get all .baml files in the directory
    baml_files = get_baml_files(baml_src_path)

    # Add @external_resource for each BAML file to establish compile-time dependencies
    for baml_file <- baml_files do
      quote do
        @external_resource unquote(baml_file)
      end
    end

    # Get BAML types
    baml_types = BamlElixir.Native.parse_baml(baml_src_path)
    baml_class_types = baml_types[:classes]
    baml_enum_types = baml_types[:enums]
    baml_functions = baml_types[:functions]

    baml_class_types_quoted = generate_class_types(baml_class_types, __CALLER__)
    baml_enum_types_quoted = generate_enum_types(baml_enum_types, __CALLER__)
    baml_functions_quoted = generate_function_modules(baml_functions, path, __CALLER__)
    recompile_function = generate_recompile_function(baml_src_path, baml_files)

    quote do
      import BamlElixir.Client

      unquote(baml_class_types_quoted)
      unquote(baml_enum_types_quoted)
      unquote(baml_functions_quoted)
      unquote(recompile_function)
    end
  end

  @doc """
  Calls a BAML function synchronously.

  ## Parameters
    - `function_name`: The name of the BAML function to call
    - `args`: A map of arguments to pass to the function
    - `opts`: A map of options
      - `path`: The path to the BAML source file
      - `collectors`: A list of collectors to use
      - `llm_client`: The name of the LLM client to use

  ## Returns
    - `{:ok, term()}` on success, where the term is the function's return value
    - `{:error, String.t()}` on failure, with an error message

  ## Examples
      {:ok, result} = BamlElixir.Client.call(client, "MyFunction", %{arg1: "value"})
  """
  @spec call(String.t(), map(), map()) ::
          {:ok, term()} | {:error, String.t()}
  def call(function_name, args, opts \\ %{}) do
    {path, collectors, client_registry, tb} = prepare_opts(opts)
    args = to_map(args)

    with {:ok, result} <-
           BamlElixir.Native.call(function_name, args, path, collectors, client_registry, tb) do
      result =
        if opts[:parse] != false do
          parse_result(result, opts[:prefix], tb)
        else
          result
        end

      {:ok, result}
    end
  end

  @doc """
  Streams a BAML function asynchronously.

  ## Parameters
    - `function_name`: The name of the BAML function to stream
    - `args`: A map of arguments to pass to the function
    - `callback`: A function that will be called with the result of the function
    - `opts`: A map of options
      - `path`: The path to the BAML source file
      - `collectors`: A list of collectors to use
      - `llm_client`: The name of the LLM client to use

  """
  def stream(function_name, args, callback, opts \\ %{}) do
    ref = make_ref()
    args = to_map(args)

    spawn_link(fn ->
      start_sync_stream(self(), ref, function_name, args, opts)
      handle_stream_result(ref, callback, opts)
    end)
  end

  def app_path(path) do
    case path do
      {app, path} ->
        Application.app_dir(app, path)

      _ ->
        path
    end
  end

  # Get all .baml files in the specified directory
  def get_baml_files(baml_src_path) do
    if File.exists?(baml_src_path) and File.dir?(baml_src_path) do
      Path.wildcard(Path.join(baml_src_path, "**/*.baml"))
    else
      []
    end
  end

  # Create a hash from a list of file paths
  def create_files_hash(file_paths) do
    file_paths
    |> Enum.map(&File.stat!/1)
    |> Enum.map(fn stat -> {stat.mtime, stat.size} end)
    |> inspect()
    |> :erlang.md5()
  end

  # Generate the __mix_recompile__?/0 function that checks if any .baml files have changed
  defp generate_recompile_function(baml_src_path, baml_files) do
    # Create a hash of all BAML files at compile time
    files_hash = create_files_hash(baml_files)

    quote do
      def __mix_recompile__?() do
        baml_src_path = unquote(baml_src_path)

        # Check if the directory still exists
        if not File.exists?(baml_src_path) or not File.dir?(baml_src_path) do
          true
        else
          # Get current BAML files and compare hashes
          current_baml_files = BamlElixir.Client.get_baml_files(baml_src_path)
          current_files_hash = BamlElixir.Client.create_files_hash(current_baml_files)
          current_files_hash != unquote(files_hash)
        end
      end
    end
  end

  defp start_sync_stream(pid, ref, function_name, args, opts) do
    {path, collectors, client_registry, tb} = prepare_opts(opts)

    spawn_link(fn ->
      result =
        BamlElixir.Native.stream(
          pid,
          ref,
          function_name,
          args,
          path,
          collectors,
          client_registry,
          tb
        )

      send(pid, {ref, result})
    end)
  end

  defp handle_stream_result(ref, callback, opts) do
    receive do
      {^ref, {:partial, result}} ->
        result =
          if opts[:parse] != false do
            parse_result(result, opts[:prefix], opts[:tb])
          else
            result
          end

        callback.({:partial, result})
        handle_stream_result(ref, callback, opts)

      {^ref, {:error, _} = msg} ->
        callback.(msg)

      {^ref, {:done, result}} ->
        result =
          if opts[:parse] != false do
            parse_result(result, opts[:prefix], opts[:tb])
          else
            result
          end

        callback.({:done, result})
    end
  end

  # Every class in the BAML source file is converted to an Elixir module
  # with a `defstruct/1` and a `@type t/0` type.
  defp generate_class_types(class_types, caller) do
    module = caller.module

    for {type_name, %{"fields" => fields, "dynamic" => dynamic}} <- class_types do
      field_names = get_field_names(fields)
      field_types = get_field_types(fields, caller)
      module_name = Module.concat([module, type_name])

      quote do
        defmodule unquote(module_name) do
          defstruct unquote(field_names)
          @type t :: %__MODULE__{unquote_splicing(field_types)}

          def name, do: unquote(type_name)
          def type, do: :class
          def dynamic?, do: unquote(dynamic)
        end
      end
    end
  end

  # Every enum in the BAML source file is converted to an Elixir module
  # with a `@type t/0` type.
  defp generate_enum_types(enum_types, caller) do
    module = caller.module

    for {enum_name, variants} <- enum_types do
      variant_atoms = Enum.map(variants, &String.to_atom/1)
      module_name = Module.concat([module, enum_name])

      union_type =
        Enum.reduce(variant_atoms, fn atom, acc ->
          {:|, [], [atom, acc]}
        end)

      quote do
        defmodule unquote(module_name) do
          @type t :: unquote(union_type)

          def name, do: unquote(enum_name)
          def values, do: unquote(variant_atoms)
          def type, do: :enum
        end
      end
    end
  end

  # Every function in the BAML source file is converted to an Elixir module
  # which has a `call/2` function and a `stream/3` function.
  defp generate_function_modules(functions, path, caller) do
    module = caller.module

    for {function_name, function_info} <- functions do
      module_name = Module.concat(module, function_name)

      param_types =
        for {param_name, param_type} <- function_info["params"] do
          {String.to_atom(param_name), to_elixir_type(param_type, caller)}
        end

      return_type = to_elixir_type(function_info["return_type"], caller)

      quote do
        defmodule unquote(module_name) do
          @spec call(%{unquote_splicing(param_types)}, map()) ::
                  {:ok, unquote(return_type)} | {:error, String.t()}
          def call(args, opts \\ %{}) do
            opts =
              opts
              |> Map.put(:path, BamlElixir.Client.app_path(unquote(path)))
              |> Map.put(:prefix, unquote(module))

            BamlElixir.Client.call(unquote(function_name), args, opts)
          end

          @spec stream(
                  %{unquote_splicing(param_types)},
                  ({:ok, unquote(return_type) | {:error, String.t()} | :done} -> any()),
                  map()
                ) ::
                  Enumerable.t()
          def stream(args, callback, opts \\ %{}) do
            opts =
              opts
              |> Map.put(:path, BamlElixir.Client.app_path(unquote(path)))
              |> Map.put(:prefix, unquote(module))

            BamlElixir.Client.stream(unquote(function_name), args, callback, opts)
          end
        end
      end
    end
  end

  defp to_elixir_type(type, caller) do
    case type do
      {:primitive, primitive} ->
        case primitive do
          :string ->
            quote(do: String.t())

          :integer ->
            quote(do: integer())

          :float ->
            quote(do: float())

          :boolean ->
            quote(do: boolean())

          nil ->
            quote(do: nil)

          :media ->
            quote(
              do:
                %{url: String.t()}
                | %{url: String.t(), media_type: String.t()}
                | %{base64: String.t()}
                | %{base64: String.t(), media_type: String.t()}
            )
        end

      {:enum, name} ->
        # Convert enum name to module reference with .t()
        module = Module.concat([caller.module, name])
        quote(do: unquote(module).t())

      {:class, name} ->
        # Convert class name to module reference with .t()
        module = Module.concat([caller.module, name])
        quote(do: unquote(module).t())

      {:list, inner_type} ->
        # Convert to list type
        quote(do: [unquote(to_elixir_type(inner_type, caller))])

      {:map, key_type, value_type} ->
        # Convert to map type
        quote(
          do: %{
            unquote(to_elixir_type(key_type, caller)) =>
              unquote(to_elixir_type(value_type, caller))
          }
        )

      {:literal, value} ->
        # For literals, use the value directly
        case value do
          v when is_atom(v) -> v
          v when is_integer(v) -> v
          v when is_boolean(v) -> v
        end

      {:union, types} ->
        # Convert union to pipe operator
        [first_type | rest_types] = types
        first_ast = to_elixir_type(first_type, caller)

        Enum.reduce(rest_types, first_ast, fn type, acc ->
          {:|, [], [to_elixir_type(type, caller), acc]}
        end)

      {:tuple, types} ->
        # Convert to tuple type
        types_ast = Enum.map(types, &to_elixir_type(&1, caller))
        {:{}, [], types_ast}

      {:optional, inner_type} ->
        # Convert optional to union with nil
        {:|, [], [to_elixir_type(inner_type, caller), nil]}

      {:alias, name} ->
        # For recursive type aliases, use the name with .t()
        module = String.to_atom(name)
        quote(do: unquote(module).t())

      _ ->
        # Fallback to any
        quote(do: any())
    end
  end

  defp get_field_names(fields) do
    for {field_name, _} <- fields do
      String.to_atom(field_name)
    end
  end

  defp get_field_types(fields, caller) do
    for {field_name, field_type} <- fields do
      elixir_type = to_elixir_type(field_type, caller)
      {String.to_atom(field_name), elixir_type}
    end
  end

  defp prepare_opts(opts) do
    path = opts[:path] || "baml_src"
    collectors = (opts[:collectors] || []) |> Enum.map(fn collector -> collector.reference end)
    client_registry = opts[:llm_client] && %{primary: opts[:llm_client]}
    {path, collectors, client_registry, opts[:tb]}
  end

  # If type builder is provided, return as map instead of struct
  defp parse_result(%{:__baml_class__ => _class_name} = result, prefix, tb)
       when not is_nil(tb) do
    Map.new(result, fn {key, value} -> {key, parse_result(value, prefix, tb)} end)
  end

  defp parse_result(%{:__baml_class__ => class_name} = result, prefix, tb) do
    module = Module.concat(prefix, class_name)
    values = Enum.map(result, fn {key, value} -> {key, parse_result(value, prefix, tb)} end)
    struct(module, values)
  end

  defp parse_result(%{:__baml_enum__ => _, :value => value}, _prefix, _tb) do
    String.to_atom(value)
  end

  defp parse_result(list, prefix, tb) when is_list(list) do
    Enum.map(list, fn item -> parse_result(item, prefix, tb) end)
  end

  defp parse_result(result, _prefix, _tb) do
    result
  end

  defp to_map(args) when is_struct(args) do
    args
    |> Map.from_struct()
    |> to_map()
  end

  defp to_map(args) when is_map(args) do
    Map.new(args, fn {key, value} -> {key, to_map(value)} end)
  end

  defp to_map(args) when is_list(args) do
    Enum.map(args, &to_map/1)
  end

  defp to_map(args) do
    args
  end
end
