use std::sync::Arc;

use autoharness_domain::ErrorClass;
use ratatui_textarea::{Input, Key};

use crate::model::{
    AttemptKey, COMMANDS, CatalogProjection, CommandEntry, Focus, Message, Model, Notice,
    PendingKind, RetryPolicy, SessionProjection, SessionsProjection, UiEffect, UiFailure, UiIntent,
    UiNotice,
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
        Message::SessionsChanged(sessions) => {
            apply_sessions(model, sessions);
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

    // Ctrl+, toggles the non-modal settings overlay from any focus except
    // the permission decision, which owns the keyboard exclusively.
    if matches!(
        input,
        Input {
            key: Key::Char(','),
            ctrl: true,
            ..
        }
    ) && model.focus != Focus::Permission
    {
        model.settings_open = !model.settings_open;
        model.dirty = true;
        return Vec::new();
    }

    // Ctrl+/ opens the modal command palette from any focus except the
    // permission decision, which owns the keyboard exclusively. F1 opens
    // contextual help under the same rule.
    if matches!(
        input,
        Input {
            key: Key::Char('/' | '?'),
            ctrl: true,
            ..
        }
    ) && model.focus != Focus::Permission
    {
        open_palette(model);
        return Vec::new();
    }
    if matches!(input, Input { key: Key::F(1), .. }) && model.focus != Focus::Permission {
        open_help(model);
        return Vec::new();
    }
    // Ctrl+F opens the transcript search bar under the same rule.
    if matches!(
        input,
        Input {
            key: Key::Char('f' | 'F'),
            ctrl: true,
            ..
        }
    ) && model.focus != Focus::Permission
    {
        open_search(model);
        return Vec::new();
    }

    if model.focus == Focus::Palette {
        return handle_palette_input(model, input);
    }

    if model.focus == Focus::Help {
        return handle_help_input(model, input);
    }

    if model.focus == Focus::Search {
        return handle_search_input(model, input);
    }

    if model.focus == Focus::Permission {
        return handle_permission_input(model, input);
    }

    if matches!(
        input,
        Input {
            key: Key::Char('l' | 'L'),
            ctrl: true,
            ..
        }
    ) {
        if !model.browser.open {
            open_browser(model);
        }
        return Vec::new();
    }

    if model.browser.open {
        return handle_browser_input(model, input);
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
            ctrl: true,
            ..
        } => recall_history(model, -1),
        Input {
            key: Key::Down,
            ctrl: true,
            ..
        } => recall_history(model, 1),
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
        Input {
            key: Key::Char('y' | 'Y'),
            ctrl: true,
            ..
        } => vec![UiEffect::CopyTranscript(
            crate::view::transcript_plain_text(model),
        )],
        input => {
            // Slash commands give keyboard-first access to every command
            // through the shared table without opening the palette overlay.
            if !has_pending_submission(model)
                && let Some(effects) = maybe_slash_command(model, &input)
            {
                return effects;
            }
            if !has_pending_submission(model) && input_composer(&mut model.composer.editor, input) {
                model.history.reset_walk();
                model.notice = None;
                model.dirty = true;
            }
            Vec::new()
        }
    }
}

/// Recognizes slash commands typed into an otherwise empty composer.
///
/// The composer content is interpreted as a single command token:
///
/// - `/name` runs the shared command table entry and clears the composer.
/// - `//text` escapes into the literal prompt `/text` on Enter.
/// - Anything else, including multiline text, falls through to ordinary input.
fn maybe_slash_command(model: &mut Model, input: &Input) -> Option<Vec<UiEffect>> {
    if !matches!(input.key, Key::Enter if !input.ctrl) {
        return None;
    }
    let first = model.composer.lines().first().cloned()?;
    let command = first
        .strip_prefix('/')
        .map(str::to_owned)
        .filter(|command| !command.is_empty())?;

    // A doubled leading slash escapes command interpretation entirely; the
    // prompt is submitted with exactly one slash stripped.
    if let Some(literal) = command.strip_prefix('/').map(str::to_owned) {
        let text = format!("/{literal}");
        model.composer.reset();
        return Some(submit_prompt_text(model, text));
    }

    match run_command_by_id(model, &command) {
        Err(rejection) => {
            // A rejected command keeps its text editable so a typo costs one
            // backspace, not a retyped line.
            model.notice = Some(Notice::Failure(UiFailure::new(
                autoharness_domain::ErrorClass::Validation,
                rejection,
                RetryPolicy::Never,
            )));
            model.dirty = true;
            Some(Vec::new())
        }
        Ok(effects) => {
            model.composer.reset();
            Some(effects)
        }
    }
}

