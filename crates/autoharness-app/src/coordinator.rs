use std::sync::Arc;

use autoharness_domain::{
    AttemptFailure, AttemptId, ClassifiedError, CommandPayload, DeliveryMode, ErrorClass,
    ErrorCode, PromptText, PublicMessage, ResponseText, RetryAdvice, SessionId,
    UsageSnapshot as DomainUsage,
};
use autoharness_engine::{
    AttemptStatus as EngineAttemptStatus, DurableEngineError, SessionAggregate,
};
use autoharness_provider::{
    CancellationToken, CatalogRequest, ChatContent, ChatMessage, ChatRequest, ChatRole,
    ModelCatalog, ModelDescriptor, Provider, ProviderError, ProviderErrorKind, ProviderStreamEvent,
};
#[cfg(test)]
use autoharness_provider_gemini::{GeminiApiKey, GeminiProvider};
use autoharness_tui::{
    ApiCredential, AppPorts, AttemptKey, CatalogProjection, RequestId, RetryPolicy, UiFailure,
    UiIntent, UiNotice,
};
use futures_util::StreamExt as _;
use tokio::sync::mpsc;

use crate::engine_actor::EngineHandle;
use crate::error::AppError;
use crate::{ids, projection, telemetry};

const PROVIDER_MESSAGE_CAPACITY: usize = 128;

pub(crate) type ProviderFactory =
    Arc<dyn Fn(ApiCredential) -> Result<Arc<dyn Provider>, ProviderError> + Send + Sync + 'static>;

#[cfg(test)]
fn gemini_provider(credential: ApiCredential) -> Result<Arc<dyn Provider>, ProviderError> {
    let api_key = GeminiApiKey::new(credential.into_string())?;
    Ok(Arc::new(GeminiProvider::new(api_key)?))
}

enum AsyncMessage {
    Catalog {
        generation: u64,
        request_id: Option<RequestId>,
        result: Result<ModelCatalog, ProviderError>,
    },
    Stream {
        attempt_id: AttemptId,
        result: Result<ProviderStreamEvent, ProviderError>,
    },
}

struct ActiveAttempt {
    attempt_id: AttemptId,
    cancellation: CancellationToken,
}

enum StartAttemptError {
    Engine(DurableEngineError),
    Provider(ProviderError),
}

/// Owns application orchestration while the terminal runner owns UI state.
pub struct Coordinator {
    session_id: SessionId,
    session: SessionAggregate,
    engine: EngineHandle,
    provider: Option<Arc<dyn Provider>>,
    provider_factory: ProviderFactory,
    ports: AppPorts,
    messages: mpsc::Sender<AsyncMessage>,
    message_rx: mpsc::Receiver<AsyncMessage>,
    shutdown: CancellationToken,
    active: Option<ActiveAttempt>,
    catalog_models: Vec<ModelDescriptor>,
    catalog_generation: u64,
    catalog_cancellation: Option<CancellationToken>,
}

impl Coordinator {
    /// Creates application composition around replayed state and bounded ports.
    #[must_use]
    #[cfg(test)]
    pub fn new(
        session_id: SessionId,
        session: SessionAggregate,
        engine: EngineHandle,
        provider: Option<Arc<dyn Provider>>,
        ports: AppPorts,
        shutdown: CancellationToken,
    ) -> Self {
        Self::with_provider_factory(
            session_id,
            session,
            engine,
            provider,
            Arc::new(gemini_provider),
            ports,
            shutdown,
        )
    }

    pub(crate) fn with_provider_factory(
        session_id: SessionId,
        session: SessionAggregate,
        engine: EngineHandle,
        provider: Option<Arc<dyn Provider>>,
        provider_factory: ProviderFactory,
        ports: AppPorts,
        shutdown: CancellationToken,
    ) -> Self {
        let (messages, message_rx) = mpsc::channel(PROVIDER_MESSAGE_CAPACITY);
        Self {
            session_id,
            session,
            engine,
            provider,
            provider_factory,
            ports,
            messages,
            message_rx,
            shutdown,
            active: None,
            catalog_models: Vec::new(),
            catalog_generation: 0,
            catalog_cancellation: None,
        }
    }

    /// Runs until terminal shutdown or application-channel closure.
    pub async fn run(mut self) -> Result<(), AppError> {
        if self.provider.is_some() {
            self.refresh_catalog(None);
        }

        loop {
            tokio::select! {
                () = self.shutdown.cancelled() => {
                    if let Some(active) = &self.active {
                        active.cancellation.cancel();
                    }
                    if let Some(cancellation) = &self.catalog_cancellation {
                        cancellation.cancel();
                    }
                    return Ok(());
                }
                intent = self.ports.intents.recv() => {
                    let Some(intent) = intent else {
                        return Ok(());
                    };
                    self.handle_intent(intent).await?;
                }
                message = self.message_rx.recv() => {
                    let Some(message) = message else {
                        return Err(AppError::WorkerStopped);
                    };
                    self.handle_async(message).await?;
                }
            }
        }
    }

    async fn handle_intent(&mut self, intent: UiIntent) -> Result<(), AppError> {
        match intent {
            UiIntent::ConfigureCredential {
                request_id,
                credential,
            } => {
                self.configure_credential(request_id, credential).await?;
            }
            UiIntent::RefreshCatalog { request_id } => {
                if self.provider.is_some() {
                    self.ports
                        .catalogs
                        .send_replace(Arc::new(CatalogProjection::Loading));
                    self.refresh_catalog(Some(request_id));
                } else {
                    self.reject(
                        request_id,
                        UiFailure::new(
                            ErrorClass::Authentication,
                            "A provider API key is not configured",
                            RetryPolicy::Never,
                        ),
                    )
                    .await?;
                }
            }
            UiIntent::SelectModel { request_id, model } => {
                if !self.model_is_available(&model) {
                    self.reject(
                        request_id,
                        UiFailure::new(
                            ErrorClass::Validation,
                            "The selected model is not in the current compatible catalog",
                            RetryPolicy::Never,
                        ),
                    )
                    .await?;
                    return Ok(());
                }
                match self
                    .execute(CommandPayload::SelectModel {
                        session_id: self.session_id.clone(),
                        model,
                    })
                    .await
                {
                    Ok(()) => self.commit(request_id).await?,
                    Err(error) => self.reject(request_id, engine_failure(&error)).await?,
                }
            }
            UiIntent::SubmitPrompt { request_id, prompt } => {
                self.submit_prompt(request_id, prompt).await?;
            }
            UiIntent::CancelAttempt {
                request_id,
                attempt_id,
            } => {
                self.cancel_attempt(request_id, attempt_id).await?;
            }
            UiIntent::RetryAttempt {
                request_id,
                attempt_id,
            } => {
                self.retry_attempt(request_id, attempt_id).await?;
            }
        }
        Ok(())
    }

