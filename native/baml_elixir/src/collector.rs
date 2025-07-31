use baml_runtime::tracingv2::storage::storage::Collector as BamlCollector;
use rustler::{Encoder, Env, Resource, ResourceArc, Term};
use std::sync::{Arc, Mutex};

#[rustler::resource_impl()]
impl Resource for CollectorResource {}

pub struct CollectorResource {
    pub inner: Arc<BamlCollector>,
}

impl CollectorResource {
    pub fn new(name: Option<String>) -> ResourceArc<CollectorResource> {
        let collector = BamlCollector::new(name);
        ResourceArc::new(CollectorResource {
            inner: Arc::new(collector),
        })
    }

    pub fn usage(&self) -> Usage {
        Usage {
            inner: self.inner.clone().usage(),
        }
    }

    pub fn last_function_log(&self) -> Option<FunctionLog> {
        self.inner.last_function_log().map(|log| FunctionLog {
            inner: Arc::new(Mutex::new(log)),
        })
    }
}

pub struct FunctionLog {
    pub inner: Arc<Mutex<baml_runtime::tracingv2::storage::storage::FunctionLog>>,
}

pub struct Usage {
    pub inner: baml_runtime::tracingv2::storage::storage::Usage,
}

pub struct Timing {
    pub inner: baml_runtime::tracingv2::storage::storage::Timing,
}

pub struct StreamTiming {
    pub inner: baml_runtime::tracingv2::storage::storage::StreamTiming,
}

pub struct LLMCallKind {
    pub inner: baml_runtime::tracingv2::storage::storage::LLMCallKind,
}

pub struct LLMCall {
    pub inner: baml_runtime::tracingv2::storage::storage::LLMCall,
}

pub struct LLMStreamCall {
    pub inner: baml_runtime::tracingv2::storage::storage::LLMStreamCall,
}

impl Encoder for Usage {
    fn encode<'a>(&self, env: Env<'a>) -> Term<'a> {
        let map = Term::map_new(env);
        map.map_put("input_tokens", self.inner.input_tokens)
            .unwrap()
            .map_put("output_tokens", self.inner.output_tokens)
            .unwrap()
    }
}

impl Encoder for Timing {
    fn encode<'a>(&self, env: Env<'a>) -> Term<'a> {
        let map = Term::map_new(env);
        map.map_put("start_time_utc_ms", self.inner.start_time_utc_ms)
            .unwrap()
            .map_put("duration_ms", self.inner.duration_ms)
            .unwrap()
        // TODO: BAML doesn't track this yet
        // .map_put(
        //     "time_to_first_parsed_ms",
        //     self.inner.time_to_first_parsed_ms,
        // )
        // .unwrap()
    }
}

impl Encoder for StreamTiming {
    fn encode<'a>(&self, env: Env<'a>) -> Term<'a> {
        let map = Term::map_new(env);
        map.map_put("start_time_utc_ms", self.inner.start_time_utc_ms)
            .unwrap()
            .map_put("duration_ms", self.inner.duration_ms)
            .unwrap()
        // TODO: BAML doesn't track this yet
        // .map_put(
        //     "time_to_first_parsed_ms",
        //     self.inner.time_to_first_parsed_ms,
        // )
        // .unwrap()
        // TODO: BAML doesn't track this yet
        // .map_put("time_to_first_token_ms", self.inner.time_to_first_token_ms)
        // .unwrap()
    }
}