/// Runs one shared command by its stable table identity.
fn run_command_by_id(model: &mut Model, id: &str) -> Result<Vec<UiEffect>, String> {
    let entry = COMMANDS
        .iter()
        .find(|entry| entry.id == id)
        .ok_or_else(|| format!("Unknown command '/{id}'. Press Ctrl+/ to list commands."))?;
    Ok(execute_command(model, *entry))
}

/// Executes one shared command through the same local actions and intents
/// that the corresponding keyboard chord uses, returning any intents that
/// must cross into application composition.
pub(crate) fn execute_command(model: &mut Model, entry: CommandEntry) -> Vec<UiEffect> {
    match entry.id {
        "sessions" => {
            open_browser(model);
            Vec::new()
        }
        "models" => {
            open_picker(model);
            Vec::new()
        }
        "connect-api-key" => {
            open_credential(model);
            Vec::new()
        }
        "settings" => {
            model.settings_open = !model.settings_open;
            model.dirty = true;
            Vec::new()
        }
        "refresh-models" => refresh_catalog(model),
        "new-session" => create_session(model),
        "help" => {
            open_help(model);
            Vec::new()
        }
        "commands" => {
            open_palette(model);
            Vec::new()
        }
        "copy" => vec![UiEffect::CopyTranscript(
            crate::view::transcript_plain_text(model),
        )],
        "export" => export_transcript(model),
        unknown => unreachable!("command table contains an unhandled entry: {unknown}"),
    }
}

/// Dispatches the durable Markdown-export intent for the active session.
fn export_transcript(model: &mut Model) -> Vec<UiEffect> {
    if model
        .pending
        .values()
        .any(|pending| matches!(pending, PendingKind::ExportTranscript))
    {
        return Vec::new();
    }
    let session_id = model.session.session_id.clone();
    let request_id = model.allocate_request();
    model
        .pending
        .insert(request_id, PendingKind::ExportTranscript);
    model.notice = Some(Notice::Info("Exporting transcript...".to_owned()));
    model.dirty = true;
    vec![UiEffect::Dispatch(UiIntent::ExportTranscript {
        request_id,
        session_id,
    })]
}