    async fn configure_credential(
        &mut self,
        request_id: RequestId,
        credential: ApiCredential,
    ) -> Result<(), AppError> {
        if self.active.is_some() {
            self.reject(
                request_id,
                UiFailure::new(
                    ErrorClass::Conflict,
                    "Wait for or cancel the active response before changing the API key",
                    RetryPolicy::Now,
                ),
            )
            .await?;
            return Ok(());
        }

        telemetry::credential_submitted();
        match (self.provider_factory)(credential) {
            Ok(provider) => {
                telemetry::provider_ready();
                self.provider = Some(provider);
                self.catalog_models.clear();
                self.ports
                    .catalogs
                    .send_replace(Arc::new(CatalogProjection::Loading));
                self.refresh_catalog(Some(request_id));
            }
            Err(error) => {
                telemetry::provider_unavailable(&error);
                self.reject(request_id, provider_failure(&error)).await?;
            }
        }
        Ok(())
    }

    async fn submit_prompt(
        &mut self,
        request_id: RequestId,
        prompt: String,
    ) -> Result<(), AppError> {
        if self.active.is_some() {
            self.reject(
                request_id,
                UiFailure::new(
                    ErrorClass::Conflict,
                    "A provider attempt is already active",
                    RetryPolicy::Now,
                ),
            )
            .await?;
            return Ok(());
        }
        if self.provider.is_none() {
            self.reject(
                request_id,
                UiFailure::new(
                    ErrorClass::Authentication,
                    "A provider API key is not configured",
                    RetryPolicy::Never,
                ),
            )
            .await?;
            return Ok(());
        }
        let prompt = self
            .provider
            .as_ref()
            .expect("provider presence checked before prompt admission")
            .redact_secrets(&prompt);
        let prompt = match PromptText::new(prompt) {
            Ok(prompt) => prompt,
            Err(error) => {
                self.reject(request_id, classified_failure(&error)).await?;
                return Ok(());
            }
        };
        if !self
            .session
            .selected_model()
            .is_some_and(|model| self.model_is_available(model))
        {
            self.reject(
                request_id,
                UiFailure::new(
                    ErrorClass::Validation,
                    "Choose a model from the current catalog before sending",
                    RetryPolicy::Never,
                ),
            )
            .await?;
            return Ok(());
        }

        let input_id = ids::input_id();
        let attempt_id = ids::attempt_id();
        if let Err(error) = self
            .execute(CommandPayload::AdmitPromptAndPrepareAttempt {
                session_id: self.session_id.clone(),
                attempt_id: attempt_id.clone(),
                input_id,
                prompt,
                delivery_mode: DeliveryMode::NextTurn,
            })
            .await
        {
            self.reject(request_id, engine_failure(&error)).await?;
            return Ok(());
        }
        telemetry::attempt_prepared();
        if let Err(error) = self.start_attempt(attempt_id).await {
            self.reject(request_id, start_attempt_failure(&error))
                .await?;
            return Ok(());
        }
        self.commit(request_id).await
    }

    async fn retry_attempt(
        &mut self,
        request_id: RequestId,
        attempt_key: AttemptKey,
    ) -> Result<(), AppError> {
        if self.active.is_some() {
            self.reject(
                request_id,
                UiFailure::new(
                    ErrorClass::Conflict,
                    "A provider attempt is already active",
                    RetryPolicy::Now,
                ),
            )
            .await?;
            return Ok(());
        }
        if self.provider.is_none() {
            self.reject(
                request_id,
                UiFailure::new(
                    ErrorClass::Authentication,
                    "A provider API key is not configured",
                    RetryPolicy::Never,
                ),
            )
            .await?;
            return Ok(());
        }
        if !self
            .session
            .selected_model()
            .is_some_and(|model| self.model_is_available(model))
        {
            self.reject(
                request_id,
                UiFailure::new(
                    ErrorClass::Validation,
                    "Choose a model from the current catalog before retrying",
                    RetryPolicy::Never,
                ),
            )
            .await?;
            return Ok(());
        }
        let prior = match AttemptId::new(attempt_key.as_str()) {
            Ok(attempt_id) => attempt_id,
            Err(error) => {
                self.reject(request_id, classified_failure(&error)).await?;
                return Ok(());
            }
        };
        let Some(input_id) = self
            .session
            .attempt(&prior)
            .map(|attempt| attempt.input_id().clone())
        else {
            self.reject(
                request_id,
                UiFailure::new(
                    ErrorClass::NotFound,
                    "The prior attempt no longer exists",
                    RetryPolicy::Never,
                ),
            )
            .await?;
            return Ok(());
        };
        let retry = ids::attempt_id();
        if let Err(error) = self
            .execute(CommandPayload::PrepareAttempt {
                session_id: self.session_id.clone(),
                attempt_id: retry.clone(),
                input_id,
                retry_of: Some(prior),
            })
            .await
        {
            self.reject(request_id, engine_failure(&error)).await?;
            return Ok(());
        }
        telemetry::attempt_prepared();
        if let Err(error) = self.start_attempt(retry).await {
            self.reject(request_id, start_attempt_failure(&error))
                .await?;
            return Ok(());
        }
        self.commit(request_id).await
    }

    async fn cancel_attempt(
        &mut self,
        request_id: RequestId,
        attempt_key: AttemptKey,
    ) -> Result<(), AppError> {
        let attempt_id = match AttemptId::new(attempt_key.as_str()) {
            Ok(attempt_id) => attempt_id,
            Err(error) => {
                self.reject(request_id, classified_failure(&error)).await?;
                return Ok(());
            }
        };
        let Some(active) = &self.active else {
            self.reject(
                request_id,
                UiFailure::new(
                    ErrorClass::Conflict,
                    "The attempt is no longer active",
                    RetryPolicy::Never,
                ),
            )
            .await?;
            return Ok(());
        };
        if active.attempt_id != attempt_id {
            self.reject(
                request_id,
                UiFailure::new(
                    ErrorClass::Conflict,
                    "A different provider attempt is active",
                    RetryPolicy::Never,
                ),
            )
            .await?;
            return Ok(());
        }
        let cancellation = active.cancellation.clone();
        match self
            .execute(CommandPayload::RequestAttemptCancellation {
                session_id: self.session_id.clone(),
                attempt_id,
            })
            .await
        {
            Ok(()) => {
                telemetry::cancellation_requested();
                cancellation.cancel();
                self.commit(request_id).await?;
            }
            Err(error) => self.reject(request_id, engine_failure(&error)).await?,
        }
        Ok(())
    }

    async fn start_attempt(&mut self, attempt_id: AttemptId) -> Result<(), StartAttemptError> {
        let request = match build_request(&self.session, &attempt_id) {
            Ok(request) => request,
            Err(error) => {
                self.execute(CommandPayload::FailAttempt {
                    session_id: self.session_id.clone(),
                    attempt_id,
                    failure: attempt_failure(&error),
                })
                .await
                .map_err(StartAttemptError::Engine)?;
                telemetry::attempt_settled("failed", Some(&error));
                return Err(StartAttemptError::Provider(error));
            }
        };
        self.execute(CommandPayload::StartAttempt {
            session_id: self.session_id.clone(),
            attempt_id: attempt_id.clone(),
        })
        .await
        .map_err(StartAttemptError::Engine)?;
        telemetry::attempt_started();
        let cancellation = self.shutdown.child_token();
        self.spawn_stream(attempt_id.clone(), request, cancellation.clone());
        self.active = Some(ActiveAttempt {
            attempt_id,
            cancellation,
        });
        Ok(())
    }

