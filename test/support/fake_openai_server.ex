defmodule BamlElixirTest.FakeOpenAIServer do
  @moduledoc """
  A fake OpenAI-compatible HTTP server for tests, built on Bypass.
  """

  import Plug.Conn

  @type expected_header_value :: String.t() | :present | {:contains, String.t()}
  @type expected_headers :: %{optional(String.t()) => expected_header_value}

  @doc """
  Starts a Bypass server and sets up expectation for a chat completion request.
  Returns a base_url suitable for `openai-generic` provider.

  ## Options
  - `response_content` - The content string to return in the response
  - `expected_headers` - Map of headers to validate (optional)

  ## Example
      base_url = FakeOpenAIServer.expect_chat_completion("Hello!")
      # Use base_url in your client_registry
  """
  @spec expect_chat_completion(String.t(), expected_headers()) :: String.t()
  def expect_chat_completion(response_content, expected_headers \\ %{}, opts \\ %{})
      when is_binary(response_content) and is_map(expected_headers) and is_map(opts) do
    cached_tokens = Map.get(opts, :cached_tokens, 0)

    bypass = Bypass.open()

    Bypass.expect(bypass, "POST", "/v1/chat/completions", fn conn ->
      validate_headers!(conn, expected_headers)

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
          "usage" => %{"prompt_tokens" => 1, "completion_tokens" => 1, "total_tokens" => 2, "prompt_tokens_details" => %{"cached_tokens" => cached_tokens, "audio_tokens" => 0}}
        })

      conn
      |> put_resp_content_type("application/json")
      |> send_resp(200, body)
    end)

    "http://localhost:#{bypass.port}/v1"
  end

  @doc """
  Starts a Bypass server and sets up expectation for a streaming chat completion request.
  Returns `{base_url, bypass}` where bypass can be used to manage the server lifecycle.

  ## Options (map form)
  - `chunks` - List of content strings to stream
  - `delay_ms` - Delay between chunks in milliseconds (default: 0)
  - `notify_pid` - PID to send `{:chunk_sent, index}` messages to (optional)
  - `expected_headers` - Map of headers to validate (optional)

  ## Examples
      # Simple form - just a list of chunks
      {base_url, bypass} = FakeOpenAIServer.expect_chat_completion_stream(["Hello", " world", "!"])

      # Full config
      {base_url, bypass} = FakeOpenAIServer.expect_chat_completion_stream(%{
        chunks: ["Hello", " world"],
        delay_ms: 100,
        notify_pid: self()
      })

      # When client may disconnect early (e.g., cancellation tests):
      Bypass.down(bypass)  # Call before test ends to avoid shutdown errors
  """
  @spec expect_chat_completion_stream(list(String.t()) | map(), expected_headers()) ::
          {String.t(), Bypass.t()}
  def expect_chat_completion_stream(chunks_or_config, expected_headers \\ %{})

  def expect_chat_completion_stream(chunks, expected_headers) when is_list(chunks) do
    expect_chat_completion_stream(%{chunks: chunks}, expected_headers)
  end

  def expect_chat_completion_stream(config, expected_headers) when is_map(config) do
    chunks = Map.fetch!(config, :chunks)
    delay_ms = Map.get(config, :delay_ms, 0)
    notify_pid = Map.get(config, :notify_pid)

    bypass = Bypass.open()

    Bypass.stub(bypass, "POST", "/v1/chat/completions", fn conn ->
      try do
        validate_headers!(conn, expected_headers)

        conn =
          conn
          |> put_resp_content_type("text/event-stream")
          |> send_chunked(200)

        total_chunks = length(chunks)

        {conn, closed?} =
          chunks
          |> Enum.with_index()
          |> Enum.reduce_while({conn, false}, fn {content, index}, {conn, _closed?} ->
            sse_data = build_sse_chunk(content, index, total_chunks)

            case chunk(conn, sse_data) do
              {:ok, conn} ->
                if notify_pid, do: send(notify_pid, {:chunk_sent, index})
                if delay_ms > 0, do: Process.sleep(delay_ms)
                {:cont, {conn, false}}

              {:error, _reason} ->
                {:halt, {conn, true}}
            end
          end)

        if closed? do
          conn
        else
          case chunk(conn, "data: [DONE]\n\n") do
            {:ok, conn} -> conn
            {:error, _} -> conn
          end
        end
      rescue
        _ -> conn
      catch
        :exit, _ -> conn
      end
    end)

    {"http://localhost:#{bypass.port}/v1", bypass}
  end

  defp validate_headers!(conn, expected_headers) when expected_headers == %{}, do: conn

  defp validate_headers!(conn, expected_headers) do
    headers =
      conn.req_headers
      |> Enum.into(%{}, fn {k, v} -> {String.downcase(k), v} end)

    Enum.each(expected_headers, fn {key, expectation} ->
      key = String.downcase(to_string(key))

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

    conn
  end

  defp build_sse_chunk(content, index, total_chunks) do
    is_last = index == total_chunks - 1

    delta =
      if index == 0 do
        %{"role" => "assistant", "content" => content}
      else
        %{"content" => content}
      end

    chunk_json =
      Jason.encode!(%{
        "id" => "chatcmpl-test",
        "object" => "chat.completion.chunk",
        "created" => 1_700_000_000,
        "model" => "gpt-4o-mini",
        "system_fingerprint" => "fp_test",
        "choices" => [
          %{
            "index" => 0,
            "delta" => delta,
            "logprobs" => nil,
            "finish_reason" => if(is_last, do: "stop", else: nil)
          }
        ]
      })

    "data: #{chunk_json}\n\n"
  end
end
