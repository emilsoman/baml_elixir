defmodule BamlElixirTest do
  use ExUnit.Case
  use BamlElixir.Client, path: "test/baml_src"

  alias BamlElixir.TypeBuilder

  doctest BamlElixir

  @tag :client_registry
  test "client_registry supports clients key (list form)" do
    client_registry = %{
      primary: "InjectedClient",
      clients: [
        %{
          name: "InjectedClient",
          provider: "definitely-not-a-provider",
          retry_policy: nil,
          options: %{model: "gpt-4o-mini"}
        }
      ]
    }

    # parse: false to avoid any parsing work; we want to exercise registry decoding/validation
    assert {:error, msg} =
             BamlElixirTest.WhichModel.call(%{}, %{client_registry: client_registry, parse: false})

    assert msg =~ "Invalid client provider"
  end

  @tag :client_registry
  test "client_registry supports clients key (map form)" do
    client_registry = %{
      primary: "InjectedClient",
      clients: %{
        "InjectedClient" => %{
          provider: "definitely-not-a-provider",
          retry_policy: nil,
          options: %{model: "gpt-4o-mini"}
        }
      }
    }

    assert {:error, msg} =
             BamlElixirTest.WhichModel.call(%{}, %{client_registry: client_registry, parse: false})

    assert msg =~ "Invalid client provider"
  end

  @tag :client_registry
  test "client_registry can inject and select a client not present in the BAML files (success path)" do
    base_url = BamlElixirTest.FakeOpenAIServer.expect_chat_completion("GPT4")

    client_registry = %{
      primary: "InjectedClient",
      clients: [
        %{
          name: "InjectedClient",
          provider: "openai-generic",
          retry_policy: nil,
          options: %{
            base_url: base_url,
            api_key: "test-key",
            model: "gpt-4o-mini"
          }
        }
      ]
    }

    # This function declares `client GPT4` in the .baml file, so success here proves
    # `client_registry.primary` overrides the static client selection.
    assert {:ok, "GPT4"} =
             BamlElixirTest.WhichModelUnion.call(%{}, %{client_registry: client_registry})
  end

  @tag :client_registry
  test "client_registry passes clients[].options.headers into the HTTP request" do
    base_url =
      BamlElixirTest.FakeOpenAIServer.expect_chat_completion("GPT4", %{
        "x-test-header" => "hello-from-elixir"
      })

    client_registry = %{
      primary: "InjectedClient",
      clients: [
        %{
          name: "InjectedClient",
          provider: "openai-generic",
          retry_policy: nil,
          options: %{
            base_url: base_url,
            api_key: "test-key",
            model: "gpt-4o-mini",
            headers: %{
              "x-test-header" => "hello-from-elixir"
            }
          }
        }
      ]
    }

    assert {:ok, "GPT4"} =
             BamlElixirTest.WhichModelUnion.call(%{}, %{client_registry: client_registry})
  end

  @tag :collector
  test "collector usage includes cached_input_tokens from fake server" do
    base_url = BamlElixirTest.FakeOpenAIServer.expect_chat_completion("GPT4", %{}, %{cached_tokens: 42})

    client_registry = %{
      primary: "InjectedClient",
      clients: [
        %{
          name: "InjectedClient",
          provider: "openai-generic",
          retry_policy: nil,
          options: %{
            base_url: base_url,
            api_key: "test-key",
            model: "gpt-4o-mini"
          }
        }
      ]
    }

    collector = BamlElixir.Collector.new("test-collector")

    assert {:ok, "GPT4"} =
             BamlElixirTest.WhichModelUnion.call(%{}, %{
               client_registry: client_registry,
               collectors: [collector]
             })

    usage = BamlElixir.Collector.usage(collector)
    assert usage["input_tokens"] == 1
    assert usage["output_tokens"] == 1
    assert usage["cached_input_tokens"] == 42
  end

  @tag :collector
  test "collector usage returns zero cached_input_tokens when none cached" do
    base_url = BamlElixirTest.FakeOpenAIServer.expect_chat_completion("GPT4")

    client_registry = %{
      primary: "InjectedClient",
      clients: [
        %{
          name: "InjectedClient",
          provider: "openai-generic",
          retry_policy: nil,
          options: %{
            base_url: base_url,
            api_key: "test-key",
            model: "gpt-4o-mini"
          }
        }
      ]
    }

    collector = BamlElixir.Collector.new("test-collector")

    assert {:ok, "GPT4"} =
             BamlElixirTest.WhichModelUnion.call(%{}, %{
               client_registry: client_registry,
               collectors: [collector]
             })

    usage = BamlElixir.Collector.usage(collector)
    assert usage["cached_input_tokens"] == 0
  end

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

  test "parsing into a struct with sync_stream" do
    {:ok, agent_pid} = Agent.start_link(fn -> 0 end, name: :counter)

    assert {:ok, %BamlElixirTest.Person{name: "John Doe", age: 28}} =
             BamlElixirTest.ExtractPerson.sync_stream(
               %{info: "John Doe, 28, Engineer"},
               fn _result ->
                 Agent.update(agent_pid, fn count -> count + 1 end)
               end
             )

    assert Agent.get(agent_pid, fn count -> count end) > 1
  end

  test "bool input and output" do
    assert {:ok, true} = BamlElixirTest.FlipSwitch.call(%{switch: false})
  end

  test "parses into a struct with a type builder" do
    assert {:ok,
            %{
              __baml_class__: "NewEmployeeFullyDynamic",
              employee_id: _,
              person: %{
                name: "Foobar123",
                age: _,
                owned_houses_count: _,
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
                     %TypeBuilder.Field{name: "owned_houses_count", type: 1},
                     %TypeBuilder.Field{
                       name: "favorite_day",
                       type: %TypeBuilder.Union{types: ["sunday", "monday"]}
                     },
                     %TypeBuilder.Field{
                       name: "favorite_color",
                       type: %TypeBuilder.Enum{name: "FavoriteColor"}
                     }
                   ]
                 },
                 %TypeBuilder.Enum{
                   name: "FavoriteColor",
                   values: [
                     %TypeBuilder.EnumValue{value: "RED", description: "Pick this always"},
                     %TypeBuilder.EnumValue{value: "GREEN"},
                     %TypeBuilder.EnumValue{value: "BLUE"}
                   ]
                 },
                 %TypeBuilder.Class{
                   name: "NewEmployeeFullyDynamic",
                   fields: [
                     %TypeBuilder.Field{
                       name: "person",
                       type: %TypeBuilder.Class{name: "TestPerson"}
                     }
                   ]
                 }
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
    assert BamlElixirTest.WhichModel.call(%{}, %{llm_client: "Claude"}) == {:ok, :Claude}
  end

  test "get union type" do
    assert BamlElixirTest.WhichModelUnion.call(%{}, %{llm_client: "GPT4"}) == {:ok, "GPT4"}

    assert BamlElixirTest.WhichModelUnion.call(%{}, %{llm_client: "Claude"}) ==
             {:ok, "Claude"}
  end

  test "Error when parsing the output of a function" do
    assert {:error, "Failed to coerce value" <> _} = BamlElixirTest.DummyOutputFunction.call(%{})
  end

  test "get usage from collector" do
    collector = BamlElixir.Collector.new("test-collector")

    assert BamlElixirTest.WhichModel.call(%{}, %{llm_client: "GPT4", collectors: [collector]}) ==
             {:ok, :GPT4oMini}

    usage = BamlElixir.Collector.usage(collector)
    assert usage["input_tokens"] == 30
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

  test "get last function log from collector with streaming" do
    collector = BamlElixir.Collector.new("test-collector")
    pid = self()

    BamlElixirTest.CreateEmployee.stream(
      %{},
      fn result -> send(pid, result) end,
      %{llm_client: "GPT4", collectors: [collector]}
    )

    _messages = wait_for_all_messages()

    last_function_log = BamlElixir.Collector.last_function_log(collector)

    %{"messages" => messages} =
      last_function_log["calls"]
      |> Enum.at(0)
      |> Map.get("request")
      |> Map.get("body")
      |> Jason.decode!()

    assert messages == [
             %{
               "content" => [
                 %{
                   "text" =>
                     "Create a fake employee data with the following information:\nAnswer in JSON using this schema:\n{\n  employee_id: string,\n}",
                   "type" => "text"
                 }
               ],
               "role" => "system"
             }
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

  test "Agent with type builder returns Tool with reasoning and tool (union of BAML tool classes)" do
    assert {:ok, result} =
             BamlElixirTest.Agent.call(
               %{message: "What's the weather in Paris?"},
               %{tb: build_tool_type(["WeatherTool", "ToNumberTool"])}
             )

    assert %{
             tool: %{__baml_class__: "WeatherTool", city: "Paris"},
             __baml_class__: "Tool",
             reasoning: _reasoning
           } = result

    assert {:ok, "error"} =
             BamlElixirTest.Agent.call(
               %{message: "What's the weather in Paris?"},
               %{tb: build_tool_type(["ToNumberTool"])}
             )

    assert {:ok, result} =
             BamlElixirTest.Agent.call(
               %{message: "Convert hundred and one to a number"},
               %{tb: build_tool_type(["ToNumberTool"])}
             )

    assert %{
             tool: %{number: 101, __baml_class__: "ToNumberTool"},
             __baml_class__: "Tool",
             reasoning: _reasoning
           } = result
  end

  test "Agent with type builder with names returns Tool with reasoning and tool (union of BAML tool classes)" do
    assert {:ok, result} =
             BamlElixirTest.Agent.call(
               %{message: "Convert hundred and one to a number"},
               %{tb: build_tool_type_with_names(["ToNumberTool"])}
             )

    assert %{
             __baml_class__: "Tool",
             reasoning: _reasoning,
             tool: %{
               __baml_class__: "ToolChoice_ToNumberTool",
               args: %{number: 101, __baml_class__: "ToNumberTool"},
               name: "ToNumberTool"
             }
           } = result
  end

  defp build_tool_type(tool_names) when is_list(tool_names) do
    tool_union = %TypeBuilder.Union{
      types: Enum.map(tool_names, fn name -> %TypeBuilder.Class{name: name} end)
    }

    [
      %TypeBuilder.Class{
        name: "Tool",
        fields: [
          %TypeBuilder.Field{name: "tool", type: tool_union}
        ]
      }
    ]
  end

  defp build_tool_type_with_names(tool_names) when is_list(tool_names) do
    tool_union = %TypeBuilder.Union{
      types:
        Enum.map(tool_names, fn name ->
          %TypeBuilder.Class{
            name: "ToolChoice_#{name}",
            fields: [
              %TypeBuilder.Field{
                name: "name",
                type: %TypeBuilder.Literal{value: name}
              },
              %TypeBuilder.Field{name: "args", type: %TypeBuilder.Class{name: name}}
            ]
          }
        end)
    }

    [
      %TypeBuilder.Class{
        name: "Tool",
        fields: [
          %TypeBuilder.Field{name: "tool", type: tool_union}
        ]
      }
    ]
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

  describe "stream cancellation" do
    import Mox

    setup [:set_mox_global, :verify_on_exit!]

    setup do
      Application.put_env(:baml_elixir, :native_module, BamlElixir.NativeMock)
      on_exit(fn -> Application.delete_env(:baml_elixir, :native_module) end)
    end

    @tag :stream_cancellation
    test "killing caller process calls abort_tripwire" do
      test_pid = self()
      tripwire_ref = make_ref()

      stub(BamlElixir.NativeMock, :create_tripwire, fn -> tripwire_ref end)

      expect(BamlElixir.NativeMock, :abort_tripwire, fn ^tripwire_ref ->
        send(test_pid, :abort_called)
        :ok
      end)

      stub(BamlElixir.NativeMock, :stream, fn pid,
                                              ref,
                                              _tripwire,
                                              _fn,
                                              _args,
                                              _path,
                                              _collectors,
                                              _registry,
                                              _tb ->
        send(test_pid, :stream_started)

        spawn(fn ->
          receive do
            :continue_streaming ->
              send(pid, {ref, {:partial, "chunk"}})
              send(pid, {ref, {:done, "result"}})
          after
            5000 -> :timeout
          end
        end)

        :ok
      end)

      caller_pid =
        spawn(fn ->
          BamlElixir.Client.stream("TestFunction", %{}, fn _ -> :ok end, %{path: "test/baml_src"})

          receive do
            :stop -> :ok
          end
        end)

      assert_receive :stream_started, 1000
      Process.exit(caller_pid, :kill)
      assert_receive :abort_called, 1000
    end
  end
end