    async fn handle_async(&mut self, message: AsyncMessage) -> Result<(), AppError> {
        match message {
            AsyncMessage::Catalog {
                generation,
                request_id,
                result,
            } => self.handle_catalog(generation, request_id, result).await,
            AsyncMessage::Stream { attempt_id, result } => {
                self.handle_stream(attempt_id, result).await
            }
        }
    }

    async fn handle_catalog(
        &mut self,
        generation: u64,
        request_id: Option<RequestId>,
        result: Result<ModelCatalog, ProviderError>,
    ) -> Result<(), AppError> {
        if generation != self.catalog_generation {
            if let Some(request_id) = request_id {
                self.reject(
                    request_id,
                    UiFailure::new(
                        ErrorClass::Cancelled,
                        "The catalog refresh was superseded by a newer request",
                        RetryPolicy::Now,
                    ),
                )
                .await?;
            }
            return Ok(());
        }
        self.catalog_cancellation = None;
        match result {
            Ok(catalog) => {
                let stale = catalog.is_stale();
                let models = catalog.into_models();
                telemetry::catalog_ready(models.len());
                self.catalog_models = models.clone();
                self.ports
                    .catalogs
                    .send_replace(Arc::new(projection::catalog(models, stale)));
                if let Some(request_id) = request_id {
                    self.commit(request_id).await?;
                }
            }
            Err(error) => {
                telemetry::catalog_failed(&error);
                if matches!(
                    error.kind(),
                    ProviderErrorKind::MissingCredential
                        | ProviderErrorKind::Authentication
                        | ProviderErrorKind::PermissionDenied
                ) {
                    self.provider = None;
                }
                let failure = provider_failure(&error);
                self.ports
                    .catalogs
                    .send_replace(Arc::new(CatalogProjection::Failed(failure.clone())));
                if let Some(request_id) = request_id {
                    self.reject(request_id, failure).await?;
                }
            }
        }
        Ok(())
    }

    async fn handle_stream(
        &mut self,
        attempt_id: AttemptId,
        result: Result<ProviderStreamEvent, ProviderError>,
    ) -> Result<(), AppError> {
        let Some(active) = &self.active else {
            return Ok(());
        };
        if active.attempt_id != attempt_id {
            return Ok(());
        }
        let cancellation_requested = self
            .session
            .attempt(&attempt_id)
            .is_some_and(|attempt| attempt.status() == EngineAttemptStatus::CancellationRequested);

        match result {
            Ok(ProviderStreamEvent::Started) => {}
            Ok(ProviderStreamEvent::TextDelta(delta)) => {
                let bytes = delta.as_str().len();
                self.execute(CommandPayload::AppendAttemptText {
                    session_id: self.session_id.clone(),
                    attempt_id,
                    text: ResponseText::new(delta.as_str())
                        .expect("provider contract excludes empty deltas"),
                })
                .await?;
                telemetry::response_segment_committed(bytes);
            }
            Ok(ProviderStreamEvent::Usage(usage)) => {
                let input_tokens = usage.input_tokens;
                let output_tokens = usage.output_tokens;
                let total_tokens = usage.total_tokens;
                self.execute(CommandPayload::RecordAttemptUsage {
                    session_id: self.session_id.clone(),
                    attempt_id,
                    usage: DomainUsage::new(
                        usage.input_tokens,
                        usage.output_tokens,
                        usage.total_tokens,
                    )
                    .with_breakdown(
                        usage.cached_input_tokens,
                        usage.reasoning_tokens,
                        usage.tool_tokens,
                    ),
                })
                .await?;
                telemetry::usage_committed(input_tokens, output_tokens, total_tokens);
            }
            Ok(ProviderStreamEvent::Completed { reason }) => {
                telemetry::completion_observed(reason);
                let outcome = completion_outcome(reason);
                let payload = completion_payload(&self.session_id, attempt_id, reason);
                self.execute(payload).await?;
                self.active = None;
                telemetry::attempt_settled(outcome, None);
            }
            Ok(ProviderStreamEvent::Cancelled) if cancellation_requested => {
                self.execute(CommandPayload::CancelAttempt {
                    session_id: self.session_id.clone(),
                    attempt_id,
                })
                .await?;
                self.active = None;
                telemetry::attempt_settled("cancelled", None);
            }
            Ok(ProviderStreamEvent::Cancelled) => {
                self.execute(CommandPayload::FailAttempt {
                    session_id: self.session_id.clone(),
                    attempt_id,
                    failure: unsolicited_cancellation_failure(),
                })
                .await?;
                self.active = None;
                telemetry::attempt_settled("provider_cancelled", None);
            }
            Err(error) if cancellation_requested => {
                self.execute(CommandPayload::CancelAttempt {
                    session_id: self.session_id.clone(),
                    attempt_id,
                })
                .await?;
                self.active = None;
                telemetry::attempt_settled("cancelled", Some(&error));
            }
            Err(error) => {
                let (failure, outcome) = if error.kind() == ProviderErrorKind::Cancelled {
                    (unsolicited_cancellation_failure(), "provider_cancelled")
                } else {
                    (attempt_failure(&error), "failed")
                };
                self.execute(CommandPayload::FailAttempt {
                    session_id: self.session_id.clone(),
                    attempt_id,
                    failure,
                })
                .await?;
                self.active = None;
                telemetry::attempt_settled(outcome, Some(&error));
            }
        }
        Ok(())
    }

    async fn execute(&mut self, payload: CommandPayload) -> Result<(), DurableEngineError> {
        let reply = self.engine.execute(ids::command(payload)).await?;
        self.session = reply.session;
        self.ports
            .sessions
            .send_replace(Arc::new(projection::session(&self.session)));
        Ok(())
    }

    fn refresh_catalog(&mut self, request_id: Option<RequestId>) {
        let Some(provider) = self.provider.clone() else {
            return;
        };
        if let Some(cancellation) = self.catalog_cancellation.take() {
            cancellation.cancel();
        }
        self.catalog_generation = self.catalog_generation.saturating_add(1);
        let generation = self.catalog_generation;
        let cancellation = self.shutdown.child_token();
        self.catalog_cancellation = Some(cancellation.clone());
        telemetry::catalog_refresh_started(generation, request_id.is_some());
        let messages = self.messages.clone();
        tokio::spawn(async move {
            let request = if request_id.is_some() {
                CatalogRequest::Refresh
            } else {
                CatalogRequest::PreferCache
            };
            let result = provider.list_models(request, cancellation).await;
            let _ = messages
                .send(AsyncMessage::Catalog {
                    generation,
                    request_id,
                    result,
                })
                .await;
        });
    }

    fn model_is_available(&self, model: &autoharness_domain::ModelRef) -> bool {
        self.catalog_models.iter().any(|descriptor| {
            descriptor.provider_id == *model.provider_id()
                && descriptor.model_id == *model.model_id()
                && descriptor.capabilities.supports_streamed_chat()
        })
    }

