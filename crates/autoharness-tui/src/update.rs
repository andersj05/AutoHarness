use std::sync::Arc;

use autoharness_domain::ErrorClass;
use ratatui_textarea::{Input, Key};

use crate::model::{
    AttemptKey, CatalogProjection, Focus, Message, Model, Notice, PendingKind, RetryPolicy,
    SessionProjection, UiEffect, UiFailure, UiIntent, UiNotice,
};
use crate::text::{display_safe, editable_safe};

/// Applies one input to local UI state and returns application-owned effects.
#[must_use]
pub fn update(model: &mut Model, message: Message) -> Vec<UiEffect> {
    match message {
        Message::Input(input) => handle_input(model, input),
        Message::Paste(text) => {
            let text = zeroize::Zeroizing::new(text);
            handle_paste(model, &text);
            Vec::new()
        }
        Message::SessionChanged(session) => {
            apply_session(model, session);
            Vec::new()
        }
        Message::CatalogChanged(catalog) => {
            apply_catalog(model, catalog);
            Vec::new()
        }
        Message::Notice(notice) => {
            apply_notice(model, notice);
            Vec::new()
        }
        Message::Tick(now) => {
            model.now = now;
            if model.session.active_attempt().is_some()
                || model.session.retryable_attempt().is_some_and(|(_, retry)| {
                    matches!(retry, RetryPolicy::After { .. } | RetryPolicy::At(_))
                })
                || matches!(
                    &*model.catalog,
                    CatalogProjection::Failed(UiFailure {
                        retry: RetryPolicy::After { .. } | RetryPolicy::At(_),
                        ..
                    })
                )
            {
                model.dirty = true;
            }
            Vec::new()
        }
        Message::Resize => {
            model.dirty = true;
            Vec::new()
        }
        Message::ShutdownRequested => {
            model.should_quit = true;
            vec![UiEffect::Quit]
        }
    }
}

fn handle_input(model: &mut Model, input: Input) -> Vec<UiEffect> {
    if matches!(
        input,
        Input {
            key: Key::Char('n' | 'N'),
            ctrl: true,
            ..
        }
    ) {
        return create_session(model);
    }

    if model.focus == Focus::Permission {
        return handle_permission_input(model, input);
    }

    if matches!(
        input,
        Input {
            key: Key::Char('k' | 'K'),
            ctrl: true,
            ..
        }
    ) {
        if !model.credential.open {
            open_credential(model);
        }
        return Vec::new();
    }

    if model.credential.open {
        return handle_credential_input(model, input);
    }

    if model.picker.open {
        return handle_picker_input(model, input);
    }

    match input {
        Input {
            key: Key::Char('p' | 'P'),
            ctrl: true,
            ..
        } => {
            open_picker(model);
            Vec::new()
        }
        Input {
            key: Key::Char('s' | 'S'),
            ctrl: true,
            ..
        }
        | Input {
            key: Key::Enter,
            ctrl: true,
            ..
        } => submit_prompt(model),
        Input {
            key: Key::Char('r' | 'R'),
            ctrl: true,
            ..
        } => retry_attempt(model),
        Input { key: Key::Esc, .. } => cancel_attempt(model),
        Input {
            key: Key::Char('c' | 'C'),
            ctrl: true,
            ..
        } => {
            if model.session.active_attempt().is_some() {
                cancel_attempt(model)
            } else {
                model.should_quit = true;
                vec![UiEffect::Quit]
            }
        }
        Input {
            key: Key::Up,
            alt: true,
            ..
        }
        | Input {
            key: Key::PageUp,
            ctrl: true,
            ..
        } => {
            scroll_up(model, 6);
            Vec::new()
        }
        Input {
            key: Key::Down,
            alt: true,
            ..
        }
        | Input {
            key: Key::PageDown,
            ctrl: true,
            ..
        } => {
            scroll_down(model, 6);
            Vec::new()
        }
        Input {
            key: Key::End,
            ctrl: true,
            ..
        } => {
            follow_tail(model);
            Vec::new()
        }
        input => {
            if !has_pending_submission(model) && input_composer(&mut model.composer.editor, input) {
                model.notice = None;
                model.dirty = true;
            }
            Vec::new()
        }
    }
}

