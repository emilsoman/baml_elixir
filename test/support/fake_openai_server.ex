defmodule BamlElixirTest.FakeOpenAIServer do
  @moduledoc false

  # A tiny OpenAI-compatible HTTP server for tests.
  # Request handling is delegated to a handler module (usually a Mox mock).

  @mock BamlElixirTest.OpenAIHandlerMock

  @doc """
  Convenience helper for tests: expect a single chat completion request and respond
  with an OpenAI-compatible JSON body whose assistant message content is `response_content`.

  The expectation is set on #{@mock} so tests don't need to reference the mock directly.
  """
  @type expected_header_value :: String.t() | :present | {:contains, String.t()}
  @type expected_headers :: %{optional(String.t()) => expected_header_value}

  @spec expect_chat_completion(String.t(), expected_headers()) :: :ok
  def expect_chat_completion(response_content, expected_headers \\ %{})
      when is_binary(response_content) and is_map(expected_headers) do
    expect_chat_completion(response_content, expected_headers, %{})
  end

  @spec expect_chat_completion(String.t(), expected_headers(), map()) :: :ok
  def expect_chat_completion(response_content, expected_headers, opts)
      when is_binary(response_content) and is_map(expected_headers) and is_map(opts) do
    cached_tokens = Map.get(opts, :cached_tokens, 0)

    body =
      Jason.encode!(%{
        "id" => "chatcmpl-test",
        "object" => "chat.completion",
        "created" => 1_700_000_000,
        "model" => "gpt-4o-mini",
        "choices" => [
          %{
            "index" => 0,
            "message" => %{"role" => "assistant", "content" => response_content},
            "finish_reason" => "stop"
          }
        ],
        "usage" => %{
          "prompt_tokens" => 1,
          "completion_tokens" => 1,
          "total_tokens" => 2,
          "prompt_tokens_details" => %{"cached_tokens" => cached_tokens, "audio_tokens" => 0}
        }
      })

    normalized_expected_headers =
      Map.new(expected_headers, fn {k, v} -> {String.downcase(to_string(k)), v} end)

    Mox.expect(@mock, :handle_request, fn path, headers, _body ->
      # Basic sanity check that the runtime hit the expected endpoint
      if !String.contains?(path, "chat/completions") do
        raise "Unexpected path: #{inspect(path)}"
      end

      assert_expected_headers!(headers, normalized_expected_headers)

      %{status: 200, headers: [{"content-type", "application/json"}], body: body}
    end)

    :ok
  end

  defp assert_expected_headers!(_headers, expected) when expected == %{}, do: :ok

  defp assert_expected_headers!(headers, expected) when is_map(headers) and is_map(expected) do
    Enum.each(expected, fn {key, expectation} ->
      case expectation do
        :present ->
          if !Map.has_key?(headers, key) do
            raise "Expected header #{inspect(key)} to be present, got: #{inspect(Map.keys(headers))}"
          end

        {:contains, substring} when is_binary(substring) ->
          actual = Map.get(headers, key)

          if is_nil(actual) or !String.contains?(actual, substring) do
            raise "Expected header #{inspect(key)} to contain #{inspect(substring)}, got: #{inspect(actual)}"
          end

        value when is_binary(value) ->
          actual = Map.get(headers, key)

          if actual != value do
            raise "Expected header #{inspect(key)} to equal #{inspect(value)}, got: #{inspect(actual)}"
          end

        other ->
          raise "Unsupported header expectation for #{inspect(key)}: #{inspect(other)}"
      end
    end)
  end

  @doc """
  Convenience helper for tests: starts the server wired to the Mox mock and returns
  a base_url suitable for `openai-generic` (`.../v1`).
  """
  @spec start_base_url() :: String.t()
  def start_base_url() do
    {:ok, server_pid, port} = start_link(@mock)
    Mox.allow(@mock, self(), server_pid)
    "http://127.0.0.1:#{port}/v1"
  end

  @spec start_link(module()) :: {:ok, pid(), non_neg_integer()}
  def start_link(handler_module) when is_atom(handler_module) do
    {:ok, listen_socket} =
      :gen_tcp.listen(0, [:binary, packet: :raw, active: false, reuseaddr: true])

    {:ok, port} = :inet.port(listen_socket)

    pid =
      spawn_link(fn ->
        {:ok, socket} = :gen_tcp.accept(listen_socket)

        {:ok, header_blob} = recv_until_headers(socket, <<>>)
        {path, headers, content_length} = parse_request(header_blob)

        body =
          if content_length > 0 do
            case :gen_tcp.recv(socket, content_length, 5_000) do
              {:ok, b} -> b
              _ -> <<>>
            end
          else
            <<>>
          end

        response = handler_module.handle_request(path, headers, body)
        %{status: status, body: resp_body} = response
        resp_headers = Map.get(response, :headers, [])

        # Ensure minimal headers exist
        resp_headers =
          resp_headers
          |> ensure_header("content-length", Integer.to_string(byte_size(resp_body)))
          |> ensure_header("connection", "close")

        resp =
          "HTTP/1.1 #{status} OK\r\n" <>
            Enum.map_join(resp_headers, "", fn {k, v} -> "#{k}: #{v}\r\n" end) <>
            "\r\n" <>
            resp_body

        :gen_tcp.send(socket, resp)
        :gen_tcp.close(socket)
        :gen_tcp.close(listen_socket)
      end)

    {:ok, pid, port}
  end

  defp ensure_header(headers, key, value) do
    key_down = String.downcase(key)

    if Enum.any?(headers, fn {k, _} -> String.downcase(k) == key_down end) do
      headers
    else
      [{key, value} | headers]
    end
  end

  defp recv_until_headers(socket, acc) do
    case :binary.match(acc, "\r\n\r\n") do
      {_, _} ->
        {:ok, acc}

      :nomatch ->
        case :gen_tcp.recv(socket, 0, 5_000) do
          {:ok, chunk} -> recv_until_headers(socket, acc <> chunk)
          other -> other
        end
    end
  end

  defp parse_request(header_blob) do
    [request_line | header_lines] =
      header_blob
      |> String.split("\r\n", trim: true)

    path =
      case String.split(request_line, " ", parts: 3) do
        [_method, p, _http] -> p
        _ -> ""
      end

    headers =
      header_lines
      |> Enum.reduce(%{}, fn line, acc ->
        case String.split(line, ":", parts: 2) do
          [k, v] -> Map.put(acc, String.downcase(String.trim(k)), String.trim(v))
          _ -> acc
        end
      end)

    content_length =
      headers
      |> Map.get("content-length", "0")
      |> String.to_integer()

    {path, headers, content_length}
  end
end