    fn spawn_stream(
        &self,
        attempt_id: AttemptId,
        request: ChatRequest,
        cancellation: CancellationToken,
    ) {
        let provider = Arc::clone(
            self.provider
                .as_ref()
                .expect("provider checked before attempt"),
        );
        let messages = self.messages.clone();
        tokio::spawn(async move {
            match provider.stream_chat(request, cancellation).await {
                Ok(mut stream) => {
                    while let Some(result) = stream.next().await {
                        let terminal = match &result {
                            Err(_) => true,
                            Ok(event) => matches!(
                                event,
                                ProviderStreamEvent::Completed { .. }
                                    | ProviderStreamEvent::Cancelled
                            ),
                        };
                        if messages
                            .send(AsyncMessage::Stream {
                                attempt_id: attempt_id.clone(),
                                result,
                            })
                            .await
                            .is_err()
                        {
                            return;
                        }
                        if terminal {
                            return;
                        }
                    }
                    let _ = messages
                        .send(AsyncMessage::Stream {
                            attempt_id,
                            result: Err(ProviderError::new(
                                ProviderErrorKind::Protocol,
                                RetryAdvice::Never,
                            )),
                        })
                        .await;
                }
                Err(error) => {
                    let _ = messages
                        .send(AsyncMessage::Stream {
                            attempt_id,
                            result: Err(error),
                        })
                        .await;
                }
            }
        });
    }

    async fn commit(&self, request_id: RequestId) -> Result<(), AppError> {
        self.ports
            .notices
            .send(UiNotice::IntentCommitted { request_id })
            .await
            .map_err(|_| AppError::WorkerStopped)
    }

    async fn reject(&self, request_id: RequestId, failure: UiFailure) -> Result<(), AppError> {
        self.ports
            .notices
            .send(UiNotice::IntentRejected {
                request_id,
                failure,
            })
            .await
            .map_err(|_| AppError::WorkerStopped)
    }
}

fn build_request(
    session: &SessionAggregate,
    attempt_id: &AttemptId,
) -> Result<ChatRequest, ProviderError> {
    let attempt = session
        .attempt(attempt_id)
        .ok_or_else(|| ProviderError::new(ProviderErrorKind::Internal, RetryAdvice::Never))?;
    let mut messages = Vec::new();
    for input in session
        .admitted_inputs()
        .iter()
        .filter(|input| input.promoted_by().is_some())
    {
        messages.push(ChatMessage {
            role: ChatRole::User,
            content: ChatContent::new(input.prompt().as_str())?,
        });
        for response in session.attempts().iter().filter(|candidate| {
            candidate.input_id() == input.input_id()
                && candidate.status() == EngineAttemptStatus::Completed
        }) {
            let text = response.response_text();
            if !text.trim().is_empty() {
                messages.push(ChatMessage {
                    role: ChatRole::Assistant,
                    content: ChatContent::new(text)?,
                });
            }
        }
    }
    ChatRequest::new(attempt.model().model_id().clone(), messages)
}

fn attempt_failure(error: &ProviderError) -> AttemptFailure {
    AttemptFailure::new(
        error.class(),
        ErrorCode::new(provider_code(error.kind())).expect("provider error codes are valid"),
        PublicMessage::new(error.to_string()).expect("provider errors have safe messages"),
        error.retry_advice(),
    )
}

fn completion_payload(
    session_id: &SessionId,
    attempt_id: AttemptId,
    reason: autoharness_provider::CompletionReason,
) -> CommandPayload {
    use autoharness_provider::CompletionReason;

    match reason {
        CompletionReason::Stop => CommandPayload::CompleteAttempt {
            session_id: session_id.clone(),
            attempt_id,
        },
        CompletionReason::Length => CommandPayload::FailAttempt {
            session_id: session_id.clone(),
            attempt_id,
            failure: completion_failure(
                ErrorClass::Validation,
                "generation_limit",
                "The response stopped at the provider generation limit",
                RetryAdvice::Immediate,
            ),
        },
        CompletionReason::Safety => CommandPayload::FailAttempt {
            session_id: session_id.clone(),
            attempt_id,
            failure: completion_failure(
                ErrorClass::PermissionDenied,
                "safety_stop",
                "The provider stopped the response for safety policy",
                RetryAdvice::Never,
            ),
        },
        CompletionReason::Recitation => CommandPayload::FailAttempt {
            session_id: session_id.clone(),
            attempt_id,
            failure: completion_failure(
                ErrorClass::PermissionDenied,
                "recitation_stop",
                "The provider stopped the response for recitation policy",
                RetryAdvice::Never,
            ),
        },
        CompletionReason::Other => CommandPayload::FailAttempt {
            session_id: session_id.clone(),
            attempt_id,
            failure: completion_failure(
                ErrorClass::Protocol,
                "provider_stop",
                "The provider stopped the response without a normal completion",
                RetryAdvice::Never,
            ),
        },
    }
}

const fn completion_outcome(reason: autoharness_provider::CompletionReason) -> &'static str {
    use autoharness_provider::CompletionReason;

    match reason {
        CompletionReason::Stop => "completed",
        CompletionReason::Length => "generation_limit",
        CompletionReason::Safety => "safety_stop",
        CompletionReason::Recitation => "recitation_stop",
        CompletionReason::Other => "provider_stop",
    }
}

fn completion_failure(
    class: ErrorClass,
    code: &'static str,
    message: &'static str,
    retry_advice: RetryAdvice,
) -> AttemptFailure {
    AttemptFailure::new(
        class,
        ErrorCode::new(code).expect("static completion code is valid"),
        PublicMessage::new(message).expect("static completion message is valid"),
        retry_advice,
    )
}

fn unsolicited_cancellation_failure() -> AttemptFailure {
    completion_failure(
        ErrorClass::Cancelled,
        "provider_cancelled",
        "The provider cancelled the attempt before completion",
        RetryAdvice::Immediate,
    )
}

fn provider_code(kind: ProviderErrorKind) -> &'static str {
    match kind {
        ProviderErrorKind::MissingCredential => "missing_credential",
        ProviderErrorKind::Authentication => "authentication",
        ProviderErrorKind::PermissionDenied => "permission_denied",
        ProviderErrorKind::InvalidRequest => "invalid_request",
        ProviderErrorKind::ModelNotFound => "model_not_found",
        ProviderErrorKind::Unsupported => "unsupported",
        ProviderErrorKind::RateLimited => "rate_limited",
        ProviderErrorKind::QuotaExceeded => "quota_exceeded",
        ProviderErrorKind::Timeout => "timeout",
        ProviderErrorKind::Unavailable => "unavailable",
        ProviderErrorKind::Conflict => "conflict",
        ProviderErrorKind::ContentBlocked => "content_blocked",
        ProviderErrorKind::Cancelled => "cancelled",
        ProviderErrorKind::Transport => "transport",
        ProviderErrorKind::Protocol => "protocol",
        ProviderErrorKind::LimitExceeded => "limit_exceeded",
        ProviderErrorKind::Internal => "internal",
    }
}