fn handle_permission_input(model: &mut Model, input: Input) -> Vec<UiEffect> {
    match input {
        Input { key: Key::Up, .. }
        | Input {
            key: Key::PageUp, ..
        } => {
            model.permission_scroll = model.permission_scroll.saturating_sub(1);
            model.dirty = true;
            Vec::new()
        }
        Input { key: Key::Down, .. }
        | Input {
            key: Key::PageDown, ..
        } => {
            model.permission_scroll = model.permission_scroll.saturating_add(1);
            model.dirty = true;
            Vec::new()
        }
        Input {
            key: Key::Char('y' | 'Y'),
            ..
        } => answer_permission(model, true),
        Input {
            key: Key::Char('n' | 'N') | Key::Esc,
            ..
        } => answer_permission(model, false),
        Input {
            key: Key::Char('c' | 'C'),
            ctrl: true,
            ..
        } => {
            model.should_quit = true;
            vec![UiEffect::Quit]
        }
        _ => Vec::new(),
    }
}

fn handle_credential_input(model: &mut Model, input: Input) -> Vec<UiEffect> {
    match input {
        Input { key: Key::Esc, .. } => {
            close_credential(model);
            if matches!(&*model.catalog, CatalogProjection::CredentialRequired) {
                model.notice = Some(Notice::Info(
                    "An API key is still required; press Ctrl+K when ready".to_owned(),
                ));
            }
            Vec::new()
        }
        Input {
            key: Key::Char('c' | 'C'),
            ctrl: true,
            ..
        } => {
            model.should_quit = true;
            vec![UiEffect::Quit]
        }
        Input {
            key: Key::Enter, ..
        }
        | Input {
            key: Key::Char('s' | 'S'),
            ctrl: true,
            ..
        } => submit_credential(model),
        Input {
            key: Key::Backspace,
            ..
        } => {
            model.credential.pop();
            model.notice = None;
            model.dirty = true;
            Vec::new()
        }
        Input {
            key: Key::Char(character),
            ctrl: false,
            alt: false,
            ..
        } => {
            match model.credential.append_character(character) {
                Ok(()) => model.notice = None,
                Err(message) => {
                    model.notice = Some(Notice::Failure(UiFailure::new(
                        ErrorClass::Validation,
                        message,
                        RetryPolicy::Never,
                    )));
                }
            }
            model.dirty = true;
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn handle_picker_input(model: &mut Model, input: Input) -> Vec<UiEffect> {
    match input {
        Input { key: Key::Esc, .. } => {
            close_picker(model);
            Vec::new()
        }
        Input {
            key: Key::Enter, ..
        } => select_picker_model(model),
        Input { key: Key::Up, .. } => {
            move_picker_selection(model, -1);
            Vec::new()
        }
        Input { key: Key::Down, .. } => {
            move_picker_selection(model, 1);
            Vec::new()
        }
        Input {
            key: Key::Backspace,
            ..
        } => {
            model.picker.query.pop();
            normalize_picker_selection(model);
            model.dirty = true;
            Vec::new()
        }
        Input {
            key: Key::Char(character),
            ctrl: false,
            alt: false,
            ..
        } if !character.is_control() => {
            model.picker.query.push(character);
            normalize_picker_selection(model);
            model.dirty = true;
            Vec::new()
        }
        Input {
            key: Key::Char('r' | 'R'),
            ctrl: true,
            ..
        } => refresh_catalog(model),
        _ => Vec::new(),
    }
}

fn handle_paste(model: &mut Model, text: &str) {
    if model.credential.open {
        match model.credential.append_paste(text) {
            Ok(()) => model.notice = None,
            Err(message) => {
                model.notice = Some(Notice::Failure(UiFailure::new(
                    ErrorClass::Validation,
                    message,
                    RetryPolicy::Never,
                )));
            }
        }
        model.dirty = true;
    } else if model.picker.open {
        let flattened = editable_safe(text).replace('\n', " ");
        model.picker.query.push_str(&flattened);
        normalize_picker_selection(model);
        model.dirty = true;
    } else if !has_pending_submission(model)
        && model.composer.editor.insert_str(editable_safe(text))
    {
        model.notice = None;
        model.dirty = true;
    }
}

fn input_composer(editor: &mut ratatui_textarea::TextArea<'static>, input: Input) -> bool {
    if let Input {
        key: Key::Char(character),
        ctrl: false,
        alt: false,
        ..
    } = input
    {
        let original = character.to_string();
        let safe = display_safe(&original);
        if safe != original {
            return editor.insert_str(safe);
        }
    }
    editor.input(input)
}

fn apply_session(model: &mut Model, session: Arc<SessionProjection>) {
    let session_changed = session.session_id != model.session.session_id;
    if !session_changed && session.revision < model.session.revision {
        return;
    }
    if session_changed {
        model.composer.reset();
        model.transcript = crate::model::TranscriptState::new();
        model.cancelling.clear();
        model.retrying.clear();
        model.answering_permissions.clear();
        model.permission_scroll = 0;
        model.credential.open = false;
        model.credential.clear();
        model.picker.open = false;
        model.picker.selected = session.selected_model.clone();
        model.focus = Focus::Composer;
    }
    if model.transcript.follow_tail {
        model.transcript.rows_from_bottom = 0;
    }
    if let Some(selected) = &session.selected_model {
        model.picker.selected = Some(selected.clone());
    }
    model.cancelling.retain(|attempt_id| {
        session.transcript.iter().any(|item| {
            matches!(
                item,
                crate::model::TranscriptItem::Assistant {
                    attempt_id: candidate,
                    status: crate::model::AttemptStatus::Streaming,
                    ..
                } if candidate == attempt_id
            )
        })
    });
    model.retrying.retain(|attempt_id| {
        let original_still_failed = session.transcript.iter().any(|item| {
            matches!(
                item,
                crate::model::TranscriptItem::Assistant {
                    attempt_id: candidate,
                    status: crate::model::AttemptStatus::Failed(_),
                    ..
                } if candidate == attempt_id
            )
        });
        let retry_projected = session.transcript.iter().any(|item| {
            matches!(
                item,
                crate::model::TranscriptItem::Assistant {
                    retry_of: Some(candidate),
                    ..
                } if candidate == attempt_id
            )
        });
        original_still_failed && !retry_projected
    });
    model.answering_permissions.retain(|tool_call_id| {
        session
            .permission_requests
            .iter()
            .any(|request| &request.tool_call_id == tool_call_id)
    });
    model.session = session;
    if !model.session.permission_requests.is_empty() {
        model.permission_scroll = 0;
        model.credential.open = false;
        model.credential.clear();
        model.picker.open = false;
        model.focus = Focus::Permission;
    } else if model.focus == Focus::Permission {
        model.permission_scroll = 0;
        model.focus = Focus::Composer;
    } else if session_changed
        && model.session.selected_model.is_none()
        && matches!(&*model.catalog, CatalogProjection::Ready { models, .. } if !models.is_empty())
    {
        open_picker(model);
    }
    model.sync_retry_deadline();
    model.dirty = true;
}

fn apply_catalog(model: &mut Model, catalog: Arc<CatalogProjection>) {
    model.catalog = catalog;
    model.sync_catalog_retry_deadline();
    normalize_picker_selection(model);
    if matches!(&*model.catalog, CatalogProjection::CredentialRequired) {
        open_credential(model);
    } else if !selected_model_available(model)
        && matches!(&*model.catalog, CatalogProjection::Ready { models, .. } if !models.is_empty())
    {
        open_picker(model);
    }
    model.dirty = true;
}

fn apply_notice(model: &mut Model, notice: UiNotice) {
    match notice {
        UiNotice::IntentCommitted { request_id } => {
            if let Some(pending) = model.pending.remove(&request_id) {
                match pending {
                    PendingKind::CreateSession => {
                        model.composer.reset();
                        model.credential.open = false;
                        model.credential.clear();
                        model.notice = Some(Notice::Info("New session created".to_owned()));
                    }
                    PendingKind::ConfigureCredential => {
                        model.notice = Some(Notice::Info("API key accepted".to_owned()));
                    }
                    PendingKind::SubmitPrompt(_) => {
                        model.composer.reset();
                        model.notice = None;
                    }
                    PendingKind::SelectModel(_) => {
                        close_picker(model);
                        model.notice = None;
                    }
                    PendingKind::CancelAttempt(attempt_id) => {
                        model.cancelling.insert(attempt_id);
                        model.notice = Some(Notice::Info("Cancellation requested".to_owned()));
                    }
                    PendingKind::RetryAttempt(attempt_id) => {
                        model.retrying.insert(attempt_id);
                        model.notice = Some(Notice::Info("Retry requested".to_owned()));
                    }
                    PendingKind::RefreshCatalog => {
                        model.notice = Some(Notice::Info("Catalog refresh requested".to_owned()));
                    }
                    PendingKind::AnswerPermission(tool_call_id) => {
                        model.answering_permissions.remove(&tool_call_id);
                        model.notice = Some(Notice::Info("Permission answer saved".to_owned()));
                    }
                }
            }
        }
        UiNotice::IntentRejected {
            request_id,
            failure,
        } => {
            let pending = model.pending.remove(&request_id);
            match pending {
                Some(PendingKind::CreateSession) => {}
                Some(PendingKind::ConfigureCredential) => {
                    open_credential(model);
                }
                Some(PendingKind::SelectModel(_)) => open_picker(model),
                Some(
                    PendingKind::RefreshCatalog
                    | PendingKind::SubmitPrompt(_)
                    | PendingKind::CancelAttempt(_)
                    | PendingKind::RetryAttempt(_)
                    | PendingKind::AnswerPermission(_),
                )
                | None => {}
            }
            model.notice = Some(Notice::Failure(failure));
        }
    }
    model.dirty = true;
}

fn create_session(model: &mut Model) -> Vec<UiEffect> {
    if model
        .pending
        .values()
        .any(|pending| matches!(pending, PendingKind::CreateSession))
    {
        return Vec::new();
    }
    let request_id = model.allocate_request();
    model.pending.insert(request_id, PendingKind::CreateSession);
    model.notice = Some(Notice::Info("Creating a new session...".to_owned()));
    model.dirty = true;
    vec![UiEffect::Dispatch(UiIntent::CreateSession { request_id })]
}

fn answer_permission(model: &mut Model, allow: bool) -> Vec<UiEffect> {
    let Some(permission) = model.session.permission_requests.first() else {
        model.focus = Focus::Composer;
        model.dirty = true;
        return Vec::new();
    };
    let tool_call_id = permission.tool_call_id.clone();
    if model.answering_permissions.contains(&tool_call_id) {
        return Vec::new();
    }
    let request_id = model.allocate_request();
    model.pending.insert(
        request_id,
        PendingKind::AnswerPermission(tool_call_id.clone()),
    );
    model.answering_permissions.insert(tool_call_id.clone());
    model.notice = Some(Notice::Info("Saving permission answer...".to_owned()));
    model.dirty = true;
    vec![UiEffect::Dispatch(UiIntent::AnswerPermission {
        request_id,
        tool_call_id,
        allow,
    })]
}

fn submit_credential(model: &mut Model) -> Vec<UiEffect> {
    if has_pending_credential(model) {
        return Vec::new();
    }
    if !model.credential.has_value() {
        model.notice = Some(Notice::Failure(UiFailure::new(
            ErrorClass::Validation,
            "Paste a non-empty API key",
            RetryPolicy::Never,
        )));
        model.dirty = true;
        return Vec::new();
    }

    let credential = model.credential.take();
    let request_id = model.allocate_request();
    model
        .pending
        .insert(request_id, PendingKind::ConfigureCredential);
    close_credential(model);
    model.notice = Some(Notice::Info(
        "Checking API key and loading models...".to_owned(),
    ));
    model.dirty = true;
    vec![UiEffect::Dispatch(UiIntent::ConfigureCredential {
        request_id,
        credential,
    })]
}

fn submit_prompt(model: &mut Model) -> Vec<UiEffect> {
    if has_pending_submission(model) {
        return Vec::new();
    }
    if model.composer.is_blank() {
        model.notice = Some(Notice::Failure(UiFailure::new(
            ErrorClass::Validation,
            "Prompt must contain non-whitespace text",
            RetryPolicy::Never,
        )));
        model.dirty = true;
        return Vec::new();
    }
    if !selected_model_available(model) {
        model.notice = Some(Notice::Info(
            "Choose a model from the current catalog before sending".to_owned(),
        ));
        open_picker(model);
        return Vec::new();
    }
    if model.session.active_attempt().is_some() {
        model.notice = Some(Notice::Info(
            "Cancel or wait for the active response before sending".to_owned(),
        ));
        model.dirty = true;
        return Vec::new();
    }

    let prompt = model.composer.text();
    let request_id = model.allocate_request();
    model
        .pending
        .insert(request_id, PendingKind::SubmitPrompt(prompt.clone()));
    model.notice = Some(Notice::Info("Saving prompt...".to_owned()));
    model.dirty = true;
    vec![UiEffect::Dispatch(UiIntent::SubmitPrompt {
        request_id,
        prompt,
    })]
}

fn select_picker_model(model: &mut Model) -> Vec<UiEffect> {
    let Some(selection) = model.picker.selected.clone() else {
        return Vec::new();
    };
    if !model
        .catalog
        .models()
        .iter()
        .any(|summary| summary.selectable && summary.model == selection)
    {
        return Vec::new();
    }
    if model
        .pending
        .values()
        .any(|pending| matches!(pending, PendingKind::SelectModel(_)))
    {
        return Vec::new();
    }

    let request_id = model.allocate_request();
    model
        .pending
        .insert(request_id, PendingKind::SelectModel(selection.clone()));
    model.notice = Some(Notice::Info("Selecting model...".to_owned()));
    model.dirty = true;
    vec![UiEffect::Dispatch(UiIntent::SelectModel {
        request_id,
        model: selection,
    })]
}

fn cancel_attempt(model: &mut Model) -> Vec<UiEffect> {
    let Some(attempt_id) = model.session.streaming_attempt().cloned() else {
        model.notice = None;
        model.dirty = true;
        return Vec::new();
    };
    if has_pending_attempt(model, &attempt_id, true) {
        return Vec::new();
    }
    let request_id = model.allocate_request();
    model
        .pending
        .insert(request_id, PendingKind::CancelAttempt(attempt_id.clone()));
    model.notice = Some(Notice::Info("Requesting cancellation...".to_owned()));
    model.dirty = true;
    vec![UiEffect::Dispatch(UiIntent::CancelAttempt {
        request_id,
        attempt_id,
    })]
}

fn retry_attempt(model: &mut Model) -> Vec<UiEffect> {
    let Some((attempt_id, retry)) = model.session.retryable_attempt() else {
        return Vec::new();
    };
    if retry == RetryPolicy::Never {
        model.notice = Some(Notice::Info(
            "This failure cannot be retried safely".to_owned(),
        ));
        model.dirty = true;
        return Vec::new();
    }
    if !model.retry_available(attempt_id, retry) {
        model.notice = Some(Notice::Info("Retry is not available yet".to_owned()));
        model.dirty = true;
        return Vec::new();
    }
    let attempt_id = attempt_id.clone();
    if has_pending_attempt(model, &attempt_id, false) {
        return Vec::new();
    }
    let request_id = model.allocate_request();
    model
        .pending
        .insert(request_id, PendingKind::RetryAttempt(attempt_id.clone()));
    model.notice = Some(Notice::Info("Requesting retry...".to_owned()));
    model.dirty = true;
    vec![UiEffect::Dispatch(UiIntent::RetryAttempt {
        request_id,
        attempt_id,
    })]
}

fn refresh_catalog(model: &mut Model) -> Vec<UiEffect> {
    if matches!(&*model.catalog, CatalogProjection::CredentialRequired) {
        open_credential(model);
        return Vec::new();
    }
    if model
        .pending
        .values()
        .any(|pending| matches!(pending, PendingKind::RefreshCatalog))
    {
        return Vec::new();
    }
    if let CatalogProjection::Failed(failure) = &*model.catalog
        && !model.catalog_retry_available(failure.retry)
    {
        model.notice = Some(Notice::Info(
            model.catalog_retry_remaining_ms(failure.retry).map_or_else(
                || "Catalog refresh is unavailable for this failure".to_owned(),
                |remaining| format!("Catalog refresh available in {remaining} ms"),
            ),
        ));
        model.dirty = true;
        return Vec::new();
    }
    let request_id = model.allocate_request();
    model
        .pending
        .insert(request_id, PendingKind::RefreshCatalog);
    model.notice = Some(Notice::Info("Refreshing models...".to_owned()));
    model.dirty = true;
    vec![UiEffect::Dispatch(UiIntent::RefreshCatalog { request_id })]
}

fn open_picker(model: &mut Model) {
    model.credential.open = false;
    model.credential.clear();
    model.picker.open = true;
    model.focus = Focus::Picker;
    normalize_picker_selection(model);
    model.dirty = true;
}

fn open_credential(model: &mut Model) {
    if has_pending_credential(model) {
        model.notice = Some(Notice::Info(
            "The current API key is still being checked".to_owned(),
        ));
        model.dirty = true;
        return;
    }
    if model.session.active_attempt().is_some() {
        model.notice = Some(Notice::Info(
            "Wait for or cancel the active response before changing the API key".to_owned(),
        ));
        model.dirty = true;
        return;
    }
    model.picker.open = false;
    model.credential.clear();
    model.credential.open = true;
    model.focus = Focus::Credential;
    model.dirty = true;
}

fn close_credential(model: &mut Model) {
    model.credential.clear();
    model.credential.open = false;
    model.focus = Focus::Composer;
    model.dirty = true;
}

fn close_picker(model: &mut Model) {
    model.picker.open = false;
    model.focus = Focus::Composer;
    model.dirty = true;
}

fn normalize_picker_selection(model: &mut Model) {
    let selectable = filtered_selectable_models(model);
    if model
        .picker
        .selected
        .as_ref()
        .is_none_or(|selected| !selectable.iter().any(|candidate| candidate == selected))
    {
        model.picker.selected = selectable.first().cloned();
    }
}

fn move_picker_selection(model: &mut Model, direction: isize) {
    let selectable = filtered_selectable_models(model);
    if selectable.is_empty() {
        model.picker.selected = None;
        return;
    }
    let current = model
        .picker
        .selected
        .as_ref()
        .and_then(|selected| {
            selectable
                .iter()
                .position(|candidate| candidate == selected)
        })
        .unwrap_or(0);
    let next = if direction < 0 {
        current.checked_sub(1).unwrap_or(selectable.len() - 1)
    } else {
        (current + 1) % selectable.len()
    };
    model.picker.selected = Some(selectable[next].clone());
    model.dirty = true;
}

fn filtered_selectable_models(model: &Model) -> Vec<autoharness_domain::ModelRef> {
    let query = model.picker.query.to_lowercase();
    model
        .catalog
        .models()
        .iter()
        .filter(|summary| {
            summary.selectable
                && (query.is_empty()
                    || summary.display_name.to_lowercase().contains(&query)
                    || summary
                        .model
                        .model_id()
                        .as_str()
                        .to_lowercase()
                        .contains(&query)
                    || summary
                        .model
                        .provider_id()
                        .as_str()
                        .to_lowercase()
                        .contains(&query))
        })
        .map(|summary| summary.model.clone())
        .collect()
}

fn selected_model_available(model: &Model) -> bool {
    model
        .session
        .selected_model
        .as_ref()
        .is_some_and(|selected| {
            model
                .catalog
                .models()
                .iter()
                .any(|summary| summary.selectable && &summary.model == selected)
        })
}

fn has_pending_submission(model: &Model) -> bool {
    model
        .pending
        .values()
        .any(|pending| matches!(pending, PendingKind::SubmitPrompt(_)))
}

fn has_pending_credential(model: &Model) -> bool {
    model
        .pending
        .values()
        .any(|pending| matches!(pending, PendingKind::ConfigureCredential))
}

fn has_pending_attempt(model: &Model, attempt_id: &AttemptKey, cancellation: bool) -> bool {
    (cancellation && model.cancellation_requested(attempt_id))
        || (!cancellation && model.retry_requested(attempt_id))
        || model.pending.values().any(|pending| match pending {
            PendingKind::CancelAttempt(candidate) if cancellation => candidate == attempt_id,
            PendingKind::RetryAttempt(candidate) if !cancellation => candidate == attempt_id,
            PendingKind::CreateSession
            | PendingKind::ConfigureCredential
            | PendingKind::RefreshCatalog
            | PendingKind::SelectModel(_)
            | PendingKind::SubmitPrompt(_)
            | PendingKind::CancelAttempt(_)
            | PendingKind::RetryAttempt(_)
            | PendingKind::AnswerPermission(_) => false,
        })
}

fn scroll_up(model: &mut Model, rows: usize) {
    model.transcript.follow_tail = false;
    model.transcript.rows_from_bottom = model.transcript.rows_from_bottom.saturating_add(rows);
    model.dirty = true;
}

fn scroll_down(model: &mut Model, rows: usize) {
    model.transcript.rows_from_bottom = model.transcript.rows_from_bottom.saturating_sub(rows);
    if model.transcript.rows_from_bottom == 0 {
        model.transcript.follow_tail = true;
    }
    model.dirty = true;
}

fn follow_tail(model: &mut Model) {
    model.transcript.follow_tail = true;
    model.transcript.rows_from_bottom = 0;
    model.dirty = true;
}
