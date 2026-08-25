use std::sync::Arc;

use autoharness_domain::ErrorClass;
use autoharness_settings::{
    ColorMode, ComposerSubmitBehavior, Density, GlyphMode, Layout, TerminalTimestampStyle,
    ThemePreset,
};
use ratatui_textarea::{Input, Key};

use crate::model::{
    AttemptKey, COMMANDS, CatalogProjection, CommandEntry, Focus, LocalPreferenceChange, Message,
    Model, MouseAction, Notice, OverlayKind, PendingKind, ProfileCredentialAction,
    ProfileCredentialEditor, ProfileEditorMode, ProfileEditorState, ProfilesProjection,
    ProviderKindLabel, ProviderProfileDraft, RetryPolicy, Route, SessionProjection,
    SessionsProjection, SettingsPreference, UiEffect, UiFailure, UiIntent, UiNotice,
};
use crate::text::{display_safe, editable_safe};

const MAX_DISPLAY_LABEL_CHARS: usize = 64;
/// Applies one input to local UI state and returns application-owned effects.
#[must_use]
pub fn update(model: &mut Model, message: Message) -> Vec<UiEffect> {
    match message {
        Message::Input(input) => handle_input(model, input),
        Message::Mouse(action) => handle_mouse(model, action),
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
        Message::ProfilesChanged(profiles) => {
            apply_profiles(model, profiles);
            Vec::new()
        }
        Message::SettingsChanged(settings) => {
            model.apply_settings(settings);
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
fn handle_mouse(model: &mut Model, action: MouseAction) -> Vec<UiEffect> {
    if model.overlay() == Some(OverlayKind::Permission) {
        return Vec::new();
    }
    if let Some(overlay) = model.overlay()
        && overlay != OverlayKind::UserProfile
    {
        return Vec::new();
    }
    match action {
        MouseAction::Route(route) => {
            navigate_to_route(model, route);
            Vec::new()
        }
        MouseAction::OpenUserProfile => {
            open_user_profile(model);
            Vec::new()
        }
        MouseAction::ChatSend => submit_prompt(model),
        MouseAction::ChatModels => {
            open_picker(model);
            Vec::new()
        }
        MouseAction::ChatNewSession => create_session(model),
        MouseAction::ChatSessions => {
            navigate_to_route(model, Route::Sessions);
            Vec::new()
        }
        MouseAction::ChatCredential => {
            open_credential(model);
            Vec::new()
        }
        MouseAction::ChatHelp => {
            navigate_to_route(model, Route::Help);
            Vec::new()
        }
        MouseAction::ProfileNew => create_profile_editor(model),
        MouseAction::ProfileCredential => {
            open_profile_credential(model);
            Vec::new()
        }
        MouseAction::ProfileTest => test_selected_profile(model),
        MouseAction::ProfileDefaultModel => set_selected_profile_default_model(model),
        MouseAction::ProfileDisconnect => {
            request_disconnect_profile(model);
            Vec::new()
        }
        MouseAction::ProfileDelete => {
            request_delete_profile(model);
            Vec::new()
        }
        MouseAction::SelectProfile(profile_id) => {
            if model
                .profiles()
                .profiles
                .iter()
                .any(|profile| profile.id == profile_id)
            {
                model.profile_center.selected = Some(profile_id);
                model.dirty = true;
            }
            Vec::new()
        }
        MouseAction::UserProfileSave => commit_user_profile(model),
        MouseAction::UserProfileCancel => {
            close_user_profile(model);
            Vec::new()
        }
    }
}

fn handle_input(model: &mut Model, input: Input) -> Vec<UiEffect> {
    if model.overlay() == Some(OverlayKind::Permission) {
        return handle_permission_input(model, input);
    }

    if let Some(route) = direct_route(&input) {
        navigate_to_route(model, route);
        return Vec::new();
    }

    if matches!(
        input,
        Input {
            key: Key::Char('l' | 'L'),
            ctrl: true,
            ..
        }
    ) {
        navigate_to_route(model, Route::Sessions);
        return Vec::new();
    }
    if matches!(
        input,
        Input {
            key: Key::Char('g' | 'G'),
            ctrl: true,
            ..
        }
    ) {
        navigate_to_route(model, Route::Profiles);
        return Vec::new();
    }
    if matches!(
        input,
        Input {
            key: Key::Char('u' | 'U'),
            alt: true,
            ..
        }
    ) {
        open_user_profile(model);
        return Vec::new();
    }
    if matches!(
        input,
        Input {
            key: Key::Char(','),
            ctrl: true,
            ..
        }
    ) {
        if model.route() == Route::Settings {
            navigate_to_route(model, Route::Chat);
        } else {
            navigate_to_route(model, Route::Settings);
        }
        return Vec::new();
    }
    if matches!(input, Input { key: Key::F(1), .. }) {
        if model.route() == Route::Help {
            close_help(model);
        } else {
            navigate_to_route(model, Route::Help);
        }
        return Vec::new();
    }

    if matches!(
        input,
        Input {
            key: Key::Char('/' | '?'),
            ctrl: true,
            ..
        }
    ) {
        close_active_overlay_state(model);
        open_palette(model);
        return Vec::new();
    }
    if matches!(
        input,
        Input {
            key: Key::Char('f' | 'F'),
            ctrl: true,
            ..
        }
    ) {
        close_active_overlay_state(model);
        if model.route() != Route::Chat {
            navigate_to_route(model, Route::Chat);
        }
        open_search(model);
        return Vec::new();
    }

    if matches!(
        input,
        Input {
            key: Key::Char('p' | 'P'),
            ctrl: true,
            ..
        }
    ) {
        close_active_overlay_state(model);
        open_picker(model);
        return Vec::new();
    }
    if matches!(
        input,
        Input {
            key: Key::Char('k' | 'K'),
            ctrl: true,
            ..
        }
    ) {
        close_active_overlay_state(model);
        open_credential(model);
        return Vec::new();
    }

    if matches!(
        input,
        Input {
            key: Key::Char('n' | 'N'),
            ctrl: true,
            ..
        }
    ) {
        close_active_overlay_state(model);
        navigate_to_route(model, Route::Chat);
        return create_session(model);
    }

    if let Some(overlay) = model.overlay() {
        return match overlay {
            OverlayKind::ModelPicker => handle_picker_input(model, input),
            OverlayKind::SessionCredential => handle_credential_input(model, input),
            OverlayKind::CommandPalette => handle_palette_input(model, input),
            OverlayKind::TranscriptSearch => handle_search_input(model, input),
            OverlayKind::Permission => handle_permission_input(model, input),
            OverlayKind::ProfileCredential => handle_profile_credential_input(model, input),
            OverlayKind::UserProfile => handle_user_profile_input(model, input),
            OverlayKind::Confirmation => match model.route() {
                Route::Sessions => handle_browser_input(model, input),
                Route::Profiles => handle_profile_input(model, input),
                Route::Chat | Route::Settings | Route::Help => Vec::new(),
            },
        };
    }
    match model.route() {
        Route::Chat => handle_chat_input(model, input),
        Route::Sessions => handle_browser_input(model, input),
        Route::Profiles => handle_profile_input(model, input),
        Route::Settings => handle_settings_input(model, input),
        Route::Help => handle_help_input(model, input),
    }
}

fn direct_route(input: &Input) -> Option<Route> {
    if !input.alt && !input.ctrl {
        return None;
    }
    match &input.key {
        Key::Char('1') => Some(Route::Chat),
        Key::Char('2') => Some(Route::Sessions),
        Key::Char('3') => Some(Route::Profiles),
        Key::Char('4') => Some(Route::Settings),
        Key::Char('5') => Some(Route::Help),
        _ => None,
    }
}

fn handle_chat_input(model: &mut Model, input: Input) -> Vec<UiEffect> {
    match input {
        Input {
            key: Key::Char('s' | 'S'),
            ctrl: true,
            ..
        } if composer_submit_behavior(model) == ComposerSubmitBehavior::ControlS => {
            submit_prompt(model)
        }
        Input {
            key: Key::Enter,
            ctrl: true,
            ..
        } if composer_submit_behavior(model) == ComposerSubmitBehavior::ControlS => {
            submit_prompt(model)
        }
        input @ Input {
            key: Key::Enter,
            ctrl: false,
            ..
        } if composer_submit_behavior(model) == ComposerSubmitBehavior::Enter => {
            maybe_slash_command(model, &input).unwrap_or_else(|| submit_prompt(model))
        }
        Input {
            key: Key::Char('s' | 'S') | Key::Enter,
            ctrl: true,
            ..
        } => insert_composer_newline(model),
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
        Input {
            key: Key::Char('x' | 'X'),
            ctrl: true,
            ..
        } => {
            model.tools_expanded = !model.tools_expanded;
            model.dirty = true;
            Vec::new()
        }
        input => {
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

fn handle_settings_input(model: &mut Model, input: Input) -> Vec<UiEffect> {
    match input {
        Input { key: Key::Esc, .. } => {
            navigate_to_route(model, Route::Chat);
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
        } if model.settings_workspace.display_label_editor.is_some() => commit_display_label(model),
        Input {
            key: Key::Backspace,
            ..
        } if model.settings_workspace.display_label_editor.is_some() => {
            if let Some(editor) = model.settings_workspace.display_label_editor.as_mut() {
                editor.pop();
                model.dirty = true;
            }
            Vec::new()
        }
        Input {
            key: Key::Char(character),
            ctrl: false,
            alt: false,
            ..
        } if model.settings_workspace.display_label_editor.is_some() => {
            if let Some(editor) = model.settings_workspace.display_label_editor.as_mut()
                && editor.chars().count() < MAX_DISPLAY_LABEL_CHARS
            {
                editor.push_str(&display_safe(&character.to_string()));
                model.dirty = true;
            }
            Vec::new()
        }
        Input { key: Key::Up, .. } => {
            move_settings_selection(model, -1);
            Vec::new()
        }
        Input { key: Key::Down, .. } => {
            move_settings_selection(model, 1);
            Vec::new()
        }
        Input {
            key: Key::PageUp, ..
        } => {
            move_settings_selection(model, -3);
            Vec::new()
        }
        Input {
            key: Key::PageDown, ..
        } => {
            move_settings_selection(model, 3);
            Vec::new()
        }
        Input { key: Key::Home, .. } => {
            move_settings_selection_to(model, 0);
            Vec::new()
        }
        Input { key: Key::End, .. } => {
            move_settings_selection_to(model, SettingsPreference::ALL.len().saturating_sub(1));
            Vec::new()
        }
        Input { key: Key::Left, .. } => change_selected_preference(model, -1),
        Input {
            key: Key::Right, ..
        } => change_selected_preference(model, 1),
        Input {
            key: Key::Enter, ..
        } if selected_settings_preference(model) == SettingsPreference::DisplayLabel => {
            begin_display_label_edit(model);
            Vec::new()
        }
        Input {
            key: Key::Char('r' | 'R'),
            ctrl: false,
            ..
        } => reset_selected_preference(model),
        Input {
            key: Key::Char('d' | 'D'),
            ctrl: false,
            ..
        } => default_selected_preference(model),
        Input {
            key: Key::Char('p' | 'P'),
            ..
        } => {
            navigate_to_route(model, Route::Profiles);
            Vec::new()
        }
        _ => Vec::new(),
    }
}
fn handle_user_profile_input(model: &mut Model, input: Input) -> Vec<UiEffect> {
    match input {
        Input { key: Key::Esc, .. } => {
            close_user_profile(model);
            Vec::new()
        }
        Input {
            key: Key::Enter, ..
        }
        | Input {
            key: Key::Char('s' | 'S'),
            ctrl: true,
            ..
        } => commit_user_profile(model),
        Input {
            key: Key::Backspace,
            ..
        } => {
            if let Some(editor) = model.user_profile.display_label_editor.as_mut() {
                editor.pop();
                model.dirty = true;
            }
            Vec::new()
        }
        Input {
            key: Key::Char(character),
            ctrl: false,
            alt: false,
            ..
        } if !character.is_control() => {
            if let Some(editor) = model.user_profile.display_label_editor.as_mut()
                && editor.chars().count() < MAX_DISPLAY_LABEL_CHARS
            {
                editor.push(character);
                model.dirty = true;
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}
fn commit_user_profile(model: &mut Model) -> Vec<UiEffect> {
    let value = model
        .user_profile
        .display_label_editor
        .take()
        .unwrap_or_default();
    let value = (!value.trim().is_empty()).then_some(value);
    let effects = dispatch_local_preference(model, LocalPreferenceChange::DisplayLabel(value));
    let _ = model.close_overlay(OverlayKind::UserProfile);
    effects
}

fn selected_settings_preference(model: &Model) -> SettingsPreference {
    SettingsPreference::at(model.settings_workspace.selected)
}

fn move_settings_selection(model: &mut Model, direction: isize) {
    let current = model.settings_workspace.selected;
    let last = SettingsPreference::ALL.len().saturating_sub(1);
    move_settings_selection_to(model, current.saturating_add_signed(direction).min(last));
}

fn move_settings_selection_to(model: &mut Model, selected: usize) {
    let last = SettingsPreference::ALL.len().saturating_sub(1);
    model.settings_workspace.selected = selected.min(last);
    model.settings_workspace.scroll = 0;
    model.settings_workspace.display_label_editor = None;
    model.dirty = true;
}

fn begin_display_label_edit(model: &mut Model) {
    let label = model
        .settings()
        .local_profile
        .display_label()
        .value()
        .as_ref()
        .map_or_else(String::new, |label| label.as_str().to_owned());
    model.settings_workspace.display_label_editor = Some(label);
    model.notice = None;
    model.dirty = true;
}

fn commit_display_label(model: &mut Model) -> Vec<UiEffect> {
    let value = model
        .settings_workspace
        .display_label_editor
        .take()
        .unwrap_or_default();
    let value = (!value.trim().is_empty()).then_some(value);
    dispatch_local_preference(model, LocalPreferenceChange::DisplayLabel(value))
}

fn change_selected_preference(model: &mut Model, direction: isize) -> Vec<UiEffect> {
    let preferences = model.settings().local_profile.preferences();
    let change = match selected_settings_preference(model) {
        SettingsPreference::DisplayLabel => return Vec::new(),
        SettingsPreference::ThemePreset => LocalPreferenceChange::ThemePreset(Some(cycle(
            *preferences.theme_preset().value(),
            &[ThemePreset::System, ThemePreset::Light, ThemePreset::Dark],
            direction,
        ))),
        SettingsPreference::ColorMode => LocalPreferenceChange::ColorMode(Some(cycle(
            *preferences.color_mode().value(),
            &[
                ColorMode::Color,
                ColorMode::NoColor,
                ColorMode::HighContrast,
            ],
            direction,
        ))),
        SettingsPreference::GlyphMode => LocalPreferenceChange::GlyphMode(Some(cycle(
            *preferences.glyph_mode().value(),
            &[GlyphMode::Unicode, GlyphMode::Ascii],
            direction,
        ))),
        SettingsPreference::ReducedMotion => {
            LocalPreferenceChange::ReducedMotion(Some(!*preferences.reduced_motion().value()))
        }
        SettingsPreference::Density => LocalPreferenceChange::Density(Some(cycle(
            *preferences.density().value(),
            &[Density::Comfortable, Density::Compact],
            direction,
        ))),
        SettingsPreference::Layout => LocalPreferenceChange::Layout(Some(cycle(
            *preferences.layout().value(),
            &[Layout::Responsive, Layout::SingleColumn],
            direction,
        ))),
        SettingsPreference::TerminalTimestampStyle => {
            LocalPreferenceChange::TerminalTimestampStyle(Some(cycle(
                *preferences.terminal_timestamp_style().value(),
                &[
                    TerminalTimestampStyle::Relative,
                    TerminalTimestampStyle::Absolute,
                    TerminalTimestampStyle::Hidden,
                ],
                direction,
            )))
        }
        SettingsPreference::ComposerSubmitBehavior => {
            LocalPreferenceChange::ComposerSubmitBehavior(Some(cycle(
                *preferences.composer_submit_behavior().value(),
                &[
                    ComposerSubmitBehavior::ControlS,
                    ComposerSubmitBehavior::Enter,
                ],
                direction,
            )))
        }
    };
    dispatch_local_preference(model, change)
}

fn reset_selected_preference(model: &mut Model) -> Vec<UiEffect> {
    dispatch_local_preference(
        model,
        match selected_settings_preference(model) {
            SettingsPreference::DisplayLabel => LocalPreferenceChange::DisplayLabel(None),
            SettingsPreference::ThemePreset => LocalPreferenceChange::ThemePreset(None),
            SettingsPreference::ColorMode => LocalPreferenceChange::ColorMode(None),
            SettingsPreference::GlyphMode => LocalPreferenceChange::GlyphMode(None),
            SettingsPreference::ReducedMotion => LocalPreferenceChange::ReducedMotion(None),
            SettingsPreference::Density => LocalPreferenceChange::Density(None),
            SettingsPreference::Layout => LocalPreferenceChange::Layout(None),
            SettingsPreference::TerminalTimestampStyle => {
                LocalPreferenceChange::TerminalTimestampStyle(None)
            }
            SettingsPreference::ComposerSubmitBehavior => {
                LocalPreferenceChange::ComposerSubmitBehavior(None)
            }
        },
    )
}

fn default_selected_preference(model: &mut Model) -> Vec<UiEffect> {
    dispatch_local_preference(
        model,
        match selected_settings_preference(model) {
            SettingsPreference::DisplayLabel => LocalPreferenceChange::DisplayLabel(None),
            SettingsPreference::ThemePreset => {
                LocalPreferenceChange::ThemePreset(Some(ThemePreset::System))
            }
            SettingsPreference::ColorMode => {
                LocalPreferenceChange::ColorMode(Some(ColorMode::Color))
            }
            SettingsPreference::GlyphMode => {
                LocalPreferenceChange::GlyphMode(Some(GlyphMode::Unicode))
            }
            SettingsPreference::ReducedMotion => LocalPreferenceChange::ReducedMotion(Some(false)),
            SettingsPreference::Density => {
                LocalPreferenceChange::Density(Some(Density::Comfortable))
            }
            SettingsPreference::Layout => LocalPreferenceChange::Layout(Some(Layout::Responsive)),
            SettingsPreference::TerminalTimestampStyle => {
                LocalPreferenceChange::TerminalTimestampStyle(Some(
                    TerminalTimestampStyle::Relative,
                ))
            }
            SettingsPreference::ComposerSubmitBehavior => {
                LocalPreferenceChange::ComposerSubmitBehavior(Some(
                    ComposerSubmitBehavior::ControlS,
                ))
            }
        },
    )
}

fn dispatch_local_preference(model: &mut Model, change: LocalPreferenceChange) -> Vec<UiEffect> {
    if model
        .pending
        .values()
        .any(|pending| matches!(pending, PendingKind::UpdateLocalPreference(_)))
    {
        return Vec::new();
    }
    let request_id = model.allocate_request();
    model.pending.insert(
        request_id,
        PendingKind::UpdateLocalPreference(change.clone()),
    );
    model.notice = Some(Notice::Info("Saving local preference...".to_owned()));
    model.dirty = true;
    vec![UiEffect::Dispatch(UiIntent::UpdateLocalPreference {
        request_id,
        change,
    })]
}

fn cycle<T: Copy + Eq>(current: T, values: &[T], direction: isize) -> T {
    let index = values
        .iter()
        .position(|value| *value == current)
        .unwrap_or(0);
    let next = if direction < 0 {
        index
            .checked_sub(1)
            .unwrap_or(values.len().saturating_sub(1))
    } else {
        (index + 1) % values.len()
    };
    values[next]
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
        "chat" => {
            navigate_to_route(model, Route::Chat);
            Vec::new()
        }
        "sessions" => {
            navigate_to_route(model, Route::Sessions);
            Vec::new()
        }
        "profiles" => {
            navigate_to_route(model, Route::Profiles);
            Vec::new()
        }
        "user-profile" => {
            open_user_profile(model);
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
            navigate_to_route(model, Route::Settings);
            Vec::new()
        }
        "refresh-models" => refresh_catalog(model),
        "new-session" => create_session(model),
        "help" => {
            navigate_to_route(model, Route::Help);
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

fn open_user_profile(model: &mut Model) {
    if model.overlay() == Some(OverlayKind::Permission) {
        return;
    }
    let label = model
        .profiles()
        .user
        .display_label
        .clone()
        .unwrap_or_default();
    model.user_profile.display_label_editor = Some(label);
    model.notice = None;
    let _ = model.open_overlay(OverlayKind::UserProfile);
    model.dirty = true;
}

fn close_user_profile(model: &mut Model) {
    model.user_profile.display_label_editor = None;
    let _ = model.close_overlay(OverlayKind::UserProfile);
    model.notice = None;
    model.dirty = true;
}

/// Opens the single command-palette modal and captures the active route.
fn open_palette(model: &mut Model) {
    if !model.open_overlay(OverlayKind::CommandPalette) {
        return;
    }
    model.palette.query.clear();
    model.palette.selected = None;
    normalize_palette_selection(model);
}

fn close_palette(model: &mut Model) {
    model.palette.query.clear();
    model.palette.selected = None;
    let _ = model.close_overlay(OverlayKind::CommandPalette);
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
    close_palette(model);
    execute_command(model, entry)
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

    // While an archiving is armed, Y confirms and N or Esc cancels.
    if model.browser.confirming_archive.is_some() {
        match input {
            Input {
                key: Key::Char('y' | 'Y'),
                ctrl: false,
                ..
            } => return confirm_archive_selected_session(model),
            Input {
                key: Key::Char('n' | 'N'),
                ctrl: false,
                ..
            }
            | Input { key: Key::Esc, .. } => {
                model.browser.confirming_archive = None;
                let _ = model.close_overlay(OverlayKind::Confirmation);
                model.notice = None;
                model.dirty = true;
                return Vec::new();
            }
            _ => return Vec::new(),
        }
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
                let _ = model.close_overlay(OverlayKind::Confirmation);
                model.notice = None;
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
            key: Key::Char('z' | 'Z'),
            ctrl: true,
            ..
        } => undo_last_lifecycle(model),
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

fn apply_profiles(model: &mut Model, profiles: Arc<ProfilesProjection>) {
    model.apply_profiles(profiles);
}

fn close_active_overlay_state(model: &mut Model) {
    let Some(overlay) = model.overlay() else {
        return;
    };
    if overlay == OverlayKind::Permission {
        return;
    }
    match overlay {
        OverlayKind::ModelPicker => {
            model.picker.query.clear();
        }
        OverlayKind::SessionCredential => {
            model.credential.clear();
        }
        OverlayKind::CommandPalette => {
            model.palette.query.clear();
            model.palette.selected = None;
        }
        OverlayKind::TranscriptSearch => {
            model.search.query.clear();
            model.search.matches.clear();
            model.search.current = None;
            model.search_pinned_row = None;
        }
        OverlayKind::ProfileCredential => {
            model.profile_center.credential = None;
        }
        OverlayKind::UserProfile => {
            model.user_profile.display_label_editor = None;
        }
        OverlayKind::Confirmation => {
            model.browser.confirming_archive = None;
            model.browser.confirming_delete = None;
            model.profile_center.confirming_disconnect = None;
            model.profile_center.confirming_delete = None;
        }
        OverlayKind::Permission => {}
    }
    let _ = model.close_overlay(overlay);
}

fn navigate_to_route(model: &mut Model, route: Route) {
    if model.overlay() == Some(OverlayKind::Permission) {
        return;
    }
    close_active_overlay_state(model);
    if model.route() == Route::Sessions && route != Route::Sessions {
        model.browser.renaming = false;
        model.browser.rename_buffer.clear();
        model.browser.confirming_delete = None;
        model.browser.confirming_archive = None;
    }
    if model.route() == Route::Profiles && route != Route::Profiles {
        model.profile_center.editor = None;
        model.profile_center.credential = None;
        model.profile_center.confirming_delete = None;
        model.profile_center.confirming_disconnect = None;
    }
    if model.route() == Route::Settings && route != Route::Settings {
        model.settings_workspace.display_label_editor = None;
    }
    if !model.navigate(route) {
        return;
    }
    match route {
        Route::Chat | Route::Settings => {}
        Route::Sessions => model.sync_browser_selection(),
        Route::Profiles => model.sync_profile_selection(),
        Route::Help => model.help.scroll = 0,
    }
    model.notice = None;
}

fn close_profile_center(model: &mut Model) {
    navigate_to_route(model, Route::Chat);
}

fn create_profile_editor(model: &mut Model) -> Vec<UiEffect> {
    model.profile_center.editor = Some(ProfileEditorState {
        mode: ProfileEditorMode::Create,
        source_id: None,
        field: 0,
        id: String::new(),
        kind: ProviderKindLabel::Gemini,
        base_url: String::new(),
        project: String::new(),
        auth_header: String::new(),
    });
    model.notice = None;
    model.dirty = true;
    Vec::new()
}

fn handle_profile_input(model: &mut Model, input: Input) -> Vec<UiEffect> {
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
    if model.profile_center.credential.is_some() {
        return handle_profile_credential_input(model, input);
    }
    if model.profile_center.editor.is_some() {
        return handle_profile_editor_input(model, input);
    }
    if let Some(profile_id) = model.profile_center.confirming_disconnect.clone() {
        return match input {
            Input {
                key: Key::Char('y' | 'Y'),
                ctrl: false,
                ..
            } => {
                model.profile_center.confirming_disconnect = None;
                dispatch_disconnect_profile(model, profile_id)
            }
            Input {
                key: Key::Char('n' | 'N'),
                ctrl: false,
                ..
            }
            | Input { key: Key::Esc, .. } => {
                model.profile_center.confirming_disconnect = None;
                let _ = model.close_overlay(OverlayKind::Confirmation);
                model.notice = None;
                model.dirty = true;
                Vec::new()
            }
            _ => Vec::new(),
        };
    }
    if let Some(profile_id) = model.profile_center.confirming_delete.clone() {
        return match input {
            Input {
                key: Key::Char('y' | 'Y'),
                ctrl: false,
                ..
            } => {
                model.profile_center.confirming_delete = None;
                dispatch_delete_profile(model, profile_id)
            }
            Input {
                key: Key::Char('n' | 'N'),
                ctrl: false,
                ..
            }
            | Input { key: Key::Esc, .. } => {
                model.profile_center.confirming_delete = None;
                let _ = model.close_overlay(OverlayKind::Confirmation);
                model.notice = None;
                model.dirty = true;
                Vec::new()
            }
            _ => Vec::new(),
        };
    }

    match input {
        Input { key: Key::Esc, .. } => {
            close_profile_center(model);
            Vec::new()
        }
        Input { key: Key::Up, .. } => {
            move_profile_selection(model, -1);
            Vec::new()
        }
        Input { key: Key::Down, .. } => {
            move_profile_selection(model, 1);
            Vec::new()
        }
        Input {
            key: Key::Enter, ..
        } => activate_selected_profile(model),
        Input {
            key: Key::Char('n' | 'N'),
            alt: true,
            ..
        } => create_profile_editor(model),
        Input {
            key: Key::Char('e' | 'E'),
            alt: true,
            ..
        } => {
            open_profile_editor(model, ProfileEditorMode::Edit);
            Vec::new()
        }
        Input {
            key: Key::Char('d' | 'D'),
            alt: true,
            ..
        } => {
            open_profile_editor(model, ProfileEditorMode::Duplicate);
            Vec::new()
        }
        Input {
            key: Key::Char('k' | 'K'),
            alt: true,
            ..
        } => {
            open_profile_credential(model);
            Vec::new()
        }
        Input {
            key: Key::Char('t' | 'T'),
            alt: true,
            ..
        } => test_selected_profile(model),
        Input {
            key: Key::Char('m' | 'M'),
            alt: true,
            ..
        } => set_selected_profile_default_model(model),
        Input {
            key: Key::Char('x' | 'X'),
            alt: true,
            ..
        } => {
            request_disconnect_profile(model);
            Vec::new()
        }
        Input {
            key: Key::Delete, ..
        } => {
            request_delete_profile(model);
            Vec::new()
        }
        Input {
            key: Key::Backspace,
            ..
        } => {
            model.profile_center.query.pop();
            model.sync_profile_selection();
            model.dirty = true;
            Vec::new()
        }
        Input {
            key: Key::Char(character),
            ctrl: false,
            alt: false,
            ..
        } if !character.is_control() => {
            model.profile_center.query.push(character);
            model.sync_profile_selection();
            model.dirty = true;
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn open_profile_editor(model: &mut Model, mode: ProfileEditorMode) {
    let Some(profile) = model.selected_profile().cloned() else {
        model.notice = Some(Notice::Info("Create a profile first".to_owned()));
        model.dirty = true;
        return;
    };
    model.profile_center.editor = Some(ProfileEditorState {
        mode,
        source_id: Some(profile.id.clone()),
        field: if mode == ProfileEditorMode::Duplicate {
            0
        } else {
            1
        },
        id: if mode == ProfileEditorMode::Duplicate {
            String::new()
        } else {
            profile.id
        },
        kind: profile.kind,
        base_url: profile.base_url,
        project: profile.project,
        auth_header: profile.auth_header,
    });
    model.notice = None;
    model.dirty = true;
}

fn handle_profile_editor_input(model: &mut Model, input: Input) -> Vec<UiEffect> {
    match input {
        Input { key: Key::Esc, .. } => {
            model.profile_center.editor = None;
            model.notice = None;
            model.dirty = true;
            Vec::new()
        }
        Input {
            key: Key::Enter, ..
        } => submit_profile_editor(model),
        Input {
            key: Key::Tab,
            shift,
            ..
        } => {
            let editor = model
                .profile_center
                .editor
                .as_mut()
                .expect("profile editor is open");
            let count = if editor.mode == ProfileEditorMode::Duplicate {
                1
            } else {
                editor.field_count()
            };
            let mut field = if shift {
                editor.field.checked_sub(1).unwrap_or(count - 1)
            } else {
                (editor.field + 1) % count
            };
            if editor.mode == ProfileEditorMode::Edit && field == 0 {
                field = if shift { count - 1 } else { 1 };
            }
            editor.field = field;
            model.dirty = true;
            Vec::new()
        }
        Input {
            key: Key::Left | Key::Right,
            ..
        } => {
            let editor = model
                .profile_center
                .editor
                .as_mut()
                .expect("profile editor is open");
            if editor.mode != ProfileEditorMode::Duplicate && editor.field == 1 {
                editor.kind = match editor.kind {
                    ProviderKindLabel::Gemini => ProviderKindLabel::Router,
                    ProviderKindLabel::Router => ProviderKindLabel::Gemini,
                };
            }
            model.dirty = true;
            Vec::new()
        }
        Input {
            key: Key::Backspace,
            ..
        } => {
            let editor = model
                .profile_center
                .editor
                .as_mut()
                .expect("profile editor is open");
            if let Some(field) = profile_editor_field(editor) {
                field.pop();
            }
            model.dirty = true;
            Vec::new()
        }
        Input {
            key: Key::Char(character),
            ctrl: false,
            alt: false,
            ..
        } if !character.is_control() => {
            let editor = model
                .profile_center
                .editor
                .as_mut()
                .expect("profile editor is open");
            if let Some(field) = profile_editor_field(editor)
                && field.len() < 2_048
            {
                field.push(character);
            }
            model.dirty = true;
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn profile_editor_field(editor: &mut ProfileEditorState) -> Option<&mut String> {
    match editor.field {
        0 if editor.mode != ProfileEditorMode::Edit => Some(&mut editor.id),
        2 if editor.kind == ProviderKindLabel::Router => Some(&mut editor.base_url),
        3 if editor.kind == ProviderKindLabel::Router => Some(&mut editor.project),
        4 if editor.kind == ProviderKindLabel::Router => Some(&mut editor.auth_header),
        _ => None,
    }
}

fn submit_profile_editor(model: &mut Model) -> Vec<UiEffect> {
    let Some(editor) = model.profile_center.editor.as_ref() else {
        return Vec::new();
    };
    if editor.id.trim().is_empty() {
        model.notice = Some(Notice::Failure(UiFailure::new(
            ErrorClass::Validation,
            "Profile name must not be empty",
            RetryPolicy::Never,
        )));
        model.dirty = true;
        return Vec::new();
    }
    if editor.kind == ProviderKindLabel::Router && editor.base_url.trim().is_empty() {
        model.notice = Some(Notice::Failure(UiFailure::new(
            ErrorClass::Validation,
            "Router profiles require a base URL",
            RetryPolicy::Never,
        )));
        model.dirty = true;
        return Vec::new();
    }
    let request_id = model.allocate_request();
    let editor = model
        .profile_center
        .editor
        .take()
        .expect("profile editor is open");
    let effect = if editor.mode == ProfileEditorMode::Duplicate {
        let source = editor.source_id.expect("duplicate editor has a source");
        let destination = editor.id;
        model.pending.insert(
            request_id,
            PendingKind::DuplicateProfile {
                source: source.clone(),
                destination: destination.clone(),
            },
        );
        UiIntent::DuplicateProfile {
            request_id,
            source,
            destination,
        }
    } else {
        let profile = ProviderProfileDraft {
            id: editor.id,
            kind: editor.kind,
            base_url: editor.base_url,
            project: editor.project,
            auth_header: editor.auth_header,
        };
        model
            .pending
            .insert(request_id, PendingKind::UpsertProfile(profile.clone()));
        UiIntent::UpsertProfile {
            request_id,
            profile,
        }
    };
    model.notice = Some(Notice::Info("Saving provider profile...".to_owned()));
    model.dirty = true;
    vec![UiEffect::Dispatch(effect)]
}

fn open_profile_credential(model: &mut Model) {
    let Some(profile) = model.selected_profile().cloned() else {
        model.notice = Some(Notice::Info("Create a profile first".to_owned()));
        model.dirty = true;
        return;
    };
    if matches!(
        profile.credential_state,
        crate::model::ProfileCredentialStateLabel::RecoveryPending
    ) {
        model.notice = Some(Notice::Info(
            "Credential repair is pending; retry after the platform vault is available".to_owned(),
        ));
        model.dirty = true;
        return;
    }
    let action = if matches!(
        profile.credential_state,
        crate::model::ProfileCredentialStateLabel::Stored
    ) {
        ProfileCredentialAction::Replace
    } else {
        ProfileCredentialAction::Save
    };
    model.profile_center.credential = Some(ProfileCredentialEditor::new(profile.id, action));
    if !model.open_overlay(OverlayKind::ProfileCredential) {
        model.profile_center.credential = None;
        return;
    }
    model.notice = None;
}

fn handle_profile_credential_input(model: &mut Model, input: Input) -> Vec<UiEffect> {
    match input {
        Input { key: Key::Esc, .. } => {
            model.profile_center.credential = None;
            model.notice = None;
            let _ = model.close_overlay(OverlayKind::ProfileCredential);
            Vec::new()
        }
        Input {
            key: Key::Enter, ..
        } => submit_profile_credential(model),
        Input {
            key: Key::Backspace,
            ..
        } => {
            model
                .profile_center
                .credential
                .as_mut()
                .expect("credential editor is open")
                .pop();
            model.dirty = true;
            Vec::new()
        }
        Input {
            key: Key::Char(character),
            ctrl: false,
            alt: false,
            ..
        } => {
            let result = model
                .profile_center
                .credential
                .as_mut()
                .expect("credential editor is open")
                .append_character(character);
            match result {
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

fn submit_profile_credential(model: &mut Model) -> Vec<UiEffect> {
    let (profile_id, action, credential) = {
        let editor = model
            .profile_center
            .credential
            .as_mut()
            .expect("credential editor is open");
        let credential = match editor.take() {
            Ok(credential) => credential,
            Err(message) => {
                model.notice = Some(Notice::Failure(UiFailure::new(
                    ErrorClass::Validation,
                    message,
                    RetryPolicy::Never,
                )));
                model.dirty = true;
                return Vec::new();
            }
        };
        (editor.profile_id.clone(), editor.action, credential)
    };
    model.profile_center.credential = None;
    let _ = model.close_overlay(OverlayKind::ProfileCredential);
    let request_id = model.allocate_request();
    let intent = match action {
        ProfileCredentialAction::Save => {
            model.pending.insert(
                request_id,
                PendingKind::SaveProfileCredential(profile_id.clone()),
            );
            UiIntent::SaveProfileCredential {
                request_id,
                profile_id,
                credential,
            }
        }
        ProfileCredentialAction::Replace => {
            model.pending.insert(
                request_id,
                PendingKind::ReplaceProfileCredential(profile_id.clone()),
            );
            UiIntent::ReplaceProfileCredential {
                request_id,
                profile_id,
                credential,
            }
        }
    };
    model.notice = Some(Notice::Info(
        "Saving credential in the operating-system vault...".to_owned(),
    ));
    model.dirty = true;
    vec![UiEffect::Dispatch(intent)]
}

fn activate_selected_profile(model: &mut Model) -> Vec<UiEffect> {
    let Some(profile_id) = model.profile_selection().map(str::to_owned) else {
        return Vec::new();
    };
    let request_id = model.allocate_request();
    model
        .pending
        .insert(request_id, PendingKind::ActivateProfile(profile_id.clone()));
    model.notice = Some(Notice::Info(
        "Switching active provider profile...".to_owned(),
    ));
    model.dirty = true;
    vec![UiEffect::Dispatch(UiIntent::ActivateProfile {
        request_id,
        profile_id,
    })]
}

fn test_selected_profile(model: &mut Model) -> Vec<UiEffect> {
    let Some(profile_id) = model.profile_selection().map(str::to_owned) else {
        return Vec::new();
    };
    let request_id = model.allocate_request();
    model
        .pending
        .insert(request_id, PendingKind::TestProfile(profile_id.clone()));
    model.notice = Some(Notice::Info("Testing provider connection...".to_owned()));
    model.dirty = true;
    vec![UiEffect::Dispatch(UiIntent::TestProfile {
        request_id,
        profile_id,
    })]
}
fn set_selected_profile_default_model(model: &mut Model) -> Vec<UiEffect> {
    let Some(profile_id) = model.profile_selection().map(str::to_owned) else {
        return Vec::new();
    };
    let request_id = model.allocate_request();
    model.pending.insert(
        request_id,
        PendingKind::SetProfileDefaultModel(profile_id.clone()),
    );
    model.notice = Some(Notice::Info(
        "Saving the current selected model as this profile's default...".to_owned(),
    ));
    model.dirty = true;
    vec![UiEffect::Dispatch(UiIntent::SetProfileDefaultModel {
        request_id,
        profile_id,
    })]
}

fn request_disconnect_profile(model: &mut Model) {
    let Some(profile) = model.selected_profile() else {
        return;
    };
    if !matches!(
        profile.credential_state,
        crate::model::ProfileCredentialStateLabel::Stored
    ) {
        model.notice = Some(Notice::Info(
            "The selected profile has no stored credential".to_owned(),
        ));
        model.dirty = true;
        return;
    }
    let profile_id = profile.id.clone();
    model.profile_center.confirming_disconnect = Some(profile_id.clone());
    let _ = model.open_overlay(OverlayKind::Confirmation);
    model.notice = Some(Notice::Info(format!(
        "Disconnect stored credential for '{profile_id}'? Y confirm / N cancel"
    )));
    model.dirty = true;
}

fn dispatch_disconnect_profile(model: &mut Model, profile_id: String) -> Vec<UiEffect> {
    let _ = model.close_overlay(OverlayKind::Confirmation);
    let request_id = model.allocate_request();
    model.pending.insert(
        request_id,
        PendingKind::DisconnectProfile(profile_id.clone()),
    );
    model.notice = Some(Notice::Info(
        "Disconnecting stored credential...".to_owned(),
    ));
    model.dirty = true;
    vec![UiEffect::Dispatch(UiIntent::DisconnectProfile {
        request_id,
        profile_id,
    })]
}

fn request_delete_profile(model: &mut Model) {
    let Some(profile_id) = model.profile_selection().map(str::to_owned) else {
        return;
    };
    model.profile_center.confirming_delete = Some(profile_id.clone());
    let _ = model.open_overlay(OverlayKind::Confirmation);
    model.notice = Some(Notice::Info(format!(
        "Delete profile '{profile_id}' and its stored credential? Y confirm / N cancel"
    )));
    model.dirty = true;
}

fn dispatch_delete_profile(model: &mut Model, profile_id: String) -> Vec<UiEffect> {
    let _ = model.close_overlay(OverlayKind::Confirmation);
    let request_id = model.allocate_request();
    model
        .pending
        .insert(request_id, PendingKind::DeleteProfile(profile_id.clone()));
    model.notice = Some(Notice::Info("Deleting provider profile...".to_owned()));
    model.dirty = true;
    vec![UiEffect::Dispatch(UiIntent::DeleteProfile {
        request_id,
        profile_id,
    })]
}

fn move_profile_selection(model: &mut Model, direction: isize) {
    let visible: Vec<String> = model
        .filtered_profiles()
        .map(|profile| profile.id.clone())
        .collect();
    if visible.is_empty() {
        model.profile_center.selected = None;
        model.dirty = true;
        return;
    }
    let current = model
        .profile_center
        .selected
        .as_ref()
        .and_then(|selected| visible.iter().position(|profile| profile == selected))
        .unwrap_or(0);
    let next = current
        .saturating_add_signed(direction)
        .min(visible.len().saturating_sub(1));
    model.profile_center.selected = visible.get(next).cloned();
    model.dirty = true;
}

fn apply_sessions(model: &mut Model, sessions: Arc<SessionsProjection>) {
    model.sessions = sessions;
    model.sync_browser_selection();
    model.dirty = true;
}

fn close_browser(model: &mut Model) {
    navigate_to_route(model, Route::Chat);
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
    model.browser.confirming_archive = None;
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
        | PendingKind::UpsertProfile(_)
        | PendingKind::DuplicateProfile { .. }
        | PendingKind::ActivateProfile(_)
        | PendingKind::SaveProfileCredential(_)
        | PendingKind::ReplaceProfileCredential(_)
        | PendingKind::TestProfile(_)
        | PendingKind::SetProfileDefaultModel(_)
        | PendingKind::DisconnectProfile(_)
        | PendingKind::UpdateLocalPreference(_)
        | PendingKind::DeleteProfile(_)
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
    // Archiving hides work from the default view, so it arms for explicit
    // confirmation; unarchiving is the safe direction and runs immediately.
    if !archived {
        model.browser.confirming_archive = Some(session_id);
        let _ = model.open_overlay(OverlayKind::Confirmation);
        model.notice = Some(Notice::Info(
            "Press Y again to archive; N or Esc cancels".to_owned(),
        ));
        model.dirty = true;
        return Vec::new();
    }
    dispatch_unarchive(model, session_id)
}

fn confirm_archive_selected_session(model: &mut Model) -> Vec<UiEffect> {
    let Some(session_id) = model.browser.confirming_archive.take() else {
        return Vec::new();
    };
    let _ = model.close_overlay(OverlayKind::Confirmation);
    if has_pending_lifecycle(model, &session_id) {
        return Vec::new();
    }
    let request_id = model.allocate_request();
    model
        .pending
        .insert(request_id, PendingKind::ArchiveSession(session_id.clone()));
    model.notice = Some(Notice::Info("Archiving session...".to_owned()));
    model.dirty = true;
    vec![UiEffect::Dispatch(UiIntent::ArchiveSession {
        request_id,
        session_id,
    })]
}

fn dispatch_unarchive(model: &mut Model, session_id: String) -> Vec<UiEffect> {
    if has_pending_lifecycle(model, &session_id) {
        return Vec::new();
    }
    let request_id = model.allocate_request();
    model.pending.insert(
        request_id,
        PendingKind::UnarchiveSession(session_id.clone()),
    );
    model.notice = Some(Notice::Info("Unarchiving session...".to_owned()));
    model.dirty = true;
    vec![UiEffect::Dispatch(UiIntent::UnarchiveSession {
        request_id,
        session_id,
    })]
}

/// Reverses the most recent committed archive or unarchive exactly once.
fn undo_last_lifecycle(model: &mut Model) -> Vec<UiEffect> {
    let Some(undoable) = model.undoable.take() else {
        return Vec::new();
    };
    model.notice = Some(Notice::Info(if undoable.archived {
        "Undoing archive...".to_owned()
    } else {
        "Undoing unarchive...".to_owned()
    }));
    model.dirty = true;
    if undoable.archived {
        dispatch_unarchive(model, undoable.session_id)
    } else {
        let request_id = model.allocate_request();
        model.pending.insert(
            request_id,
            PendingKind::ArchiveSession(undoable.session_id.clone()),
        );
        vec![UiEffect::Dispatch(UiIntent::ArchiveSession {
            request_id,
            session_id: undoable.session_id,
        })]
    }
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
    let _ = model.open_overlay(OverlayKind::Confirmation);
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
    let _ = model.close_overlay(OverlayKind::Confirmation);
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
    match model.overlay() {
        Some(OverlayKind::ProfileCredential) => {
            let Some(credential) = model.profile_center.credential.as_mut() else {
                return;
            };
            match credential.append_paste(text) {
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
        }
        Some(OverlayKind::SessionCredential) => {
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
        }
        Some(OverlayKind::ModelPicker) => {
            let flattened = editable_safe(text).replace('\n', " ");
            model.picker.query.push_str(&flattened);
            normalize_picker_selection(model);
            model.dirty = true;
        }
        Some(OverlayKind::CommandPalette) => {
            model
                .palette
                .query
                .push_str(&editable_safe(text).replace('\n', " "));
            normalize_palette_selection(model);
            model.dirty = true;
        }
        Some(
            OverlayKind::TranscriptSearch | OverlayKind::Permission | OverlayKind::Confirmation,
        ) => {}
        Some(OverlayKind::UserProfile) => {
            if let Some(editor) = model.user_profile.display_label_editor.as_mut() {
                let flattened = editable_safe(text).replace('\n', " ");
                let remaining = MAX_DISPLAY_LABEL_CHARS.saturating_sub(editor.chars().count());
                editor.push_str(&flattened.chars().take(remaining).collect::<String>());
                model.dirty = true;
            }
        }
        None if model.route() == Route::Profiles && model.profile_center.editor.is_some() => {
            let editor = model
                .profile_center
                .editor
                .as_mut()
                .expect("profile editor is open");
            if let Some(field) = profile_editor_field(editor) {
                let flattened = editable_safe(text).replace('\n', " ");
                let remaining = 2_048usize.saturating_sub(field.len());
                field.push_str(&flattened.chars().take(remaining).collect::<String>());
                model.dirty = true;
            }
        }
        None if model.route() == Route::Sessions && model.browser.renaming => {
            let flattened = editable_safe(text).replace('\n', " ");
            let remaining = 128usize.saturating_sub(model.browser.rename_buffer.chars().count());
            model
                .browser
                .rename_buffer
                .push_str(&flattened.chars().take(remaining).collect::<String>());
            model.dirty = true;
        }
        None if model.route() == Route::Settings
            && model.settings_workspace.display_label_editor.is_some() =>
        {
            if let Some(editor) = model.settings_workspace.display_label_editor.as_mut() {
                let flattened = editable_safe(text).replace('\n', " ");
                let remaining = MAX_DISPLAY_LABEL_CHARS.saturating_sub(editor.chars().count());
                let appended = flattened.chars().take(remaining).collect::<String>();
                let truncated = appended.chars().count() < flattened.chars().count();
                editor.push_str(&appended);
                if truncated {
                    model.notice = Some(Notice::Info(
                        "Display label limited to 64 characters".to_owned(),
                    ));
                }
                model.dirty = true;
            }
        }
        None if model.route() == Route::Chat
            && !has_pending_submission(model)
            && model.composer.editor.insert_str(editable_safe(text)) =>
        {
            model.notice = None;
            model.dirty = true;
        }
        None => {}
    }
}

/// Opens the transcript search bar and takes the keyboard.
fn open_search(model: &mut Model) {
    if !model.open_overlay(OverlayKind::TranscriptSearch) {
        return;
    }
    model.search.query.clear();
    model.search.matches.clear();
    model.search.current = None;
}

fn close_search(model: &mut Model) {
    model.search.query.clear();
    model.search.matches.clear();
    model.search.current = None;
    model.search_pinned_row = None;
    let _ = model.close_overlay(OverlayKind::TranscriptSearch);
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

fn composer_submit_behavior(model: &Model) -> ComposerSubmitBehavior {
    *model
        .settings()
        .local_profile
        .preferences()
        .composer_submit_behavior()
        .value()
}

fn insert_composer_newline(model: &mut Model) -> Vec<UiEffect> {
    if !has_pending_submission(model) && model.composer.editor.insert_str("\n") {
        model.history.reset_walk();
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
        close_active_overlay_state(model);
        model.picker.selected = session.selected_model.clone();
        model.focus = model.route().focus();
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
        close_active_overlay_state(model);
        let _ = model.open_overlay(OverlayKind::Permission);
    } else if model.overlay() == Some(OverlayKind::Permission) {
        model.permission_scroll = 0;
        let _ = model.close_overlay(OverlayKind::Permission);
    } else if session_changed
        && model.session.selected_model.is_none()
        && matches!(&*model.catalog, CatalogProjection::Ready { models, .. } if !models.is_empty())
    {
        navigate_to_route(model, Route::Chat);
        open_picker(model);
    }
    model.sync_retry_deadline();
    model.dirty = true;
}

fn apply_catalog(model: &mut Model, catalog: Arc<CatalogProjection>) {
    model.catalog = catalog;
    model.sync_catalog_retry_deadline();
    normalize_picker_selection(model);
    if model.route() == Route::Chat
        && model.overlay().is_none()
        && matches!(&*model.catalog, CatalogProjection::CredentialRequired)
    {
        open_credential(model);
    } else if model.route() == Route::Chat
        && model.overlay().is_none()
        && !selected_model_available(model)
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
                        model.credential.clear();
                        model.notice = Some(Notice::Info("New session created".to_owned()));
                    }
                    PendingKind::ConfigureCredential => {
                        model.notice = Some(Notice::Info("API key accepted".to_owned()));
                    }
                    PendingKind::UpsertProfile(profile) => {
                        model.profile_center.selected = Some(profile.id);
                        model.notice = Some(Notice::Info("Provider profile saved".to_owned()));
                    }
                    PendingKind::DuplicateProfile { destination, .. } => {
                        model.profile_center.selected = Some(destination);
                        model.notice = Some(Notice::Info(
                            "Profile duplicated without a credential".to_owned(),
                        ));
                    }
                    PendingKind::ActivateProfile(_) => {
                        model.notice = Some(Notice::Info("Active provider switched".to_owned()));
                    }
                    PendingKind::SaveProfileCredential(_) => {
                        model.notice = Some(Notice::Info(
                            "Credential saved in the operating-system vault".to_owned(),
                        ));
                    }
                    PendingKind::ReplaceProfileCredential(_) => {
                        model.notice = Some(Notice::Info("Stored credential replaced".to_owned()));
                    }
                    PendingKind::TestProfile(_) => {
                        model.notice = Some(Notice::Info(
                            "Provider connection test completed".to_owned(),
                        ));
                    }
                    PendingKind::SetProfileDefaultModel(_) => {
                        model.notice = Some(Notice::Info("Profile default model saved".to_owned()));
                    }
                    PendingKind::DisconnectProfile(_) => {
                        model.notice =
                            Some(Notice::Info("Stored credential disconnected".to_owned()));
                    }
                    PendingKind::UpdateLocalPreference(_) => {
                        model.notice = Some(Notice::Info(
                            "Local preference saved; waiting for the resolved settings projection"
                                .to_owned(),
                        ));
                    }
                    PendingKind::DeleteProfile(_) => {
                        model.notice = Some(Notice::Info("Provider profile deleted".to_owned()));
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
                    PendingKind::ArchiveSession(session_id) => {
                        model.undoable = Some(crate::model::UndoableLifecycle {
                            session_id,
                            archived: true,
                        });
                        model.notice =
                            Some(Notice::Info("Session archived - Ctrl+Z to undo".to_owned()));
                    }
                    PendingKind::UnarchiveSession(session_id) => {
                        model.undoable = Some(crate::model::UndoableLifecycle {
                            session_id,
                            archived: false,
                        });
                        model.notice = Some(Notice::Info(
                            "Session unarchived - Ctrl+Z to undo".to_owned(),
                        ));
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
                Some(PendingKind::UpsertProfile(profile)) => {
                    let mode = if model
                        .profiles
                        .profiles
                        .iter()
                        .any(|existing| existing.id == profile.id)
                    {
                        ProfileEditorMode::Edit
                    } else {
                        ProfileEditorMode::Create
                    };
                    model.profile_center.editor = Some(ProfileEditorState {
                        mode,
                        source_id: None,
                        field: if mode == ProfileEditorMode::Edit {
                            1
                        } else {
                            0
                        },
                        id: profile.id,
                        kind: profile.kind,
                        base_url: profile.base_url,
                        project: profile.project,
                        auth_header: profile.auth_header,
                    });
                }
                Some(PendingKind::DuplicateProfile {
                    source,
                    destination,
                }) => {
                    if let Some(profile) = model
                        .profiles
                        .profiles
                        .iter()
                        .find(|profile| profile.id == source)
                    {
                        model.profile_center.editor = Some(ProfileEditorState {
                            mode: ProfileEditorMode::Duplicate,
                            source_id: Some(source),
                            field: 0,
                            id: destination,
                            kind: profile.kind,
                            base_url: profile.base_url.clone(),
                            project: profile.project.clone(),
                            auth_header: profile.auth_header.clone(),
                        });
                    }
                }
                Some(PendingKind::SaveProfileCredential(profile_id)) => {
                    model.profile_center.selected = Some(profile_id.clone());
                    model.profile_center.credential = Some(ProfileCredentialEditor::new(
                        profile_id,
                        ProfileCredentialAction::Save,
                    ));
                }
                Some(PendingKind::ReplaceProfileCredential(profile_id)) => {
                    model.profile_center.selected = Some(profile_id.clone());
                    model.profile_center.credential = Some(ProfileCredentialEditor::new(
                        profile_id,
                        ProfileCredentialAction::Replace,
                    ));
                }
                Some(PendingKind::SelectModel(_)) => open_picker(model),
                Some(
                    PendingKind::ActivateProfile(_)
                    | PendingKind::TestProfile(_)
                    | PendingKind::SetProfileDefaultModel(_)
                    | PendingKind::DisconnectProfile(_)
                    | PendingKind::UpdateLocalPreference(_)
                    | PendingKind::DeleteProfile(_)
                    | PendingKind::RefreshCatalog
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
    if !model.open_overlay(OverlayKind::ModelPicker) {
        return;
    }
    normalize_picker_selection(model);
}

fn close_help(model: &mut Model) {
    model.help.scroll = 0;
    if !model.navigate_back() {
        navigate_to_route(model, Route::Chat);
    }
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
    model.credential.clear();
    let _ = model.open_overlay(OverlayKind::SessionCredential);
}

fn close_credential(model: &mut Model) {
    model.credential.clear();
    let _ = model.close_overlay(OverlayKind::SessionCredential);
}

fn close_picker(model: &mut Model) {
    model.picker.query.clear();
    let _ = model.close_overlay(OverlayKind::ModelPicker);
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
            | PendingKind::UpsertProfile(_)
            | PendingKind::DuplicateProfile { .. }
            | PendingKind::ActivateProfile(_)
            | PendingKind::SaveProfileCredential(_)
            | PendingKind::ReplaceProfileCredential(_)
            | PendingKind::TestProfile(_)
            | PendingKind::SetProfileDefaultModel(_)
            | PendingKind::DisconnectProfile(_)
            | PendingKind::UpdateLocalPreference(_)
            | PendingKind::DeleteProfile(_)
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
