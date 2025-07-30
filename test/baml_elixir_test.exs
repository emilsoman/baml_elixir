defmodule BamlElixirTest do
  use ExUnit.Case
  use BamlElixir.Client, path: "test/baml_src"

  alias BamlElixir.TypeBuilder

  doctest BamlElixir

  test "parses into a struct" do
    assert {:ok, %BamlElixirTest.Person{name: "John Doe", age: 28}} =
             BamlElixirTest.ExtractPerson.call(%{info: "John Doe, 28, Engineer"})
  end

  test "parsing into a struct with streaming" do
    pid = self()

    BamlElixirTest.ExtractPerson.stream(%{info: "John Doe, 28, Engineer"}, fn result ->
      send(pid, result)
    end)

    messages = wait_for_all_messages()

    # assert more than 1 partial message
    assert Enum.filter(messages, fn {type, _} -> type == :partial end) |> length() > 1

    assert Enum.filter(messages, fn {type, _} -> type == :done end) == [
             {:done, %BamlElixirTest.Person{name: "John Doe", age: 28}}
           ]
  end

  test "parses into a struct with a type builder" do
    assert {:ok,
            %{
              __baml_class__: "NewEmployeeFullyDynamic",
              employee_id: _,
              person: %{
                name: _,
                age: _,
                owned_houses_count: _,
                type: _,
                __baml_class__: "TestPerson"
              }
            }} =
             BamlElixirTest.CreateEmployee.call(%{}, %{
               tb: [
                 %TypeBuilder.Class{
                   name: "TestPerson",
                   fields: [
                     %TypeBuilder.Field{name: "name", type: :string},
                     %TypeBuilder.Field{name: "age", type: :int},
                     %TypeBuilder.Field{name: "owned_houses_count", type: 1},
                     %TypeBuilder.Field{name: "type", type: {:union, ["alive", "dead"]}},
                     %TypeBuilder.Field{name: "favorite_color", type: {:enum, "FavoriteColor"}}
                   ]
                 },
                 %TypeBuilder.Class{
                   name: "NewEmployeeFullyDynamic",
                   fields: [
                     %TypeBuilder.Field{name: "person", type: {:class, "TestPerson"}}
                   ]
                 },
                 %TypeBuilder.Enum{name: "FavoriteColor", values: ["RED", "GREEN", "BLUE"]}
               ]
             })
  end

  test "parses type builder with nested types" do
    assert {:ok,
            %{
              __baml_class__: "NewEmployeeFullyDynamic",
              employee_id: _,
              person: %{
                __baml_class__: "ThisClassIsNotDefinedInTheBAMLFile",
                name: _,
                age: _,
                departments: list_of_deps,
                managers: list_of_managers,
                work_experience: work_exp_map
              }
            } = employee} =
             BamlElixirTest.CreateEmployee.call(%{}, %{
               tb: [
                 %TypeBuilder.Class{
                   name: "NewEmployeeFullyDynamic",
                   fields: [
                     %TypeBuilder.Field{
                       name: "person",
                       type: %TypeBuilder.Class{
                         name: "ThisClassIsNotDefinedInTheBAMLFile",
                         fields: [
                           %TypeBuilder.Field{name: "name", type: :string},
                           %TypeBuilder.Field{name: "age", type: :int},
                           %TypeBuilder.Field{
                             name: "departments",
                             type: %TypeBuilder.List{
                               type: %TypeBuilder.Class{
                                 name: "Department",
                                 fields: [
                                   %TypeBuilder.Field{name: "name", type: :string},
                                   %TypeBuilder.Field{name: "location", type: :string}
                                 ]
                               }
                             }
                           },
                           %TypeBuilder.Field{
                             name: "managers",
                             type: %TypeBuilder.List{type: :string}
                           },
                           %TypeBuilder.Field{
                             name: "work_experience",
                             type: %TypeBuilder.Map{
                               key_type: :string,
                               value_type: :string
                             }
                           }
                         ]
                       }
                     }
                   ]
                 }
               ]
             })

    assert Enum.sort(Map.keys(employee)) ==
             Enum.sort([:__baml_class__, :employee_id, :person])

    assert Enum.sort(Map.keys(employee.person)) ==
             Enum.sort([:__baml_class__, :name, :age, :departments, :managers, :work_experience])

    assert is_list(list_of_deps)
    assert is_list(list_of_managers)
    assert is_map(work_exp_map)
    assert Enum.all?(work_exp_map, fn {key, value} -> is_binary(key) and is_binary(value) end)
  end

  test "change default model" do
    assert BamlElixirTest.WhichModel.call(%{}, %{llm_client: "GPT4"}) == {:ok, :GPT4oMini}
    assert BamlElixirTest.WhichModel.call(%{}, %{llm_client: "DeepSeekR1"}) == {:ok, :DeepSeekR1}
  end

  test "get union type" do
    assert BamlElixirTest.WhichModelUnion.call(%{}, %{llm_client: "GPT4"}) == {:ok, "GPT"}

    assert BamlElixirTest.WhichModelUnion.call(%{}, %{llm_client: "DeepSeekR1"}) ==
             {:ok, "DeepSeek"}
  end

  test "Error when parsing the output of a function" do
    assert {:error, "Failed to coerce value" <> _} = BamlElixirTest.DummyOutputFunction.call(%{})
  end

  test "get usage from collector" do
    collector = BamlElixir.Collector.new("test-collector")

    assert BamlElixirTest.WhichModel.call(%{}, %{llm_client: "GPT4", collectors: [collector]}) ==
             {:ok, :GPT4oMini}

    usage = BamlElixir.Collector.usage(collector)
    assert usage["input_tokens"] == 33
    assert usage["output_tokens"] > 0
  end

  test "get usage from collector with streaming using GPT4" do
    collector = BamlElixir.Collector.new("test-collector")
    pid = self()

    BamlElixirTest.CreateEmployee.stream(
      %{},
      fn result -> send(pid, result) end,
      %{llm_client: "GPT4", collectors: [collector]}
    )

    _messages = wait_for_all_messages()

    usage = BamlElixir.Collector.usage(collector)
    assert usage["input_tokens"] == 32
  end

  test "get last function log from collector" do
    collector = BamlElixir.Collector.new("test-collector")

    assert BamlElixirTest.WhichModel.call(%{}, %{llm_client: "GPT4", collectors: [collector]}) ==
             {:ok, :GPT4oMini}

    last_function_log = BamlElixir.Collector.last_function_log(collector)
    assert last_function_log["function_name"] == "WhichModel"

    response_body =
      last_function_log["calls"]
      |> Enum.at(0)
      |> Map.get("response")
      |> Map.get("body")
      |> Jason.decode!()

    assert response_body["usage"]["prompt_tokens_details"] == %{
             "audio_tokens" => 0,
             "cached_tokens" => 0
           }

    assert Map.keys(last_function_log) == [
             "calls",
             "function_name",
             "id",
             "log_type",
             "raw_llm_response",
             "timing",
             "usage"
           ]
  end

  test "parsing of nested structs" do
    attendees = %BamlElixirTest.Attendees{
      hosts: [
        %BamlElixirTest.Person{name: "John Doe", age: 28},
        %BamlElixirTest.Person{name: "Bob Johnson", age: 35}
      ],
      guests: [
        %BamlElixirTest.Person{name: "Alice Smith", age: 25},
        %BamlElixirTest.Person{name: "Carol Brown", age: 30},
        %BamlElixirTest.Person{name: "Jane Doe", age: 28}
      ]
    }

    assert {:ok, attendees} ==
             BamlElixirTest.ParseAttendees.call(%{
               attendees: """
               John Doe 28 - Host
               Alice Smith 25 - Guest
               Bob Johnson 35 - Host
               Carol Brown 30 - Guest
               Jane Doe 28 - Guest
               """
             })
  end

  defp wait_for_all_messages(messages \\ []) do
    receive do
      {:partial, _} = message ->
        wait_for_all_messages([message | messages])

      {:done, _} = message ->
        [message | messages] |> Enum.reverse()

      {:error, message} ->
        raise "Error: #{inspect(message)}"
    end
  end
end