/// Submits exact prompt text through the same pending-request path as a
/// composer submission.
fn submit_prompt_text(model: &mut Model, prompt: String) -> Vec<UiEffect> {
    if has_pending_submission(model) || prompt.trim().is_empty() {
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

/// Opens the modal command-palette overlay, remembering where the keyboard
/// came from so Esc restores it exactly.
fn open_palette(model: &mut Model) {
    if !model.palette.open {
        model.palette.return_focus = model.focus;
    }
    model.palette.open = true;
    model.palette.query.clear();
    model.palette.selected = None;
    normalize_palette_selection(model);
    model.focus = Focus::Palette;
    model.dirty = true;
}

fn close_palette(model: &mut Model) {
    model.palette.open = false;
    model.palette.query.clear();
    model.palette.selected = None;
    model.focus = match model.palette.return_focus {
        Focus::Palette | Focus::Permission => Focus::Composer,
        restored => restored,
    };
    model.dirty = true;
}

fn filtered_palette_commands(model: &Model) -> Vec<CommandEntry> {
    let query = model.palette.query.to_lowercase();
    COMMANDS
        .iter()
        .filter(|entry| {
            query.is_empty()
                || entry.id.contains(&query)
                || entry.label.to_lowercase().contains(&query)
                || entry.description.to_lowercase().contains(&query)
        })
        .copied()
        .collect()
}

fn normalize_palette_selection(model: &mut Model) {
    let entries = filtered_palette_commands(model);
    let valid = model
        .palette
        .selected
        .is_some_and(|selected| entries.iter().any(|entry| entry.id == selected));
    if !valid {
        model.palette.selected = entries.first().map(|entry| entry.id);
    } else if entries.is_empty() {
        model.palette.selected = None;
    }
}

fn move_palette_selection(model: &mut Model, direction: isize) {
    let entries = filtered_palette_commands(model);
    if entries.is_empty() {
        model.palette.selected = None;
        return;
    }
    let current = model
        .palette
        .selected
        .and_then(|selected| entries.iter().position(|entry| entry.id == selected))
        .unwrap_or(0);
    let next = if direction < 0 {
        current.checked_sub(1).unwrap_or(entries.len() - 1)
    } else {
        (current + 1) % entries.len()
    };
    model.palette.selected = Some(entries[next].id);
    model.dirty = true;
}

fn execute_palette_selection(model: &mut Model) -> Vec<UiEffect> {
    let Some(selected) = model.palette.selected else {
        return Vec::new();
    };
    let Some(entry) = COMMANDS.iter().find(|entry| entry.id == selected).copied() else {
        return Vec::new();
    };
    model.palette.open = false;
    model.palette.query.clear();
    model.palette.selected = None;
    let effects = execute_command(model, entry);
    // A command that opened another modal overlay keeps its focus there;
    // otherwise the keyboard returns to the remembered focus.
    if !model.palette.open && model.focus == Focus::Palette {
        model.focus = match model.palette.return_focus {
            Focus::Palette | Focus::Permission => Focus::Composer,
            restored => restored,
        };
        model.dirty = true;
    }
    effects
}

/// Applies keyboard input while the command palette owns focus.
///
/// Quit and fresh-session chords stay global so power users never lose
/// them; plain characters extend the filter query.
fn handle_palette_input(model: &mut Model, input: Input) -> Vec<UiEffect> {
    if matches!(
        input,
        Input {
            key: Key::Char('c' | 'C'),
            ctrl: true,
            ..
        }
    ) {
        model.should_quit = true;
        return vec![UiEffect::Quit];
    }
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
    match input {
        Input { key: Key::Esc, .. } => {
            close_palette(model);
            Vec::new()
        }
        Input {
            key: Key::Enter, ..
        } => execute_palette_selection(model),
        Input { key: Key::Up, .. } => {
            move_palette_selection(model, -1);
            Vec::new()
        }
        Input { key: Key::Down, .. } => {
            move_palette_selection(model, 1);
            Vec::new()
        }
        Input {
            key: Key::Backspace,
            ..
        } => {
            model.palette.query.pop();
            normalize_palette_selection(model);
            model.dirty = true;
            Vec::new()
        }
        Input {
            key: Key::Char(character),
            ctrl: false,
            alt: false,
            ..
        } if !character.is_control() => {
            model.palette.query.push(character);
            normalize_palette_selection(model);
            model.dirty = true;
            Vec::new()
        }
        _ => Vec::new(),
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

/// Applies keyboard input while the session-browser overlay owns focus.
///
/// Letter shortcuts use Ctrl chords so plain characters always extend the
/// filter query, matching the model picker's convention.
fn handle_browser_input(model: &mut Model, input: Input) -> Vec<UiEffect> {
    // Ctrl+C always quits, even from inside the browser overlay.
    if matches!(
        input,
        Input {
            key: Key::Char('c' | 'C'),
            ctrl: true,
            ..
        }
    ) {
        model.should_quit = true;
        return vec![UiEffect::Quit];
    }
    if model.browser.renaming {
        return handle_browser_rename_input(model, input);
    }

    // While a deletion is armed, Y confirms and N or Esc cancels.
    if model.browser.confirming_delete.is_some() {
        match input {
            Input {
                key: Key::Char('y' | 'Y'),
                ctrl: false,
                ..
            } => return confirm_delete_selected_session(model),
            Input {
                key: Key::Char('n' | 'N'),
                ctrl: false,
                ..
            }
            | Input { key: Key::Esc, .. } => {
                model.browser.confirming_delete = None;
                model.dirty = true;
                return Vec::new();
            }
            _ => return Vec::new(),
        }
    }

    match input {
        Input { key: Key::Esc, .. } => {
            close_browser(model);
            Vec::new()
        }
        Input {
            key: Key::Enter, ..
        } => open_selected_session(model),
        Input { key: Key::Up, .. } => {
            move_browser_selection(model, -1);
            Vec::new()
        }
        Input { key: Key::Down, .. } => {
            move_browser_selection(model, 1);
            Vec::new()
        }
        Input {
            key: Key::Char('r' | 'R'),
            ctrl: true,
            ..
        } => rename_selected_session(model),
        Input {
            key: Key::Char('a' | 'A'),
            ctrl: true,
            ..
        } => toggle_archive_selected_session(model),
        Input {
            key: Key::Char('d' | 'D'),
            ctrl: true,
            ..
        } => request_delete_selected_session(model),
        Input {
            key: Key::Backspace,
            ..
        } => {
            model.browser.query.pop();
            model.sync_browser_selection();
            model.dirty = true;
            Vec::new()
        }
        Input {
            key: Key::Char(character),
            ctrl: false,
            alt: false,
            ..
        } if !character.is_control() => {
            model.browser.query.push(character);
            model.sync_browser_selection();
            model.dirty = true;
            Vec::new()
        }
        _ => Vec::new(),
    }
}

/// Applies keyboard input while a rename buffer is active in the browser.
fn handle_browser_rename_input(model: &mut Model, input: Input) -> Vec<UiEffect> {
    match input {
        Input { key: Key::Esc, .. } => {
            model.browser.renaming = false;
            model.browser.rename_buffer.clear();
            model.dirty = true;
            Vec::new()
        }
        Input {
            key: Key::Enter, ..
        } => submit_browser_rename(model),
        Input {
            key: Key::Backspace,
            ..
        } => {
            model.browser.rename_buffer.pop();
            model.dirty = true;
            Vec::new()
        }
        Input {
            key: Key::Char(character),
            ctrl: false,
            alt: false,
            ..
        } if !character.is_control() && model.browser.rename_buffer.len() < 128 => {
            model.browser.rename_buffer.push(character);
            model.dirty = true;
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn apply_sessions(model: &mut Model, sessions: Arc<SessionsProjection>) {
    model.sessions = sessions;
    model.sync_browser_selection();
    model.dirty = true;
}

fn open_browser(model: &mut Model) {
    model.picker.open = false;
    model.credential.open = false;
    model.credential.clear();
    model.browser.open = true;
    model.browser.renaming = false;
    model.browser.rename_buffer.clear();
    model.browser.confirming_delete = None;
    if let Some(active) = model
        .sessions
        .sessions
        .iter()
        .find(|entry| entry.active)
        .map(|entry| entry.session_id.clone())
    {
        model.browser.selected = Some(active);
    }
    model.focus = Focus::Browser;
    model.sync_browser_selection();
    model.dirty = true;
}

fn close_browser(model: &mut Model) {
    model.browser.open = false;
    model.browser.renaming = false;
    model.browser.rename_buffer.clear();
    model.browser.confirming_delete = None;
    model.focus = Focus::Composer;
    model.dirty = true;
}

fn selected_browser_entry(model: &Model) -> Option<&crate::model::SessionBrowserEntry> {
    let selected = model.browser.selected.as_ref()?;
    model
        .browser_entries()
        .into_iter()
        .find(|entry| &entry.session_id == selected)
}

fn move_browser_selection(model: &mut Model, direction: isize) {
    let entries = model.browser_entries();
    if entries.is_empty() {
        return;
    }
    let current = model
        .browser
        .selected
        .as_ref()
        .and_then(|selected| {
            entries
                .iter()
                .position(|entry| &entry.session_id == selected)
        })
        .unwrap_or(0);
    let next = if direction < 0 {
        current.checked_sub(1).unwrap_or(entries.len() - 1)
    } else {
        (current + 1) % entries.len()
    };
    model.browser.selected = Some(entries[next].session_id.clone());
    model.browser.confirming_delete = None;
    model.dirty = true;
}

fn has_pending_lifecycle(model: &Model, session_id: &str) -> bool {
    model.pending.values().any(|pending| match pending {
        PendingKind::RenameSession(candidate)
        | PendingKind::ArchiveSession(candidate)
        | PendingKind::UnarchiveSession(candidate)
        | PendingKind::DeleteSession(candidate)
        | PendingKind::OpenSession(candidate) => candidate == session_id,
        PendingKind::CreateSession
        | PendingKind::ConfigureCredential
        | PendingKind::RefreshCatalog
        | PendingKind::SelectModel(_)
        | PendingKind::SubmitPrompt(_)
        | PendingKind::CancelAttempt(_)
        | PendingKind::RetryAttempt(_)
        | PendingKind::AnswerPermission(_)
        | PendingKind::ExportTranscript => false,
    })
}

fn open_selected_session(model: &mut Model) -> Vec<UiEffect> {
    let Some(entry) = selected_browser_entry(model).map(|entry| entry.session_id.clone()) else {
        return Vec::new();
    };
    if has_pending_lifecycle(model, &entry) {
        return Vec::new();
    }
    if entry == model.session.session_id {
        close_browser(model);
        return Vec::new();
    }
    if model.session.active_attempt().is_some() {
        model.notice = Some(Notice::Info(
            "Cancel or wait for the active response before switching sessions".to_owned(),
        ));
        model.dirty = true;
        return Vec::new();
    }

    let request_id = model.allocate_request();
    model
        .pending
        .insert(request_id, PendingKind::OpenSession(entry.clone()));
    model.notice = Some(Notice::Info("Opening session...".to_owned()));
    model.dirty = true;
    vec![UiEffect::Dispatch(UiIntent::OpenSession {
        request_id,
        session_id: entry,
    })]
}

fn rename_selected_session(model: &mut Model) -> Vec<UiEffect> {
    let Some(title) = selected_browser_entry(model).map(|entry| entry.title.clone()) else {
        return Vec::new();
    };
    model.browser.renaming = true;
    model.browser.rename_buffer = title;
    model.dirty = true;
    Vec::new()
}

fn submit_browser_rename(model: &mut Model) -> Vec<UiEffect> {
    let Some(session_id) = model.browser.selected.clone() else {
        return Vec::new();
    };
    let title = model.browser.rename_buffer.trim().to_owned();
    if title.is_empty() || title.len() > 128 {
        model.notice = Some(Notice::Failure(UiFailure::new(
            ErrorClass::Validation,
            "Session title must be 1-128 visible characters",
            RetryPolicy::Never,
        )));
        model.dirty = true;
        return Vec::new();
    }
    if has_pending_lifecycle(model, &session_id) {
        return Vec::new();
    }
    model.browser.renaming = false;
    model.browser.rename_buffer.clear();
    let request_id = model.allocate_request();
    model
        .pending
        .insert(request_id, PendingKind::RenameSession(session_id.clone()));
    model.notice = Some(Notice::Info("Saving new title...".to_owned()));
    model.dirty = true;
    vec![UiEffect::Dispatch(UiIntent::RenameSession {
        request_id,
        session_id,
        title,
    })]
}

fn toggle_archive_selected_session(model: &mut Model) -> Vec<UiEffect> {
    let Some((session_id, archived)) =
        selected_browser_entry(model).map(|entry| (entry.session_id.clone(), entry.archived))
    else {
        return Vec::new();
    };
    if has_pending_lifecycle(model, &session_id) {
        return Vec::new();
    }
    let request_id = model.allocate_request();
    let (kind, intent) = if archived {
        (
            PendingKind::UnarchiveSession(session_id.clone()),
            UiIntent::UnarchiveSession {
                request_id,
                session_id: session_id.clone(),
            },
        )
    } else {
        (
            PendingKind::ArchiveSession(session_id.clone()),
            UiIntent::ArchiveSession {
                request_id,
                session_id: session_id.clone(),
            },
        )
    };
    model.pending.insert(request_id, kind);
    model.notice = Some(Notice::Info(if archived {
        "Unarchiving session...".to_owned()
    } else {
        "Archiving session...".to_owned()
    }));
    model.dirty = true;
    vec![UiEffect::Dispatch(intent)]
}

fn request_delete_selected_session(model: &mut Model) -> Vec<UiEffect> {
    let Some(entry) = selected_browser_entry(model) else {
        return Vec::new();
    };
    if entry.active {
        model.notice = Some(Notice::Info(
            "Switch to another session before deleting the active one".to_owned(),
        ));
        model.dirty = true;
        return Vec::new();
    }
    model.browser.confirming_delete = Some(entry.session_id.clone());
    model.notice = Some(Notice::Info(
        "Press Y again to permanently delete; N or Esc cancels".to_owned(),
    ));
    model.dirty = true;
    Vec::new()
}

fn confirm_delete_selected_session(model: &mut Model) -> Vec<UiEffect> {
    let Some(session_id) = model.browser.confirming_delete.take() else {
        // A stray Y with no armed deletion is ignored.
        return Vec::new();
    };
    if has_pending_lifecycle(model, &session_id) {
        return Vec::new();
    }
    let request_id = model.allocate_request();
    model
        .pending
        .insert(request_id, PendingKind::DeleteSession(session_id.clone()));
    model.notice = Some(Notice::Info("Deleting session...".to_owned()));
    model.dirty = true;
    vec![UiEffect::Dispatch(UiIntent::DeleteSession {
        request_id,
        session_id,
    })]
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

/// Opens the transcript search bar and takes the keyboard.
fn open_search(model: &mut Model) {
    model.search.open = true;
    model.search.query.clear();
    model.search.matches.clear();
    model.search.current = None;
    model.focus = Focus::Search;
    model.dirty = true;
}

fn close_search(model: &mut Model) {
    model.search.open = false;
    model.search.query.clear();
    model.search.matches.clear();
    model.search.current = None;
    // A closed search no longer pins the scroll position.
    model.focus = Focus::Composer;
    model.dirty = true;
}

/// Recomputes matches for the current query over rendered transcript lines.
///
/// Matching runs on the same display text the renderer produces so counts,
/// jump targets, and visible rows always agree. Case is ignored; queries are
/// bounded by the composer safety limits through `editable_safe`.
fn refresh_search_matches(model: &mut Model) {
    let query = model.search.query.to_lowercase();
    model.search.matches.clear();
    model.search.current = None;
    if query.is_empty() {
        return;
    }
    let lines = crate::view::transcript_display_lines(model);
    for (row, line) in lines.iter().enumerate() {
        if line.to_lowercase().contains(&query) {
            model.search.matches.push(row);
        }
    }
}

/// Advances to the next or previous match and scrolls it into view.
fn step_search(model: &mut Model, direction: isize) {
    if model.search.matches.is_empty() {
        return;
    }
    let count = model.search.matches.len();
    let next = match model.search.current {
        None => {
            if direction < 0 {
                count - 1
            } else {
                0
            }
        }
        Some(current) => {
            let candidate = current as isize + direction;
            candidate.rem_euclid(count as isize) as usize
        }
    };
    model.search.current = Some(next);

    // Pin the transcript scroll to the matching row: disable tail-follow and
    // set the offset so the row lands mid-viewport when possible.
    let row = model.search.matches[next];
    model.transcript.follow_tail = false;
    model.search_pinned_row = Some(row);
    model.dirty = true;
}

/// Applies keyboard input while the search bar owns the keyboard.
///
/// Quit and fresh-session chords stay global; BackTab walks backwards.
fn handle_search_input(model: &mut Model, input: Input) -> Vec<UiEffect> {
    if matches!(
        input,
        Input {
            key: Key::Char('c' | 'C'),
            ctrl: true,
            ..
        }
    ) {
        model.should_quit = true;
        return vec![UiEffect::Quit];
    }
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
    match input {
        Input { key: Key::Esc, .. } => {
            close_search(model);
            Vec::new()
        }
        Input {
            key: Key::Enter, ..
        } => {
            step_search(model, 1);
            Vec::new()
        }
        Input {
            key: Key::Tab,
            shift: true,
            ..
        } => {
            step_search(model, -1);
            Vec::new()
        }
        Input {
            key: Key::Backspace,
            ..
        } => {
            model.search.query.pop();
            refresh_search_matches(model);
            model.dirty = true;
            Vec::new()
        }
        Input {
            key: Key::Char(character),
            ctrl: false,
            alt: false,
            ..
        } if !character.is_control() => {
            model.search.query.push(character);
            refresh_search_matches(model);
            model.dirty = true;
            Vec::new()
        }
        _ => Vec::new(),
    }
}

/// Steps composer history without ever touching durable state.
///
/// A successful recall replaces the composer content; any ordinary edit
/// afterwards ends the walk so Ctrl+Down returns to the edited draft.
fn recall_history(model: &mut Model, direction: isize) -> Vec<UiEffect> {
    let draft = model.composer.text();
    if let Some(recalled) = model.history.step(direction, &draft) {
        model.composer.reset();
        if !recalled.is_empty() {
            let _ = model
                .composer
                .editor
                .insert_str(crate::text::editable_safe(&recalled));
        }
        model.notice = None;
        model.dirty = true;
    }
    Vec::new()
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
    let outgoing = model.session.session_id.clone();
    let session_changed = session.session_id != outgoing;
    if !session_changed && session.revision < model.session.revision {
        return;
    }
    if session_changed {
        // Stash the outgoing draft, clear the editor, and restore whatever
        // draft belongs to the incoming durable session.
        let current = model.composer.text();
        model.drafts.stash(&outgoing, current);
        model.composer.reset();
        if let Some(saved) = model.drafts.take_for(&session.session_id) {
            let _ = model
                .composer
                .editor
                .insert_str(crate::text::editable_safe(&saved));
        }
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
                    PendingKind::SubmitPrompt(prompt) => {
                        model.history.record(&prompt);
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
                    PendingKind::RenameSession(_) => {
                        model.notice = Some(Notice::Info("Title saved".to_owned()));
                    }
                    PendingKind::ArchiveSession(_) => {
                        model.notice = Some(Notice::Info("Session archived".to_owned()));
                    }
                    PendingKind::UnarchiveSession(_) => {
                        model.notice = Some(Notice::Info("Session unarchived".to_owned()));
                    }
                    PendingKind::DeleteSession(_) => {
                        model.notice = Some(Notice::Info("Session deleted".to_owned()));
                    }
                    PendingKind::OpenSession(_) => {
                        close_browser(model);
                        model.notice = Some(Notice::Info("Session opened".to_owned()));
                    }
                    PendingKind::ExportTranscript => {
                        model.notice = Some(Notice::Info("Transcript exported".to_owned()));
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
                    | PendingKind::AnswerPermission(_)
                    | PendingKind::RenameSession(_)
                    | PendingKind::ArchiveSession(_)
                    | PendingKind::UnarchiveSession(_)
                    | PendingKind::DeleteSession(_)
                    | PendingKind::OpenSession(_)
                    | PendingKind::ExportTranscript,
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

/// Opens the modal contextual help overlay, remembering where the keyboard
/// came from so Esc restores it exactly.
fn open_help(model: &mut Model) {
    if !model.help.open {
        model.help.return_focus = model.focus;
    }
    model.help.open = true;
    model.help.scroll = 0;
    model.focus = Focus::Help;
    model.dirty = true;
}

fn close_help(model: &mut Model) {
    model.help.open = false;
    model.help.scroll = 0;
    model.focus = match model.help.return_focus {
        Focus::Help | Focus::Permission => Focus::Composer,
        restored => restored,
    };
    model.dirty = true;
}

fn scroll_help(model: &mut Model, rows: i32) {
    let next = i32::from(model.help.scroll)
        .saturating_add(rows)
        .clamp(0, i32::from(u16::MAX));
    model.help.scroll = next as u16;
    model.dirty = true;
}

/// Applies keyboard input while the help overlay owns focus.
///
/// Quit stays global; every other key is either navigation or ignored so
/// drafts and durable state stay untouched.
fn handle_help_input(model: &mut Model, input: Input) -> Vec<UiEffect> {
    if matches!(
        input,
        Input {
            key: Key::Char('c' | 'C'),
            ctrl: true,
            ..
        }
    ) {
        model.should_quit = true;
        return vec![UiEffect::Quit];
    }
    match input {
        Input { key: Key::Esc, .. } | Input { key: Key::F(1), .. } => {
            close_help(model);
            Vec::new()
        }
        Input { key: Key::Up, .. }
        | Input {
            key: Key::PageUp, ..
        } => {
            scroll_help(model, -1);
            Vec::new()
        }
        Input { key: Key::Down, .. }
        | Input {
            key: Key::PageDown, ..
        } => {
            scroll_help(model, 1);
            Vec::new()
        }
        _ => Vec::new(),
    }
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
            | PendingKind::AnswerPermission(_)
            | PendingKind::RenameSession(_)
            | PendingKind::ArchiveSession(_)
            | PendingKind::UnarchiveSession(_)
            | PendingKind::DeleteSession(_)
            | PendingKind::OpenSession(_)
            | PendingKind::ExportTranscript => false,
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