impl Encoder for LLMCall {
    fn encode<'a>(&self, env: Env<'a>) -> Term<'a> {
        let map = Term::map_new(env);
        map.map_put("client_name", self.inner.client_name.clone())
            .unwrap()
            .map_put("provider", self.inner.provider.clone())
            .unwrap()
            .map_put(
                "timing",
                Timing {
                    inner: self.inner.timing.clone(),
                },
            )
            .unwrap()
            .map_put(
                "request",
                self.inner.request.as_deref().map(|r| {
                    let map = Term::map_new(env);
                    map.map_put("method", r.method.clone())
                        .unwrap()
                        .map_put("url", r.url.clone())
                        .unwrap()
                        .map_put("headers", r.headers().clone())
                        .unwrap()
                        .map_put("body", r.body.text().unwrap_or_default().encode(env))
                        .unwrap()
                }),
            )
            .unwrap()
            .map_put(
                "response",
                self.inner.response.as_deref().map(|r| {
                    let map = Term::map_new(env);
                    map.map_put("status", r.status.clone())
                        .unwrap()
                        .map_put("headers", r.headers())
                        .unwrap()
                        .map_put("body", r.body.text().unwrap_or_default().encode(env))
                        .unwrap()
                }),
            )
            .unwrap()
            .map_put(
                "usage",
                Usage {
                    inner: self.inner.usage.clone().unwrap_or_default(),
                },
            )
            .unwrap()
    }
}

impl Encoder for LLMStreamCall {
    fn encode<'a>(&self, env: Env<'a>) -> Term<'a> {
        let map = Term::map_new(env);
        map.map_put("client_name", self.inner.llm_call.client_name.clone())
            .unwrap()
            .map_put("provider", self.inner.llm_call.provider.clone())
            .unwrap()
            .map_put(
                "timing",
                StreamTiming {
                    inner: self.inner.timing.clone(),
                },
            )
            .unwrap()
            .map_put(
                "request",
                self.inner.llm_call.request.as_deref().map(|r| {
                    let map = Term::map_new(env);
                    map.map_put("method", r.method.clone())
                        .unwrap()
                        .map_put("url", r.url.clone())
                        .unwrap()
                        .map_put("headers", r.headers())
                        .unwrap()
                        .map_put("body", r.body.text().unwrap_or_default().encode(env))
                        .unwrap()
                }),
            )
            .unwrap()
            .map_put(
                "response",
                self.inner.llm_call.response.as_deref().map(|r| {
                    let map = Term::map_new(env);
                    map.map_put("status", r.status.clone())
                        .unwrap()
                        .map_put("headers", r.headers())
                        .unwrap()
                        .map_put("body", r.body.text().unwrap_or_default().encode(env))
                        .unwrap()
                }),
            )
            .unwrap()
            .map_put(
                "usage",
                Usage {
                    inner: self.inner.llm_call.usage.clone().unwrap_or_default(),
                },
            )
            .unwrap()
    }
}

impl Encoder for LLMCallKind {
    fn encode<'a>(&self, env: Env<'a>) -> Term<'a> {
        match &self.inner {
            baml_runtime::tracingv2::storage::storage::LLMCallKind::Basic(call) => LLMCall {
                inner: call.clone(),
            }
            .encode(env),
            baml_runtime::tracingv2::storage::storage::LLMCallKind::Stream(stream) => {
                LLMStreamCall {
                    inner: stream.clone(),
                }
                .encode(env)
            }
        }
    }
}

impl Encoder for FunctionLog {
    fn encode<'a>(&self, env: Env<'a>) -> Term<'a> {
        let map = Term::map_new(env);
        let mut inner = self.inner.lock().unwrap();
        map.map_put("id", inner.id().to_string())
            .unwrap()
            .map_put("function_name", inner.function_name())
            .unwrap()
            .map_put("log_type", inner.log_type().clone())
            .unwrap()
            .map_put(
                "timing",
                Timing {
                    inner: inner.timing().clone(),
                },
            )
            .unwrap()
            .map_put(
                "usage",
                Usage {
                    inner: inner.usage().clone(),
                },
            )
            .unwrap()
            .map_put(
                "calls",
                inner
                    .calls()
                    .iter()
                    .map(|c| LLMCallKind { inner: c.clone() }.encode(env))
                    .collect::<Vec<_>>(),
            )
            .unwrap()
            .map_put(
                "raw_llm_response",
                inner.raw_llm_response().unwrap_or_default().encode(env),
            )
            .unwrap()
    }
}