fn classified_failure(error: &(impl ClassifiedError + std::fmt::Display)) -> UiFailure {
    UiFailure::new(
        error.class(),
        error.to_string(),
        RetryPolicy::from_advice(error.retry_advice(), 0),
    )
}

fn engine_failure(error: &DurableEngineError) -> UiFailure {
    classified_failure(error)
}

fn provider_failure(error: &ProviderError) -> UiFailure {
    classified_failure(error)
}

fn start_attempt_failure(error: &StartAttemptError) -> UiFailure {
    match error {
        StartAttemptError::Engine(error) => engine_failure(error),
        StartAttemptError::Provider(error) => provider_failure(error),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{LazyLock, Mutex};
    use std::time::Duration;

    use autoharness_domain::{
        Causation, CommandId, CorrelationId, EventEnvelope, EventId, EventPayload, InputId,
        ModelId, ModelRef, ProviderId, SessionSequence, TimestampMillis,
    };
    use autoharness_provider::{
        CapabilitySupport, Catalog, CatalogFreshness, CatalogRequest, Chat, CompletionReason,
        ModelCapabilities, ModelCatalog, ProviderAvailability, ProviderEventStream,
        ProviderMetadata, TextDelta, UsageSnapshot as ProviderUsage,
    };
    use autoharness_provider_openai::{OpenAiRouterProvider, RouterCredential, RouterSettings};
    use autoharness_tui::{SessionProjection, TranscriptItem, UiPorts, bounded_ports};
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::watch;

    use super::*;

    static FAKE_PROVIDER_ID: LazyLock<ProviderId> =
        LazyLock::new(|| ProviderId::new("google-ai-studio").expect("fixture provider ID"));

    #[derive(Default)]
    struct FakeProvider {
        calls: AtomicUsize,
        requests: Mutex<Vec<ChatRequest>>,
    }

    #[async_trait::async_trait]
    impl Catalog for FakeProvider {
        async fn list_models(
            &self,
            _request: CatalogRequest,
            _cancellation: CancellationToken,
        ) -> Result<ModelCatalog, ProviderError> {
            Ok(ModelCatalog::new(
                vec![fixture_model_descriptor()],
                CatalogFreshness::Live,
            ))
        }
    }

    #[async_trait::async_trait]
    impl Chat for FakeProvider {
        async fn stream_chat(
            &self,
            request: ChatRequest,
            cancellation: CancellationToken,
        ) -> Result<ProviderEventStream, ProviderError> {
            self.requests.lock().expect("request lock").push(request);
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            if call == 0 {
                Ok(Box::pin(async_stream::stream! {
                    yield Ok(ProviderStreamEvent::Started);
                    yield Ok(ProviderStreamEvent::TextDelta(
                        TextDelta::new("partial").expect("text delta"),
                    ));
                    cancellation.cancelled().await;
                    yield Ok(ProviderStreamEvent::Cancelled);
                }))
            } else {
                Ok(Box::pin(futures_util::stream::iter([
                    Ok(ProviderStreamEvent::Started),
                    Ok(ProviderStreamEvent::TextDelta(
                        TextDelta::new("recovered").expect("text delta"),
                    )),
                    Ok(ProviderStreamEvent::Usage(ProviderUsage {
                        input_tokens: Some(3),
                        output_tokens: Some(2),
                        cached_input_tokens: Some(1),
                        reasoning_tokens: Some(1),
                        tool_tokens: Some(0),
                        total_tokens: Some(5),
                    })),
                    Ok(ProviderStreamEvent::Completed {
                        reason: CompletionReason::Stop,
                    }),
                ])))
            }
        }
    }

    impl autoharness_provider::SecretRedactor for FakeProvider {
        fn redact_secrets(&self, value: &str) -> String {
            value.replace("test-api-secret", "[REDACTED]")
        }
    }

    impl ProviderMetadata for FakeProvider {
        fn provider_id(&self) -> &ProviderId {
            &FAKE_PROVIDER_ID
        }

        fn availability(&self) -> ProviderAvailability {
            ProviderAvailability::Ready
        }
    }

    #[derive(Default)]
    struct UnsolicitedCancellationProvider {
        calls: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl Catalog for UnsolicitedCancellationProvider {
        async fn list_models(
            &self,
            _request: CatalogRequest,
            _cancellation: CancellationToken,
        ) -> Result<ModelCatalog, ProviderError> {
            Ok(ModelCatalog::new(
                vec![fixture_model_descriptor()],
                CatalogFreshness::Live,
            ))
        }
    }

    #[async_trait::async_trait]
    impl Chat for UnsolicitedCancellationProvider {
        async fn stream_chat(
            &self,
            _request: ChatRequest,
            _cancellation: CancellationToken,
        ) -> Result<ProviderEventStream, ProviderError> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            let events = if call == 0 {
                vec![
                    Ok(ProviderStreamEvent::Started),
                    Ok(ProviderStreamEvent::Cancelled),
                ]
            } else {
                vec![
                    Ok(ProviderStreamEvent::Started),
                    Ok(ProviderStreamEvent::TextDelta(
                        TextDelta::new("recovered").expect("text delta"),
                    )),
                    Ok(ProviderStreamEvent::Completed {
                        reason: CompletionReason::Stop,
                    }),
                ]
            };
            Ok(Box::pin(futures_util::stream::iter(events)))
        }
    }

    impl autoharness_provider::SecretRedactor for UnsolicitedCancellationProvider {
        fn redact_secrets(&self, value: &str) -> String {
            value.to_owned()
        }
    }

    impl ProviderMetadata for UnsolicitedCancellationProvider {
        fn provider_id(&self) -> &ProviderId {
            &FAKE_PROVIDER_ID
        }
    }

    struct AuthenticationProvider;

    #[async_trait::async_trait]
    impl Catalog for AuthenticationProvider {
        async fn list_models(
            &self,
            _request: CatalogRequest,
            _cancellation: CancellationToken,
        ) -> Result<ModelCatalog, ProviderError> {
            Err(ProviderError::new(
                ProviderErrorKind::Authentication,
                RetryAdvice::Never,
            ))
        }
    }

    #[async_trait::async_trait]
    impl Chat for AuthenticationProvider {
        async fn stream_chat(
            &self,
            _request: ChatRequest,
            _cancellation: CancellationToken,
        ) -> Result<ProviderEventStream, ProviderError> {
            Err(ProviderError::new(
                ProviderErrorKind::Authentication,
                RetryAdvice::Never,
            ))
        }
    }

    impl autoharness_provider::SecretRedactor for AuthenticationProvider {
        fn redact_secrets(&self, value: &str) -> String {
            value.to_owned()
        }
    }

    impl ProviderMetadata for AuthenticationProvider {
        fn provider_id(&self) -> &ProviderId {
            &FAKE_PROVIDER_ID
        }
    }

    fn fixture_model() -> ModelRef {
        ModelRef::new(
            ProviderId::new("google-ai-studio").expect("provider ID"),
            ModelId::new("models/gemini-fixture").expect("model ID"),
        )
    }

    fn fixture_model_descriptor() -> ModelDescriptor {
        let model = fixture_model();
        ModelDescriptor {
            provider_id: model.provider_id().clone(),
            model_id: model.model_id().clone(),
            display_name: "Gemini fixture".to_owned(),
            description: None,
            input_token_limit: Some(1_024),
            output_token_limit: Some(1_024),
            capabilities: ModelCapabilities {
                chat: CapabilitySupport::Supported,
                streaming: CapabilitySupport::Supported,
                managed_interactions: CapabilitySupport::Unknown,
                thinking: CapabilitySupport::Unknown,
            },
        }
    }

    async fn wait_for_catalog(ui: &mut UiPorts) {
        loop {
            if matches!(
                &**ui.catalogs.borrow_and_update(),
                CatalogProjection::Ready { .. }
            ) {
                return;
            }
            tokio::time::timeout(Duration::from_secs(5), ui.catalogs.changed())
                .await
                .expect("catalog timeout")
                .expect("catalog sender remains open");
        }
    }

    async fn wait_for_session(
        sessions: &mut watch::Receiver<Arc<SessionProjection>>,
        predicate: impl Fn(&SessionProjection) -> bool,
    ) -> Arc<SessionProjection> {
        loop {
            let current = Arc::clone(&sessions.borrow_and_update());
            if predicate(&current) {
                return current;
            }
            tokio::time::timeout(Duration::from_secs(5), sessions.changed())
                .await
                .expect("session timeout")
                .expect("session sender remains open");
        }
    }

    async fn expect_commit(ui: &mut UiPorts, request_id: RequestId) {
        let notice = tokio::time::timeout(Duration::from_secs(5), ui.notices.recv())
            .await
            .expect("notice timeout")
            .expect("notice sender remains open");
        assert_eq!(notice, UiNotice::IntentCommitted { request_id });
    }

    #[test]
    fn provider_request_excludes_partial_failed_assistant_output() {
        let session_id = SessionId::new("session-1").expect("valid session ID");
        let input_id = InputId::new("input-1").expect("valid input ID");
        let attempt_id = AttemptId::new("attempt-1").expect("valid attempt ID");
        let model = ModelRef::new(
            ProviderId::new("gemini").expect("valid provider ID"),
            ModelId::new("models/gemini-test").expect("valid model ID"),
        );
        let payloads = [
            EventPayload::SessionCreated,
            EventPayload::ModelSelected {
                model: model.clone(),
            },
            EventPayload::InputAdmitted {
                input_id: input_id.clone(),
                prompt: PromptText::new("hello").expect("valid prompt"),
                delivery_mode: DeliveryMode::NextTurn,
            },
            EventPayload::AttemptPrepared {
                attempt_id: attempt_id.clone(),
                input_id,
                model,
                retry_of: None,
            },
            EventPayload::AttemptStarted {
                attempt_id: attempt_id.clone(),
            },
            EventPayload::AttemptTextAppended {
                attempt_id: attempt_id.clone(),
                text: ResponseText::new("partial secret").expect("valid response"),
            },
        ];
        let events: Vec<_> = payloads
            .into_iter()
            .enumerate()
            .map(|(index, payload)| {
                EventEnvelope::new_v1(
                    EventId::new(format!("event-{index}")).expect("valid event ID"),
                    session_id.clone(),
                    SessionSequence::new(index as u64 + 1).expect("valid sequence"),
                    TimestampMillis::new(index as i64),
                    Causation::Command(
                        CommandId::new(format!("command-{index}")).expect("valid command ID"),
                    ),
                    CorrelationId::new(format!("correlation-{index}"))
                        .expect("valid correlation ID"),
                    payload,
                )
            })
            .collect();
        let aggregate = SessionAggregate::rehydrate(session_id, &events).expect("valid history");

        let request = build_request(&aggregate, &attempt_id).expect("valid provider request");

        assert_eq!(request.messages.len(), 1);
        assert_eq!(request.messages[0].content.as_str(), "hello");
    }

    #[test]
    fn non_normal_provider_stops_become_visible_durable_failures() {
        let attempt_id = AttemptId::new("attempt-stop").expect("attempt ID");

        let length = completion_payload(
            &SessionId::new("session-stop").expect("session ID"),
            attempt_id.clone(),
            CompletionReason::Length,
        );
        let safety = completion_payload(
            &SessionId::new("session-stop").expect("session ID"),
            attempt_id,
            CompletionReason::Safety,
        );

        assert!(matches!(
            length,
            CommandPayload::FailAttempt { failure, .. }
                if failure.code().as_str() == "generation_limit"
                    && failure.retry_advice() == RetryAdvice::Immediate
        ));
        assert!(matches!(
            safety,
            CommandPayload::FailAttempt { failure, .. }
                if failure.code().as_str() == "safety_stop"
                    && failure.retry_advice() == RetryAdvice::Never
        ));
    }

    #[tokio::test]
    async fn in_app_credential_configures_provider_without_persisting_the_key() {
        let sentinel = "gemini-in-app-secret-sentinel";
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("credential.sqlite3");
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database).expect("engine actor");
        let provider = Arc::new(FakeProvider::default());
        let provider_for_factory = provider.clone();
        let factory: ProviderFactory = Arc::new(move |credential| {
            let key = GeminiApiKey::new(credential.into_string())?;
            assert_eq!(format!("{key:?}"), "GeminiApiKey([REDACTED])");
            Ok(provider_for_factory.clone())
        });
        let initial_session = Arc::new(projection::session(&session));
        let (mut ui, app) = bounded_ports(
            initial_session,
            Arc::new(CatalogProjection::CredentialRequired),
        );
        let shutdown = CancellationToken::new();
        let coordinator = Coordinator::with_provider_factory(
            session_id,
            session,
            actor.handle(),
            None,
            factory,
            app,
            shutdown.clone(),
        );
        let task = tokio::spawn(coordinator.run());

        let request_id = RequestId::new(1);
        ui.intents
            .send(UiIntent::ConfigureCredential {
                request_id,
                credential: ApiCredential::new(sentinel.to_owned()).expect("fixture credential"),
            })
            .await
            .expect("credential intent");
        wait_for_catalog(&mut ui).await;
        expect_commit(&mut ui, request_id).await;

        shutdown.cancel();
        task.await
            .expect("coordinator join")
            .expect("coordinator shutdown");
        actor.shutdown().await.expect("actor shutdown");

        for entry in std::fs::read_dir(directory.path()).expect("read data directory") {
            let path = entry.expect("data file entry").path();
            if path.is_file() {
                let bytes = std::fs::read(path).expect("read data file");
                assert!(
                    !bytes
                        .windows(sentinel.len())
                        .any(|window| window == sentinel.as_bytes()),
                    "in-app credential must not reach durable files"
                );
            }
        }
    }

    #[tokio::test]
    async fn rejected_in_app_credential_can_be_replaced_without_restarting() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("credential-retry.sqlite3");
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database).expect("engine actor");
        let valid_provider = Arc::new(FakeProvider::default());
        let valid_provider_for_factory = valid_provider.clone();
        let calls = Arc::new(AtomicUsize::new(0));
        let factory_calls = calls.clone();
        let factory: ProviderFactory = Arc::new(move |credential| {
            let _key = GeminiApiKey::new(credential.into_string())?;
            if factory_calls.fetch_add(1, Ordering::SeqCst) == 0 {
                Ok(Arc::new(AuthenticationProvider))
            } else {
                Ok(valid_provider_for_factory.clone())
            }
        });
        let initial_session = Arc::new(projection::session(&session));
        let (mut ui, app) = bounded_ports(
            initial_session,
            Arc::new(CatalogProjection::CredentialRequired),
        );
        let shutdown = CancellationToken::new();
        let coordinator = Coordinator::with_provider_factory(
            session_id,
            session,
            actor.handle(),
            None,
            factory,
            app,
            shutdown.clone(),
        );
        let task = tokio::spawn(coordinator.run());

        let rejected_request = RequestId::new(1);
        ui.intents
            .send(UiIntent::ConfigureCredential {
                request_id: rejected_request,
                credential: ApiCredential::new("syntactically-valid-key".to_owned())
                    .expect("fixture credential"),
            })
            .await
            .expect("credential intent");
        let rejected = tokio::time::timeout(Duration::from_secs(5), ui.notices.recv())
            .await
            .expect("notice timeout")
            .expect("notice sender remains open");
        assert!(matches!(
            rejected,
            UiNotice::IntentRejected {
                request_id,
                failure: UiFailure {
                    class: ErrorClass::Authentication,
                    ..
                },
            } if request_id == rejected_request
        ));

        let accepted_request = RequestId::new(2);
        ui.intents
            .send(UiIntent::ConfigureCredential {
                request_id: accepted_request,
                credential: ApiCredential::new("replacement-key".to_owned())
                    .expect("fixture credential"),
            })
            .await
            .expect("replacement credential intent");
        wait_for_catalog(&mut ui).await;
        expect_commit(&mut ui, accepted_request).await;
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        shutdown.cancel();
        task.await
            .expect("coordinator join")
            .expect("coordinator shutdown");
        actor.shutdown().await.expect("actor shutdown");
    }

    #[tokio::test]
    async fn configured_router_runs_the_same_catalog_and_session_path() {
        let (base_url, server) = spawn_router_fixture().await;
        let settings =
            RouterSettings::new(base_url.parse().expect("fixture URL"), Some("composed"))
                .expect("router settings")
                .with_authentication("x-router-key", "Token")
                .expect("router authentication");
        let provider = Arc::new(
            OpenAiRouterProvider::new(
                settings,
                RouterCredential::new("router-composed-secret").expect("credential"),
            )
            .expect("router provider"),
        );
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("router-composition.sqlite3");
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database).expect("engine actor");
        let initial_session = Arc::new(projection::session(&session));
        let (mut ui, app) = bounded_ports(initial_session, Arc::new(CatalogProjection::Loading));
        let shutdown = CancellationToken::new();
        let coordinator = Coordinator::new(
            session_id,
            session,
            actor.handle(),
            Some(provider),
            app,
            shutdown.clone(),
        );
        let task = tokio::spawn(coordinator.run());

        wait_for_catalog(&mut ui).await;
        let model = ModelRef::new(
            ProviderId::new("router:composed").expect("provider ID"),
            ModelId::new("router-model").expect("model ID"),
        );
        let select_request = RequestId::new(1);
        ui.intents
            .send(UiIntent::SelectModel {
                request_id: select_request,
                model: model.clone(),
            })
            .await
            .expect("select model");
        expect_commit(&mut ui, select_request).await;
        let submit_request = RequestId::new(2);
        ui.intents
            .send(UiIntent::SubmitPrompt {
                request_id: submit_request,
                prompt: "router composed prompt".to_owned(),
            })
            .await
            .expect("submit prompt");
        expect_commit(&mut ui, submit_request).await;
        let completed = wait_for_session(&mut ui.sessions, |projection| {
            projection.transcript.iter().any(|item| {
                matches!(
                    item,
                    TranscriptItem::Assistant {
                        text,
                        status: autoharness_tui::AttemptStatus::Completed,
                        ..
                    } if text == "router response"
                )
            })
        })
        .await;
        assert_eq!(completed.selected_model.as_ref(), Some(&model));

        shutdown.cancel();
        task.await.expect("coordinator join").expect("shutdown");
        actor.shutdown().await.expect("actor shutdown");
        let requests = server.await.expect("fixture server");
        assert_eq!(
            requests,
            vec!["GET /v1/models?limit=1000", "POST /v1/chat/completions"]
        );
    }

    #[tokio::test]
    async fn composed_cancel_retry_and_restart_path_is_replay_equivalent() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("composition.sqlite3");
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database.clone()).expect("engine actor");
        let provider = Arc::new(FakeProvider::default());
        let initial_session = Arc::new(projection::session(&session));
        let (mut ui, app) = bounded_ports(initial_session, Arc::new(CatalogProjection::Loading));
        let shutdown = CancellationToken::new();
        let coordinator = Coordinator::new(
            session_id.clone(),
            session,
            actor.handle(),
            Some(provider.clone()),
            app,
            shutdown.clone(),
        );
        let task = tokio::spawn(coordinator.run());

        wait_for_catalog(&mut ui).await;
        let select_request = RequestId::new(1);
        ui.intents
            .send(UiIntent::SelectModel {
                request_id: select_request,
                model: fixture_model(),
            })
            .await
            .expect("select intent");
        expect_commit(&mut ui, select_request).await;

        let submit_request = RequestId::new(2);
        ui.intents
            .send(UiIntent::SubmitPrompt {
                request_id: submit_request,
                prompt: "exact test-api-secret user prompt".to_owned(),
            })
            .await
            .expect("submit intent");
        expect_commit(&mut ui, submit_request).await;
        let streaming = wait_for_session(&mut ui.sessions, |projection| {
            projection.transcript.iter().any(|item| {
                matches!(
                    item,
                    TranscriptItem::Assistant {
                        text,
                        status: autoharness_tui::AttemptStatus::Streaming,
                        ..
                    } if text == "partial"
                )
            })
        })
        .await;
        let first_attempt = streaming
            .transcript
            .iter()
            .find_map(|item| match item {
                TranscriptItem::Assistant { attempt_id, .. } => Some(attempt_id.clone()),
                TranscriptItem::User { .. } => None,
            })
            .expect("streaming attempt");

        let cancel_request = RequestId::new(3);
        ui.intents
            .send(UiIntent::CancelAttempt {
                request_id: cancel_request,
                attempt_id: first_attempt.clone(),
            })
            .await
            .expect("cancel intent");
        expect_commit(&mut ui, cancel_request).await;
        wait_for_session(&mut ui.sessions, |projection| {
            projection.transcript.iter().any(|item| {
                matches!(
                    item,
                    TranscriptItem::Assistant {
                        attempt_id,
                        status: autoharness_tui::AttemptStatus::Cancelled,
                        ..
                    } if attempt_id == &first_attempt
                )
            })
        })
        .await;

        let retry_request = RequestId::new(4);
        ui.intents
            .send(UiIntent::RetryAttempt {
                request_id: retry_request,
                attempt_id: first_attempt,
            })
            .await
            .expect("retry intent");
        expect_commit(&mut ui, retry_request).await;
        let completed = wait_for_session(&mut ui.sessions, |projection| {
            projection.transcript.iter().any(|item| {
                matches!(
                    item,
                    TranscriptItem::Assistant {
                        text,
                        status: autoharness_tui::AttemptStatus::Completed,
                        retry_of: Some(_),
                        ..
                    } if text == "recovered"
                )
            })
        })
        .await;
        assert_eq!(completed.selected_model.as_ref(), Some(&fixture_model()));

        shutdown.cancel();
        task.await
            .expect("coordinator join")
            .expect("coordinator shutdown");
        actor.shutdown().await.expect("actor shutdown");

        let (reopened, recovered_session_id, recovered) =
            crate::engine_actor::EngineActor::start(database).expect("reopen engine actor");
        assert_eq!(recovered_session_id, session_id);
        assert_eq!(projection::session(&recovered), *completed);
        let recovered_usage = recovered
            .attempts()
            .last()
            .and_then(|attempt| attempt.usage())
            .expect("recovered usage");
        assert_eq!(recovered_usage.cached_input_tokens(), Some(1));
        assert_eq!(recovered_usage.reasoning_tokens(), Some(1));
        assert_eq!(recovered_usage.tool_tokens(), Some(0));
        reopened.shutdown().await.expect("reopened actor shutdown");

        let requests = provider.requests.lock().expect("request lock");
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[1].messages.len(), 1);
        assert_eq!(
            requests[1].messages[0].content.as_str(),
            "exact [REDACTED] user prompt"
        );
        drop(requests);
        for entry in std::fs::read_dir(directory.path()).expect("read data directory") {
            let path = entry.expect("data file entry").path();
            if path.is_file() {
                let bytes = std::fs::read(path).expect("read data file");
                assert!(
                    !bytes
                        .windows("test-api-secret".len())
                        .any(|window| window == b"test-api-secret"),
                    "provider credential sentinel must not reach durable files"
                );
            }
        }
    }

    #[tokio::test]
    async fn unsolicited_provider_cancellation_is_a_retryable_failure() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("unsolicited-cancellation.sqlite3");
        let (actor, session_id, session) =
            crate::engine_actor::EngineActor::start(database).expect("engine actor");
        let provider = Arc::new(UnsolicitedCancellationProvider::default());
        let initial_session = Arc::new(projection::session(&session));
        let (mut ui, app) = bounded_ports(initial_session, Arc::new(CatalogProjection::Loading));
        let shutdown = CancellationToken::new();
        let coordinator = Coordinator::new(
            session_id,
            session,
            actor.handle(),
            Some(provider.clone()),
            app,
            shutdown.clone(),
        );
        let task = tokio::spawn(coordinator.run());

        wait_for_catalog(&mut ui).await;
        let select_request = RequestId::new(1);
        ui.intents
            .send(UiIntent::SelectModel {
                request_id: select_request,
                model: fixture_model(),
            })
            .await
            .expect("select intent");
        expect_commit(&mut ui, select_request).await;

        let submit_request = RequestId::new(2);
        ui.intents
            .send(UiIntent::SubmitPrompt {
                request_id: submit_request,
                prompt: "retry provider cancellation".to_owned(),
            })
            .await
            .expect("submit intent");
        expect_commit(&mut ui, submit_request).await;
        let failed = wait_for_session(&mut ui.sessions, |projection| {
            projection.transcript.iter().any(|item| {
                matches!(
                    item,
                    TranscriptItem::Assistant {
                        status: autoharness_tui::AttemptStatus::Failed(UiFailure {
                            class: ErrorClass::Cancelled,
                            retry: RetryPolicy::Now,
                            ..
                        }),
                        ..
                    }
                )
            })
        })
        .await;
        let failed_attempt = failed
            .transcript
            .iter()
            .find_map(|item| match item {
                TranscriptItem::Assistant { attempt_id, .. } => Some(attempt_id.clone()),
                TranscriptItem::User { .. } => None,
            })
            .expect("failed attempt");

        let retry_request = RequestId::new(3);
        ui.intents
            .send(UiIntent::RetryAttempt {
                request_id: retry_request,
                attempt_id: failed_attempt,
            })
            .await
            .expect("retry intent");
        expect_commit(&mut ui, retry_request).await;
        wait_for_session(&mut ui.sessions, |projection| {
            projection.transcript.iter().any(|item| {
                matches!(
                    item,
                    TranscriptItem::Assistant {
                        text,
                        status: autoharness_tui::AttemptStatus::Completed,
                        retry_of: Some(_),
                        ..
                    } if text == "recovered"
                )
            })
        })
        .await;
        assert_eq!(provider.calls.load(Ordering::SeqCst), 2);

        shutdown.cancel();
        task.await
            .expect("coordinator join")
            .expect("coordinator shutdown");
        actor.shutdown().await.expect("actor shutdown");
    }

    async fn spawn_router_fixture() -> (String, tokio::task::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind router fixture");
        let address = listener.local_addr().expect("fixture address");
        let base_url = format!("http://{address}/");
        let task = tokio::spawn(async move {
            let mut requests = Vec::new();
            for (content_type, body) in [
                (
                    "application/json",
                    r#"{"data":[{"id":"router-model","name":"Router model","capabilities":{"chat":true,"streaming":true}}],"has_more":false}"#,
                ),
                (
                    "text/event-stream",
                    concat!(
                        "data: {\"choices\":[{\"delta\":{\"content\":\"router response\"},\"finish_reason\":\"stop\"}]}\n\n",
                        "data: [DONE]\n\n"
                    ),
                ),
            ] {
                let (mut socket, _) = listener.accept().await.expect("fixture request");
                requests.push(read_router_request(&mut socket).await);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket
                    .write_all(response.as_bytes())
                    .await
                    .expect("fixture response");
            }
            requests
        });
        (base_url, task)
    }

    async fn read_router_request(socket: &mut TcpStream) -> String {
        let mut bytes = Vec::new();
        let header_end = loop {
            if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                break index + 4;
            }
            let mut chunk = [0_u8; 4096];
            let read = socket.read(&mut chunk).await.expect("fixture read");
            assert_ne!(read, 0);
            bytes.extend_from_slice(&chunk[..read]);
        };
        let (request_line, content_length) = {
            let headers = std::str::from_utf8(&bytes[..header_end]).expect("request headers");
            let request_line = headers
                .lines()
                .next()
                .expect("request line")
                .trim_end_matches(" HTTP/1.1")
                .to_owned();
            let content_length = headers
                .lines()
                .find_map(|line| {
                    line.strip_prefix("content-length:")
                        .or_else(|| line.strip_prefix("Content-Length:"))
                })
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            (request_line, content_length)
        };
        while bytes.len() < header_end.saturating_add(content_length) {
            let mut chunk = [0_u8; 4096];
            let read = socket.read(&mut chunk).await.expect("fixture body");
            assert_ne!(read, 0);
            bytes.extend_from_slice(&chunk[..read]);
        }
        request_line
    }
}
