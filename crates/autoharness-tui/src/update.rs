use std::sync::Arc;

use autoharness_domain::ErrorClass;
use autoharness_settings::{
    ColorMode, ComposerSubmitBehavior, Density, GlyphMode, Layout, PromptStatusDetail,
    TerminalTimestampStyle, ThemePreset,
};
use ratatui_textarea::{Input, Key};

use crate::model::{
    AttemptKey, COMMANDS, CatalogProjection, CodexLoginState, CommandEntry, Focus,
    LocalPreferenceChange, MODEL_THINKING_LEVELS, MemoryContent, MemoryDraftEditor,
    MemoryLifecycleMode, MemoryLifecycleState, MemoryPane, MemoryScopeFilter, MemoryStatusFilter,
    MemoryTargetSnapshot, MemoryWorkspaceFocus, Message, Model, ModelDefaultStep, MouseAction,
    Notice, OverlayKind, PROVIDER_CHOICES, PendingKind, ProfileCenterFocus,
    ProfileCredentialAction, ProfileCredentialEditor, ProfileEditorMode, ProfileEditorState,
    ProfilesProjection, ProviderChoice, ProviderKindLabel, ProviderProfileDraft, RetryPolicy,
    Route, SETTINGS_NAV_COUNT, SessionProjection, SessionsProjection, SettingsCategory,
    SettingsPreference, UiEffect, UiFailure, UiIntent, UiNotice,
};
use crate::text::{display_safe, editable_safe};

const MAX_DISPLAY_LABEL_CHARS: usize = 64;
/// Applies one input to local UI state and returns application-owned effects.
#[must_use]
pub fn update(model: &mut Model, message: Message) -> Vec<UiEffect> {
    match message {
        Message::Input(input) => {
            model.mark_activity();
            handle_input(model, input)
        }
        Message::Mouse(action) => {
            model.mark_activity();
            handle_mouse(model, action)
        }
        Message::TranscriptScroll(rows) => {
            model.mark_activity();
            if model.route() == Route::Chat && model.overlay().is_none() {
                if rows > 0 {
                    scroll_up(model, usize::from(rows.unsigned_abs()));
                } else if rows < 0 {
                    scroll_down(model, usize::from(rows.unsigned_abs()));
                }
            }
            Vec::new()
        }
        Message::Paste(text) => {
            model.mark_activity();
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
        Message::MemoryChanged(memory) => {
            model.apply_memory(memory);
            Vec::new()
        }
        Message::Notice(notice) => {
            apply_notice(model, notice);
            Vec::new()
        }
        Message::Tick(clock) => {
            let was_startup_active = model.startup_active();
            model.advance_clock(clock);
            if was_startup_active || model.startup_active() {
                model.dirty = true;
            }
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
            model.mark_activity();
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
    if let Some(overlay) = model.overlay() {
        let allowed = match overlay {
            OverlayKind::UserProfile => matches!(
                action,
                MouseAction::UserProfileSave | MouseAction::UserProfileCancel
            ),
            OverlayKind::Confirmation => {
                matches!(action, MouseAction::Confirm | MouseAction::Cancel)
            }
            OverlayKind::ModelPicker => {
                matches!(
                    action,
                    MouseAction::PickerSelect(_) | MouseAction::OverlayCancel
                )
            }
            OverlayKind::CommandPalette => {
                matches!(
                    action,
                    MouseAction::PaletteRun(_) | MouseAction::OverlayCancel
                )
            }
            OverlayKind::SessionCredential => {
                matches!(
                    action,
                    MouseAction::CredentialSubmit | MouseAction::CredentialCancel
                )
            }
            OverlayKind::ProfileCredential => matches!(
                action,
                MouseAction::ProfileCredentialSubmit | MouseAction::ProfileCredentialCancel
            ),
            OverlayKind::Permission => {
                matches!(
                    action,
                    MouseAction::PermissionAllow | MouseAction::PermissionDeny
                )
            }
            OverlayKind::TranscriptSearch => false,
            OverlayKind::MemoryLifecycle => matches!(
                action,
                MouseAction::MemoryActionSelect(_)
                    | MouseAction::MemoryLifecycleSubmit
                    | MouseAction::MemoryProposalReject
                    | MouseAction::MemoryLifecycleCancel
            ),
        };
        if !allowed {
            return Vec::new();
        }
    }
    match action {
        MouseAction::Route(route) => {
            navigate_to_route(model, route);
            Vec::new()
        }
        MouseAction::SettingsTab(tab) => {
            navigate_to_route(model, Route::Settings);
            model.settings_workspace.nav_selected = tab.min(SETTINGS_NAV_COUNT.saturating_sub(1));
            model.settings_workspace.nav_focus = true;
            model.settings_workspace.selected = 0;
            normalize_settings_selection(model);
            model.dirty = true;
            Vec::new()
        }
        MouseAction::FocusComposer => {
            if model.route() == Route::Chat && model.overlay().is_none() {
                model.focus = Focus::Composer;
                follow_tail(model);
            }
            Vec::new()
        }
        MouseAction::FocusTranscript => {
            if model.route() == Route::Chat && model.overlay().is_none() {
                model.transcript.follow_tail = false;
                model.dirty = true;
            }
            Vec::new()
        }
        MouseAction::ChatModels => {
            open_picker(model);
            Vec::new()
        }
        MouseAction::ChatRetry => retry_attempt(model),
        MouseAction::ChatFreshSession => create_session(model),
        MouseAction::SettingsRow(index) => {
            if model.route() == Route::Settings {
                move_settings_selection_to(model, index);
            }
            Vec::new()
        }
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
        MouseAction::SelectProviderChoice(index) => {
            if index < PROVIDER_CHOICES.len() {
                model.profile_center.choice_selected = index;
                model.profile_center.focus = ProfileCenterFocus::ProviderChoices;
                model.dirty = true;
            }
            Vec::new()
        }
        MouseAction::CodexLogin => begin_codex_login(model),
        MouseAction::CodexLoginCancel => cancel_codex_login(model),
        MouseAction::ProfileEditorSubmit => submit_profile_editor(model),
        MouseAction::ProfileEditorCancel => {
            close_profile_editor(model);
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
                model.profile_center.focus = ProfileCenterFocus::ConnectedProfiles;
                model.dirty = true;
            }
            Vec::new()
        }
        MouseAction::SessionOpen => open_selected_session(model),
        MouseAction::SessionRename => rename_selected_session(model),
        MouseAction::SessionArchive => toggle_archive_selected_session(model),
        MouseAction::SessionDelete => request_delete_selected_session(model),
        MouseAction::Confirm => confirm_mouse_action(model),
        MouseAction::Cancel => cancel_mouse_action(model),
        MouseAction::UserProfileSave => commit_user_profile(model),
        MouseAction::UserProfileCancel => {
            close_user_profile(model);
            Vec::new()
        }
        MouseAction::CredentialSubmit => submit_credential(model),
        MouseAction::CredentialCancel => {
            close_credential(model);
            Vec::new()
        }
        MouseAction::ProfileCredentialSubmit => submit_profile_credential(model),
        MouseAction::ProfileCredentialCancel => {
            model.profile_center.credential = None;
            let _ = model.close_overlay(OverlayKind::ProfileCredential);
            model.notice = None;
            model.dirty = true;
            Vec::new()
        }
        MouseAction::OverlayCancel => {
            close_active_overlay_state(model);
            model.dirty = true;
            Vec::new()
        }
        MouseAction::PermissionAllow => answer_permission(model, true),
        MouseAction::PermissionDeny => answer_permission(model, false),
        MouseAction::MemoryFocusSearch => {
            if model.route() == Route::Memory {
                model.memory_workspace.focus = MemoryWorkspaceFocus::Search;
                model.dirty = true;
            }
            Vec::new()
        }
        MouseAction::MemorySelect(memory_id) => {
            if model.route() == Route::Memory
                && model
                    .memory_entries()
                    .iter()
                    .any(|summary| summary.id() == memory_id)
            {
                model.memory_workspace.selected = Some(memory_id);
                model.memory_workspace.focus = MemoryWorkspaceFocus::List;
                model.memory_workspace.admission_selected = 0;
                model.dirty = true;
            }
            Vec::new()
        }
        MouseAction::MemorySelectAdmission(index) => {
            if model.route() == Route::Memory {
                let count = model
                    .selected_memory()
                    .and_then(|(_, detail)| detail)
                    .map_or(0, |detail| detail.admissions().len());
                if index < count {
                    model.memory_workspace.admission_selected = index;
                    model.memory_workspace.focus = MemoryWorkspaceFocus::Admissions;
                    model.memory_workspace.pane = MemoryPane::Admissions;
                    model.dirty = true;
                }
            }
            Vec::new()
        }
        MouseAction::MemoryCycleStatus => {
            if model.route() == Route::Memory {
                cycle_memory_status(model, 1);
            }
            Vec::new()
        }
        MouseAction::MemoryCycleScope => {
            if model.route() == Route::Memory {
                cycle_memory_scope(model, 1);
            }
            Vec::new()
        }
        MouseAction::MemoryOpen => {
            open_memory_detail(model);
            Vec::new()
        }
        MouseAction::MemoryBack => {
            memory_back(model);
            Vec::new()
        }
        MouseAction::MemoryAdmissions => {
            open_memory_admissions(model);
            Vec::new()
        }
        MouseAction::MemoryRemember => {
            open_memory_lifecycle(model, MemoryLifecycleMode::Remember);
            Vec::new()
        }
        MouseAction::MemoryRevise => {
            open_memory_lifecycle(model, MemoryLifecycleMode::Revise);
            Vec::new()
        }
        MouseAction::MemoryReview => {
            open_memory_lifecycle(model, MemoryLifecycleMode::Review);
            Vec::new()
        }
        MouseAction::MemoryActions => {
            open_memory_lifecycle(model, MemoryLifecycleMode::Actions);
            Vec::new()
        }
        MouseAction::MemoryRetract => {
            open_memory_lifecycle(model, MemoryLifecycleMode::Retract);
            Vec::new()
        }
        MouseAction::MemoryDelete => {
            open_memory_lifecycle(model, MemoryLifecycleMode::Delete);
            Vec::new()
        }
        MouseAction::MemoryExport => {
            open_memory_lifecycle(model, MemoryLifecycleMode::Export);
            Vec::new()
        }
        MouseAction::MemoryActionSelect(index) => {
            select_memory_lifecycle_action(model, index);
            Vec::new()
        }
        MouseAction::MemoryLifecycleSubmit => submit_memory_lifecycle(model, true),
        MouseAction::MemoryProposalReject => submit_memory_lifecycle(model, false),
        MouseAction::MemoryLifecycleCancel => {
            close_memory_lifecycle(model);
            Vec::new()
        }
        MouseAction::PickerSelect(selection) => {
            if model
                .catalog
                .models()
                .iter()
                .any(|summary| summary.selectable && summary.model == selection)
            {
                model.picker.selected = Some(selection);
                select_picker_model(model)
            } else {
                Vec::new()
            }
        }
        MouseAction::PaletteRun(command) => {
            if model
                .palette_entries()
                .iter()
                .any(|entry| entry.id == command)
            {
                close_palette(model);
                run_command_by_id(model, &command).unwrap_or_default()
            } else {
                Vec::new()
            }
        }
    }
}

fn confirm_mouse_action(model: &mut Model) -> Vec<UiEffect> {
    match model.route() {
        Route::Sessions if model.browser.confirming_archive.is_some() => {
            confirm_archive_selected_session(model)
        }
        Route::Sessions if model.browser.confirming_delete.is_some() => {
            confirm_delete_selected_session(model)
        }
        Route::Profiles => {
            if let Some(profile_id) = model.profile_center.confirming_disconnect.take() {
                dispatch_disconnect_profile(model, profile_id)
            } else if let Some(profile_id) = model.profile_center.confirming_delete.take() {
                dispatch_delete_profile(model, profile_id)
            } else {
                Vec::new()
            }
        }
        Route::Sessions => Vec::new(),
        Route::Chat | Route::Settings | Route::Help | Route::Memory => Vec::new(),
    }
}

fn cancel_mouse_action(model: &mut Model) -> Vec<UiEffect> {
    model.browser.confirming_archive = None;
    model.browser.confirming_delete = None;
    model.profile_center.confirming_disconnect = None;
    model.profile_center.confirming_delete = None;
    let _ = model.close_overlay(OverlayKind::Confirmation);
    model.notice = None;
    model.dirty = true;
    Vec::new()
}

fn handle_input(model: &mut Model, input: Input) -> Vec<UiEffect> {
    if model.overlay() == Some(OverlayKind::Permission) {
        return handle_permission_input(model, input);
    }
    if model.overlay() == Some(OverlayKind::MemoryLifecycle) {
        return handle_memory_lifecycle_input(model, input);
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
    ) && model.route() != Route::Settings
    {
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
        navigate_to_route(model, Route::Settings);
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
            OverlayKind::MemoryLifecycle => handle_memory_lifecycle_input(model, input),
            OverlayKind::Confirmation => match model.route() {
                Route::Sessions => handle_browser_input(model, input),
                Route::Profiles => handle_profile_input(model, input),
                Route::Settings if model.settings_workspace.nav_selected == 1 => {
                    handle_profile_input(model, input)
                }
                Route::Chat | Route::Settings | Route::Help | Route::Memory => Vec::new(),
            },
        };
    }
    match model.route() {
        Route::Chat => handle_chat_input(model, input),
        Route::Sessions => handle_browser_input(model, input),
        Route::Profiles => handle_profile_input(model, input),
        Route::Settings => handle_settings_input(model, input),
        Route::Help => handle_help_input(model, input),
        Route::Memory => handle_memory_input(model, input),
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
        Key::Char('6') => Some(Route::Memory),
        _ => None,
    }
}

fn handle_memory_input(model: &mut Model, input: Input) -> Vec<UiEffect> {
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
    if input.alt {
        let mode = match input.key {
            Key::Char('n' | 'N') => Some(MemoryLifecycleMode::Remember),
            Key::Char('e' | 'E') => Some(MemoryLifecycleMode::Revise),
            Key::Char('v' | 'V') => Some(MemoryLifecycleMode::Review),
            Key::Char('a' | 'A') => Some(MemoryLifecycleMode::Actions),
            Key::Char('x' | 'X') => Some(MemoryLifecycleMode::Retract),
            Key::Char('d' | 'D') => Some(MemoryLifecycleMode::Delete),
            Key::Char('s' | 'S') => Some(MemoryLifecycleMode::Export),
            _ => None,
        };
        if let Some(mode) = mode {
            open_memory_lifecycle(model, mode);
            return Vec::new();
        }
    }
    match input {
        Input {
            key: Key::Char('/'),
            ctrl: false,
            alt: false,
            ..
        } => {
            model.memory_workspace.focus = MemoryWorkspaceFocus::Search;
            model.dirty = true;
        }
        Input {
            key: Key::Tab,
            shift: true,
            ..
        } => cycle_memory_focus(model, -1),
        Input { key: Key::Tab, .. } => cycle_memory_focus(model, 1),
        Input { key: Key::Esc, .. } => {
            if model.memory_workspace.focus == MemoryWorkspaceFocus::Search
                && !model.memory_workspace.query.is_empty()
            {
                model.memory_workspace.query.clear();
                model.sync_memory_selection();
                model.dirty = true;
            } else if model.memory_workspace.pane != MemoryPane::List {
                memory_back(model);
            } else {
                navigate_to_route(model, Route::Chat);
            }
        }
        Input {
            key: Key::Enter, ..
        } => match model.memory_workspace.focus {
            MemoryWorkspaceFocus::Status => cycle_memory_status(model, 1),
            MemoryWorkspaceFocus::Scope => cycle_memory_scope(model, 1),
            MemoryWorkspaceFocus::Admissions => {
                model.memory_workspace.pane = MemoryPane::Detail;
                model.memory_workspace.focus = MemoryWorkspaceFocus::Detail;
                model.dirty = true;
            }
            MemoryWorkspaceFocus::Detail => open_memory_admissions(model),
            MemoryWorkspaceFocus::Search | MemoryWorkspaceFocus::List => open_memory_detail(model),
        },
        Input { key: Key::Up, .. } => move_memory_selection(model, -1),
        Input { key: Key::Down, .. } => move_memory_selection(model, 1),
        Input { key: Key::Left, .. } => match model.memory_workspace.focus {
            MemoryWorkspaceFocus::Status => cycle_memory_status(model, -1),
            MemoryWorkspaceFocus::Scope => cycle_memory_scope(model, -1),
            _ => {}
        },
        Input {
            key: Key::Right, ..
        } => match model.memory_workspace.focus {
            MemoryWorkspaceFocus::Status => cycle_memory_status(model, 1),
            MemoryWorkspaceFocus::Scope => cycle_memory_scope(model, 1),
            _ => {}
        },
        Input {
            key: Key::Backspace,
            ..
        } if model.memory_workspace.focus == MemoryWorkspaceFocus::Search => {
            model.memory_workspace.query.pop();
            model.sync_memory_selection();
            model.dirty = true;
        }
        Input {
            key: Key::Char(character),
            ctrl: false,
            alt: false,
            ..
        } if !character.is_control() => {
            model.memory_workspace.focus = MemoryWorkspaceFocus::Search;
            model.memory_workspace.query.push(character);
            model.sync_memory_selection();
            model.dirty = true;
        }
        _ => {}
    }
    Vec::new()
}

fn cycle_memory_focus(model: &mut Model, direction: i32) {
    const FOCI: [MemoryWorkspaceFocus; 6] = [
        MemoryWorkspaceFocus::Search,
        MemoryWorkspaceFocus::Status,
        MemoryWorkspaceFocus::Scope,
        MemoryWorkspaceFocus::List,
        MemoryWorkspaceFocus::Detail,
        MemoryWorkspaceFocus::Admissions,
    ];
    let current = FOCI
        .iter()
        .position(|focus| *focus == model.memory_workspace.focus)
        .unwrap_or_default();
    let next = (i32::try_from(current).unwrap_or(0) + direction)
        .rem_euclid(i32::try_from(FOCI.len()).unwrap_or(1));
    model.memory_workspace.focus = FOCI[usize::try_from(next).unwrap_or_default()];
    model.memory_workspace.pane = match model.memory_workspace.focus {
        MemoryWorkspaceFocus::Admissions => MemoryPane::Admissions,
        MemoryWorkspaceFocus::Detail => MemoryPane::Detail,
        _ => model.memory_workspace.pane,
    };
    model.dirty = true;
}

fn cycle_memory_status(model: &mut Model, direction: i32) {
    let current = MemoryStatusFilter::ALL
        .iter()
        .position(|filter| *filter == model.memory_workspace.status)
        .unwrap_or_default();
    let next = (i32::try_from(current).unwrap_or(0) + direction)
        .rem_euclid(i32::try_from(MemoryStatusFilter::ALL.len()).unwrap_or(1));
    model.memory_workspace.status =
        MemoryStatusFilter::ALL[usize::try_from(next).unwrap_or_default()];
    model.memory_workspace.focus = MemoryWorkspaceFocus::Status;
    model.sync_memory_selection();
    model.dirty = true;
}

fn cycle_memory_scope(model: &mut Model, direction: i32) {
    let current = MemoryScopeFilter::ALL
        .iter()
        .position(|filter| *filter == model.memory_workspace.scope)
        .unwrap_or_default();
    let next = (i32::try_from(current).unwrap_or(0) + direction)
        .rem_euclid(i32::try_from(MemoryScopeFilter::ALL.len()).unwrap_or(1));
    model.memory_workspace.scope =
        MemoryScopeFilter::ALL[usize::try_from(next).unwrap_or_default()];
    model.memory_workspace.focus = MemoryWorkspaceFocus::Scope;
    model.sync_memory_selection();
    model.dirty = true;
}

fn move_memory_selection(model: &mut Model, direction: i32) {
    if model.memory_workspace.focus == MemoryWorkspaceFocus::Admissions {
        let count = model
            .selected_memory()
            .and_then(|(_, detail)| detail)
            .map_or(0, |detail| detail.admissions().len());
        if count > 0 {
            let current = i32::try_from(model.memory_workspace.admission_selected).unwrap_or(0);
            let last = i32::try_from(count.saturating_sub(1)).unwrap_or(0);
            model.memory_workspace.admission_selected =
                usize::try_from((current + direction).clamp(0, last)).unwrap_or_default();
            model.dirty = true;
        }
        return;
    }
    model.memory_workspace.focus = MemoryWorkspaceFocus::List;
    let entries = model
        .memory_entries()
        .iter()
        .map(|summary| summary.id().to_owned())
        .collect::<Vec<_>>();
    if entries.is_empty() {
        model.memory_workspace.selected = None;
        return;
    }
    let current = model
        .memory_workspace
        .selected
        .as_ref()
        .and_then(|selected| entries.iter().position(|candidate| candidate == selected))
        .unwrap_or_default();
    let last = i32::try_from(entries.len().saturating_sub(1)).unwrap_or(0);
    let next = (i32::try_from(current).unwrap_or(0) + direction).clamp(0, last);
    model.memory_workspace.selected = entries
        .get(usize::try_from(next).unwrap_or_default())
        .cloned();
    model.memory_workspace.admission_selected = 0;
    model.dirty = true;
}

fn open_memory_detail(model: &mut Model) {
    if model.route() == Route::Memory && model.memory_workspace.selected.is_some() {
        model.memory_workspace.pane = MemoryPane::Detail;
        model.memory_workspace.focus = MemoryWorkspaceFocus::Detail;
        model.dirty = true;
    }
}

fn open_memory_admissions(model: &mut Model) {
    if model.route() == Route::Memory && model.memory_workspace.selected.is_some() {
        model.memory_workspace.pane = MemoryPane::Admissions;
        model.memory_workspace.focus = MemoryWorkspaceFocus::Admissions;
        model.dirty = true;
    }
}

fn memory_back(model: &mut Model) {
    match model.memory_workspace.pane {
        MemoryPane::Admissions => {
            model.memory_workspace.pane = MemoryPane::Detail;
            model.memory_workspace.focus = MemoryWorkspaceFocus::Detail;
        }
        MemoryPane::Detail | MemoryPane::List => {
            model.memory_workspace.pane = MemoryPane::List;
            model.memory_workspace.focus = MemoryWorkspaceFocus::List;
        }
    }
    model.dirty = true;
}

fn open_memory_lifecycle(model: &mut Model, mode: MemoryLifecycleMode) {
    if model.route() != Route::Memory || model.overlay().is_some() {
        return;
    }
    let target = if mode == MemoryLifecycleMode::Remember {
        None
    } else {
        let Some((summary, Some(detail))) = model.selected_memory() else {
            model.notice = Some(Notice::Info(
                "Select a memory with loaded revision detail first".to_owned(),
            ));
            model.dirty = true;
            return;
        };
        let content = detail
            .has_content()
            .then(|| MemoryContent::new(detail.content()))
            .transpose()
            .ok()
            .flatten();
        Some(MemoryTargetSnapshot {
            memory_id: summary.id().to_owned(),
            status: summary.status(),
            scope: summary.scope(),
            revision: detail.revision(),
            content,
            source: detail.source().to_owned(),
            trust: detail.trust(),
            revision_context: detail.revision_context().cloned(),
        })
    };
    if mode != MemoryLifecycleMode::Remember && mode != MemoryLifecycleMode::Actions {
        let available = model.memory_actions();
        if !available.contains(&mode) {
            model.notice = Some(Notice::Info(
                match mode {
                    MemoryLifecycleMode::Review => {
                        "This row is not a reviewable proposal with exact revision metadata"
                    }
                    MemoryLifecycleMode::Revise
                    | MemoryLifecycleMode::Retract
                    | MemoryLifecycleMode::Delete => {
                        "Exact lifecycle metadata is not loaded for this action"
                    }
                    MemoryLifecycleMode::Export => {
                        "Exact revision content is not loaded for export"
                    }
                    MemoryLifecycleMode::Remember | MemoryLifecycleMode::Actions => {
                        "This memory action is unavailable"
                    }
                }
                .to_owned(),
            ));
            model.dirty = true;
            return;
        }
    }
    if mode == MemoryLifecycleMode::Actions && model.memory_actions().is_empty() {
        model.notice = Some(Notice::Info(
            "No lifecycle actions are available for this row".to_owned(),
        ));
        model.dirty = true;
        return;
    }
    let editor = match mode {
        MemoryLifecycleMode::Remember => Some(MemoryDraftEditor::default()),
        MemoryLifecycleMode::Revise => target
            .as_ref()
            .and_then(|target| target.content.as_ref())
            .map(MemoryDraftEditor::from_content),
        MemoryLifecycleMode::Review
        | MemoryLifecycleMode::Actions
        | MemoryLifecycleMode::Retract
        | MemoryLifecycleMode::Delete
        | MemoryLifecycleMode::Export => None,
    };
    if !model.open_overlay(OverlayKind::MemoryLifecycle) {
        return;
    }
    model.memory_lifecycle = Some(MemoryLifecycleState {
        mode,
        target,
        editor,
        action_selected: 0,
        pending_request: None,
        scroll: 0,
    });
    model.notice = None;
    model.dirty = true;
}

fn close_memory_lifecycle(model: &mut Model) {
    if model.memory_lifecycle_pending() {
        return;
    }
    model.memory_lifecycle = None;
    let _ = model.close_overlay(OverlayKind::MemoryLifecycle);
    model.notice = None;
    model.dirty = true;
}

fn select_memory_lifecycle_action(model: &mut Model, index: usize) {
    let action_count = model.memory_actions().len();
    let Some(state) = model.memory_lifecycle.as_mut() else {
        return;
    };
    if state.mode == MemoryLifecycleMode::Actions && index < action_count {
        state.action_selected = index;
        model.dirty = true;
    }
}

fn activate_selected_memory_action(model: &mut Model) {
    let actions = model.memory_actions();
    let Some(state) = model.memory_lifecycle.as_mut() else {
        return;
    };
    if state.mode != MemoryLifecycleMode::Actions {
        return;
    }
    let Some(mode) = actions.get(state.action_selected).copied() else {
        return;
    };
    state.mode = mode;
    state.scroll = 0;
    state.editor = (mode == MemoryLifecycleMode::Revise)
        .then(|| {
            state
                .target
                .as_ref()
                .and_then(|target| target.content.as_ref())
                .map(MemoryDraftEditor::from_content)
        })
        .flatten();
    model.dirty = true;
}

fn handle_memory_lifecycle_input(model: &mut Model, input: Input) -> Vec<UiEffect> {
    let Some(state) = model.memory_lifecycle.as_ref() else {
        let _ = model.close_overlay(OverlayKind::MemoryLifecycle);
        return Vec::new();
    };
    if state.pending_request.is_some() {
        return Vec::new();
    }
    let mode = state.mode;
    match mode {
        MemoryLifecycleMode::Remember | MemoryLifecycleMode::Revise => match input {
            Input {
                key: Key::Char('s' | 'S'),
                ctrl: true,
                ..
            } => submit_memory_lifecycle(model, true),
            Input { key: Key::Esc, .. } => {
                close_memory_lifecycle(model);
                Vec::new()
            }
            Input {
                key: Key::Enter, ..
            } => {
                edit_memory_draft(model, Some('\n'));
                Vec::new()
            }
            Input {
                key: Key::Backspace,
                ..
            } => {
                if let Some(editor) = model
                    .memory_lifecycle
                    .as_mut()
                    .and_then(|state| state.editor.as_mut())
                {
                    editor.pop();
                    model.notice = None;
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
                edit_memory_draft(model, Some(character));
                Vec::new()
            }
            _ => Vec::new(),
        },
        MemoryLifecycleMode::Actions => match input {
            Input { key: Key::Up, .. } => {
                move_memory_action_selection(model, -1);
                Vec::new()
            }
            Input { key: Key::Down, .. } => {
                move_memory_action_selection(model, 1);
                Vec::new()
            }
            Input {
                key: Key::Enter, ..
            } => {
                activate_selected_memory_action(model);
                Vec::new()
            }
            Input { key: Key::Esc, .. } => {
                close_memory_lifecycle(model);
                Vec::new()
            }
            _ => Vec::new(),
        },
        MemoryLifecycleMode::Review => match input {
            Input { key: Key::Up, .. } => {
                scroll_memory_lifecycle(model, -1);
                Vec::new()
            }
            Input { key: Key::Down, .. } => {
                scroll_memory_lifecycle(model, 1);
                Vec::new()
            }
            Input {
                key: Key::Char('a' | 'A') | Key::Enter,
                ..
            } => submit_memory_lifecycle(model, true),
            Input {
                key: Key::Char('r' | 'R'),
                ..
            } => submit_memory_lifecycle(model, false),
            Input { key: Key::Esc, .. } => {
                close_memory_lifecycle(model);
                Vec::new()
            }
            _ => Vec::new(),
        },
        MemoryLifecycleMode::Retract
        | MemoryLifecycleMode::Delete
        | MemoryLifecycleMode::Export => match input {
            Input { key: Key::Up, .. } => {
                scroll_memory_lifecycle(model, -1);
                Vec::new()
            }
            Input { key: Key::Down, .. } => {
                scroll_memory_lifecycle(model, 1);
                Vec::new()
            }
            Input {
                key: Key::Char('y' | 'Y') | Key::Enter,
                ..
            } => submit_memory_lifecycle(model, true),
            Input {
                key: Key::Char('n' | 'N') | Key::Esc,
                ..
            } => {
                close_memory_lifecycle(model);
                Vec::new()
            }
            _ => Vec::new(),
        },
    }
}

fn scroll_memory_lifecycle(model: &mut Model, direction: i32) {
    let Some(state) = model.memory_lifecycle.as_mut() else {
        return;
    };
    if direction < 0 {
        state.scroll = state.scroll.saturating_sub(1);
    } else {
        state.scroll = state.scroll.saturating_add(1);
    }
    model.dirty = true;
}

fn edit_memory_draft(model: &mut Model, character: Option<char>) {
    let result = model
        .memory_lifecycle
        .as_mut()
        .and_then(|state| state.editor.as_mut())
        .map_or(Ok(()), |editor| {
            character.map_or(Ok(()), |character| editor.append_character(character))
        });
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
}

fn move_memory_action_selection(model: &mut Model, direction: i32) {
    let count = model.memory_actions().len();
    let Some(state) = model.memory_lifecycle.as_mut() else {
        return;
    };
    if count == 0 {
        return;
    }
    let last = i32::try_from(count.saturating_sub(1)).unwrap_or_default();
    let current = i32::try_from(state.action_selected).unwrap_or_default();
    state.action_selected = usize::try_from((current + direction).clamp(0, last)).unwrap_or(0);
    model.dirty = true;
}

fn submit_memory_lifecycle(model: &mut Model, affirmative: bool) -> Vec<UiEffect> {
    let Some(state) = model.memory_lifecycle.as_ref() else {
        return Vec::new();
    };
    if state.pending_request.is_some() {
        return Vec::new();
    }
    if state.mode == MemoryLifecycleMode::Actions {
        activate_selected_memory_action(model);
        return Vec::new();
    }
    let mode = state.mode;
    let target = state.target.clone();
    let content = state
        .editor
        .as_ref()
        .map(MemoryDraftEditor::content)
        .transpose();
    let content = match content {
        Ok(content) => content,
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
    let request_id = model.allocate_request();
    let (pending, intent, notice) = match mode {
        MemoryLifecycleMode::Remember => {
            let Some(content) = content else {
                return Vec::new();
            };
            (
                PendingKind::RememberMemory(content.clone()),
                UiIntent::RememberMemory {
                    request_id,
                    content,
                },
                "Saving memory...",
            )
        }
        MemoryLifecycleMode::Revise => {
            let Some(target) = target else {
                return Vec::new();
            };
            let Some(context) = target.revision_context.as_ref() else {
                return Vec::new();
            };
            let Some(content) = content else {
                return Vec::new();
            };
            (
                PendingKind::ReviseMemory {
                    memory_id: target.memory_id.clone(),
                    content: content.clone(),
                },
                UiIntent::ReviseMemory {
                    request_id,
                    memory_id: target.memory_id,
                    expected_last_sequence: context.expected_last_sequence(),
                    content,
                },
                "Saving correction...",
            )
        }
        MemoryLifecycleMode::Review => {
            let Some(target) = target else {
                return Vec::new();
            };
            let Some(context) = target.revision_context.as_ref() else {
                return Vec::new();
            };
            let Some(proposal_revision_id) = context.proposal_revision_id().map(str::to_owned)
            else {
                return Vec::new();
            };
            if affirmative {
                (
                    PendingKind::ApproveMemoryProposal(target.memory_id.clone()),
                    UiIntent::ApproveMemoryProposal {
                        request_id,
                        memory_id: target.memory_id,
                        expected_last_sequence: context.expected_last_sequence(),
                        proposal_revision_id,
                    },
                    "Approving exact proposal...",
                )
            } else {
                (
                    PendingKind::RejectMemoryProposal(target.memory_id.clone()),
                    UiIntent::RejectMemoryProposal {
                        request_id,
                        memory_id: target.memory_id,
                        expected_last_sequence: context.expected_last_sequence(),
                        proposal_revision_id,
                    },
                    "Rejecting exact proposal...",
                )
            }
        }
        MemoryLifecycleMode::Retract => {
            let Some(target) = target else {
                return Vec::new();
            };
            let Some(context) = target.revision_context.as_ref() else {
                return Vec::new();
            };
            (
                PendingKind::RetractMemory(target.memory_id.clone()),
                UiIntent::RetractMemory {
                    request_id,
                    memory_id: target.memory_id,
                    expected_last_sequence: context.expected_last_sequence(),
                    revision_id: context.revision_id().to_owned(),
                },
                "Retracting future admission...",
            )
        }
        MemoryLifecycleMode::Delete => {
            let Some(target) = target else {
                return Vec::new();
            };
            let Some(context) = target.revision_context.as_ref() else {
                return Vec::new();
            };
            (
                PendingKind::DeleteMemory(target.memory_id.clone()),
                UiIntent::DeleteMemory {
                    request_id,
                    memory_id: target.memory_id,
                    expected_last_sequence: context.expected_last_sequence(),
                },
                "Recording logical deletion...",
            )
        }
        MemoryLifecycleMode::Export => {
            let Some(target) = target else {
                return Vec::new();
            };
            (
                PendingKind::ExportMemory(target.memory_id.clone()),
                UiIntent::ExportMemory {
                    request_id,
                    memory_id: target.memory_id,
                },
                "Exporting memory...",
            )
        }
        MemoryLifecycleMode::Actions => return Vec::new(),
    };
    model.pending.insert(request_id, pending);
    if let Some(state) = model.memory_lifecycle.as_mut() {
        state.pending_request = Some(request_id);
    }
    model.notice = Some(Notice::Info(notice.to_owned()));
    model.dirty = true;
    vec![UiEffect::Dispatch(intent)]
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
            key: Key::PageUp, ..
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
            key: Key::PageDown, ..
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
                && model.composer.is_blank()
                && matches!(
                    input,
                    Input {
                        key: Key::Char('/'),
                        ctrl: false,
                        alt: false,
                        ..
                    }
                )
            {
                open_palette(model);
                return Vec::new();
            }
            if !has_pending_submission(model)
                && let Some(effects) = maybe_slash_command(model, &input)
            {
                return effects;
            }
            if !has_pending_submission(model) && input_composer(&mut model.composer.editor, input) {
                model.history.reset_walk();
                model.notice = None;
                follow_tail(model);
            }
            Vec::new()
        }
    }
}

fn handle_settings_input(model: &mut Model, input: Input) -> Vec<UiEffect> {
    if model.settings_workspace.search_active {
        return handle_settings_search_input(model, input);
    }
    if model.settings_workspace.choice_picker_open {
        return handle_settings_choice_picker_input(model, input);
    }
    if model.settings_workspace.detail_open {
        if matches!(input, Input { key: Key::Esc, .. }) {
            model.settings_workspace.detail_open = false;
            model.dirty = true;
            return Vec::new();
        }
        return handle_model_defaults_input(model, input);
    }
    match input {
        Input { key: Key::Esc, .. } => {
            if model.settings_workspace.display_label_editor.is_some() {
                model.settings_workspace.display_label_editor = None;
                model.dirty = true;
            } else if model.settings_workspace.nav_focus {
                navigate_to_route(model, Route::Chat);
            } else {
                model.settings_workspace.nav_focus = true;
                model.dirty = true;
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
        Input {
            key: Key::Char('f' | 'F'),
            ctrl: true,
            ..
        } => {
            model.settings_workspace.search_query.clear();
            model.settings_workspace.search_selected = 0;
            model.settings_workspace.search_active = true;
            model.dirty = true;
            Vec::new()
        }
        Input { key: Key::Tab, .. } => {
            model.settings_workspace.nav_focus = !model.settings_workspace.nav_focus;
            normalize_settings_selection(model);
            model.dirty = true;
            Vec::new()
        }
        Input {
            key: Key::Enter, ..
        } if model.settings_workspace.nav_focus => activate_settings_nav(model),
        Input { key: Key::Up, .. } if model.settings_workspace.nav_focus => {
            move_settings_nav(model, -1);
            Vec::new()
        }
        Input { key: Key::Down, .. } if model.settings_workspace.nav_focus => {
            move_settings_nav(model, 1);
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
            model.settings_workspace.nav_focus = false;
            move_settings_selection(model, -3);
            Vec::new()
        }
        Input {
            key: Key::PageDown, ..
        } => {
            model.settings_workspace.nav_focus = false;
            move_settings_selection(model, 3);
            Vec::new()
        }
        Input { key: Key::Home, .. } => {
            move_settings_selection_to(model, first_editable_settings_row(model));
            Vec::new()
        }
        Input { key: Key::End, .. } => {
            move_settings_selection_to(model, last_editable_settings_row(model));
            Vec::new()
        }
        Input {
            key: Key::Left | Key::Right,
            ..
        } if model.settings_workspace.nav_focus => Vec::new(),
        Input { key: Key::Left, .. } => change_selected_preference(model, -1),
        Input {
            key: Key::Right, ..
        } => change_selected_preference(model, 1),
        Input {
            key: Key::Enter | Key::Char(' '),
            ..
        } => activate_selected_setting(model),
        Input {
            key: Key::Backspace,
            shift: true,
            ..
        } => default_selected_preference(model),
        Input {
            key: Key::Backspace,
            ..
        } => reset_selected_preference(model),
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

fn handle_settings_search_input(model: &mut Model, input: Input) -> Vec<UiEffect> {
    match input {
        Input { key: Key::Esc, .. } => {
            model.settings_workspace.search_active = false;
            model.settings_workspace.search_query.clear();
            model.settings_workspace.search_selected = 0;
            model.dirty = true;
        }
        Input {
            key: Key::Enter, ..
        } => {
            if let Some((category, row, preference)) = model
                .settings_search_results()
                .get(model.settings_workspace.search_selected)
                .copied()
            {
                model.settings_workspace.nav_selected = SettingsCategory::ALL
                    .iter()
                    .position(|candidate| *candidate == category)
                    .unwrap_or_default();
                model.settings_workspace.selected = row;
                model.settings_workspace.nav_focus = !preference.editable();
                model.settings_workspace.search_active = false;
                model.settings_workspace.search_query.clear();
                normalize_settings_selection(model);
                model.dirty = true;
            }
        }
        Input { key: Key::Up, .. }
        | Input {
            key: Key::Tab,
            shift: true,
            ..
        } => {
            model.settings_workspace.search_selected =
                model.settings_workspace.search_selected.saturating_sub(1);
            model.dirty = true;
        }
        Input { key: Key::Down, .. } | Input { key: Key::Tab, .. } => {
            let last = model.settings_search_results().len().saturating_sub(1);
            model.settings_workspace.search_selected = model
                .settings_workspace
                .search_selected
                .saturating_add(1)
                .min(last);
            model.dirty = true;
        }
        Input {
            key: Key::Backspace,
            ..
        } => {
            model.settings_workspace.search_query.pop();
            model.settings_workspace.search_selected = 0;
            model.dirty = true;
        }
        Input {
            key: Key::Char(character),
            ctrl: false,
            alt: false,
            ..
        } if !character.is_control() => {
            model.settings_workspace.search_query.push(character);
            model.settings_workspace.search_selected = 0;
            model.dirty = true;
        }
        _ => {}
    }
    Vec::new()
}

fn handle_settings_choice_picker_input(model: &mut Model, input: Input) -> Vec<UiEffect> {
    match input {
        Input { key: Key::Esc, .. } => {
            model.settings_workspace.choice_picker_open = false;
            model.dirty = true;
            Vec::new()
        }
        Input { key: Key::Up, .. } => {
            model.settings_workspace.choice_picker_selected = model
                .settings_workspace
                .choice_picker_selected
                .saturating_sub(1);
            model.dirty = true;
            Vec::new()
        }
        Input { key: Key::Down, .. } => {
            model.settings_workspace.choice_picker_selected = model
                .settings_workspace
                .choice_picker_selected
                .saturating_add(1)
                .min(8);
            model.dirty = true;
            Vec::new()
        }
        Input {
            key: Key::Enter, ..
        } => {
            let value = [
                ThemePreset::System,
                ThemePreset::Light,
                ThemePreset::Dark,
                ThemePreset::Aurora,
                ThemePreset::Ember,
                ThemePreset::Midnight,
                ThemePreset::Ocean,
                ThemePreset::Forest,
                ThemePreset::Rose,
            ][model.settings_workspace.choice_picker_selected.min(8)];
            model.settings_workspace.choice_picker_open = false;
            dispatch_local_preference(model, LocalPreferenceChange::ThemePreset(Some(value)))
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
fn move_settings_nav(model: &mut Model, direction: isize) {
    let current = model.settings_workspace.nav_selected;
    let last = SETTINGS_NAV_COUNT.saturating_sub(1);
    model.settings_workspace.nav_selected = current.saturating_add_signed(direction).min(last);
    model.settings_workspace.selected = 0;
    model.settings_workspace.display_label_editor = None;
    model.settings_workspace.detail_open = false;
    normalize_settings_selection(model);
    model.dirty = true;
}

fn activate_settings_nav(model: &mut Model) -> Vec<UiEffect> {
    model.settings_workspace.nav_focus = false;
    normalize_settings_selection(model);
    model.notice = None;
    model.dirty = true;
    Vec::new()
}
fn selected_settings_preference(model: &Model) -> SettingsPreference {
    let category = SettingsCategory::at(model.settings_workspace.nav_selected);
    SettingsPreference::rows(category)
        .get(model.settings_workspace.selected)
        .copied()
        .or_else(|| {
            SettingsPreference::rows(category)
                .iter()
                .copied()
                .find(|row| row.editable())
        })
        .unwrap_or(SettingsPreference::ThemePreset)
}

fn move_settings_selection(model: &mut Model, direction: isize) {
    let category = SettingsCategory::at(model.settings_workspace.nav_selected);
    let rows = SettingsPreference::rows(category);
    if rows.is_empty() {
        return;
    }
    let editable = rows
        .iter()
        .enumerate()
        .filter_map(|(index, row)| row.editable().then_some(index))
        .collect::<Vec<_>>();
    let Some(position) = editable
        .iter()
        .position(|index| *index == model.settings_workspace.selected)
    else {
        normalize_settings_selection(model);
        return;
    };
    let last = editable.len().saturating_sub(1);
    let next = position.saturating_add_signed(direction).min(last);
    move_settings_selection_to(model, editable[next]);
}
fn move_settings_selection_to(model: &mut Model, selected: usize) {
    let category = SettingsCategory::at(model.settings_workspace.nav_selected);
    let rows = SettingsPreference::rows(category);
    let last = rows.len().saturating_sub(1);
    model.settings_workspace.nav_focus = false;
    model.settings_workspace.selected = selected.min(last);
    normalize_settings_selection(model);
    model.settings_workspace.scroll = 0;
    model.settings_workspace.display_label_editor = None;
    model.dirty = true;
}

fn normalize_settings_selection(model: &mut Model) {
    let category = SettingsCategory::at(model.settings_workspace.nav_selected);
    let rows = SettingsPreference::rows(category);
    if rows
        .get(model.settings_workspace.selected)
        .is_some_and(|row| row.editable())
    {
        return;
    }
    model.settings_workspace.selected = rows
        .iter()
        .position(|row| row.editable())
        .unwrap_or_default();
}

fn first_editable_settings_row(model: &Model) -> usize {
    let category = SettingsCategory::at(model.settings_workspace.nav_selected);
    SettingsPreference::rows(category)
        .iter()
        .position(|row| row.editable())
        .unwrap_or_default()
}

fn last_editable_settings_row(model: &Model) -> usize {
    let category = SettingsCategory::at(model.settings_workspace.nav_selected);
    SettingsPreference::rows(category)
        .iter()
        .rposition(|row| row.editable())
        .unwrap_or_default()
}

fn activate_selected_setting(model: &mut Model) -> Vec<UiEffect> {
    match selected_settings_preference(model) {
        SettingsPreference::ThemePreset => {
            let current = *model
                .settings()
                .local_profile
                .preferences()
                .theme_preset()
                .value();
            model.settings_workspace.choice_picker_selected = [
                ThemePreset::System,
                ThemePreset::Light,
                ThemePreset::Dark,
                ThemePreset::Aurora,
                ThemePreset::Ember,
                ThemePreset::Midnight,
                ThemePreset::Ocean,
                ThemePreset::Forest,
                ThemePreset::Rose,
            ]
            .iter()
            .position(|candidate| *candidate == current)
            .unwrap_or_default();
            model.settings_workspace.choice_picker_open = true;
            model.dirty = true;
            Vec::new()
        }
        SettingsPreference::DisplayLabel => {
            begin_display_label_edit(model);
            Vec::new()
        }
        SettingsPreference::ConnectCredential => {
            open_credential(model);
            Vec::new()
        }
        SettingsPreference::ManageProviders => {
            navigate_to_route(model, Route::Profiles);
            Vec::new()
        }
        SettingsPreference::ConfigureModels => {
            sync_model_default_selection(model);
            model.settings_workspace.detail_open = true;
            model.dirty = true;
            Vec::new()
        }
        SettingsPreference::OpenSessions => {
            navigate_to_route(model, Route::Sessions);
            Vec::new()
        }
        SettingsPreference::OpenMemory => {
            navigate_to_route(model, Route::Memory);
            Vec::new()
        }
        SettingsPreference::ReducedMotion => change_selected_preference(model, 1),
        preference if preference.editable() => change_selected_preference(model, 1),
        _ => Vec::new(),
    }
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
        SettingsPreference::DisplayLabel
        | SettingsPreference::Provider
        | SettingsPreference::Profile
        | SettingsPreference::Credential
        | SettingsPreference::Source
        | SettingsPreference::Model
        | SettingsPreference::Mode
        | SettingsPreference::Approvals
        | SettingsPreference::Retention
        | SettingsPreference::Logging
        | SettingsPreference::GlyphCheck
        | SettingsPreference::KeyboardNavigation
        | SettingsPreference::StateIndicators
        | SettingsPreference::Workspace
        | SettingsPreference::ColorDepth
        | SettingsPreference::Version
        | SettingsPreference::ManageProviders
        | SettingsPreference::ConnectCredential
        | SettingsPreference::ConfigureModels
        | SettingsPreference::OpenSessions
        | SettingsPreference::OpenMemory => return Vec::new(),
        SettingsPreference::ThemePreset => LocalPreferenceChange::ThemePreset(Some(cycle(
            *preferences.theme_preset().value(),
            &[
                ThemePreset::System,
                ThemePreset::Light,
                ThemePreset::Dark,
                ThemePreset::Aurora,
                ThemePreset::Ember,
                ThemePreset::Midnight,
                ThemePreset::Ocean,
                ThemePreset::Forest,
                ThemePreset::Rose,
            ],
            direction,
        ))),
        SettingsPreference::ColorMode => LocalPreferenceChange::ColorMode(Some(cycle(
            *preferences.color_mode().value(),
            &[
                ColorMode::Color,
                ColorMode::Soft,
                ColorMode::Vivid,
                ColorMode::NoColor,
                ColorMode::HighContrast,
            ],
            direction,
        ))),
        SettingsPreference::GlyphMode => LocalPreferenceChange::GlyphMode(Some(cycle(
            *preferences.glyph_mode().value(),
            &[GlyphMode::Unicode, GlyphMode::NerdFont, GlyphMode::Ascii],
            direction,
        ))),
        SettingsPreference::PromptStatusDetail => {
            LocalPreferenceChange::PromptStatusDetail(Some(cycle(
                *preferences.prompt_status_detail().value(),
                &[
                    PromptStatusDetail::Essential,
                    PromptStatusDetail::Workspace,
                    PromptStatusDetail::Detailed,
                ],
                direction,
            )))
        }
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
            SettingsPreference::Provider
            | SettingsPreference::Profile
            | SettingsPreference::Credential
            | SettingsPreference::Source
            | SettingsPreference::Model
            | SettingsPreference::Mode
            | SettingsPreference::Approvals
            | SettingsPreference::Retention
            | SettingsPreference::Logging
            | SettingsPreference::GlyphCheck
            | SettingsPreference::KeyboardNavigation
            | SettingsPreference::StateIndicators
            | SettingsPreference::Workspace
            | SettingsPreference::ColorDepth
            | SettingsPreference::Version
            | SettingsPreference::ManageProviders
            | SettingsPreference::ConnectCredential
            | SettingsPreference::ConfigureModels
            | SettingsPreference::OpenSessions
            | SettingsPreference::OpenMemory => return Vec::new(),
            SettingsPreference::ThemePreset => LocalPreferenceChange::ThemePreset(None),
            SettingsPreference::ColorMode => LocalPreferenceChange::ColorMode(None),
            SettingsPreference::GlyphMode => LocalPreferenceChange::GlyphMode(None),
            SettingsPreference::PromptStatusDetail => {
                LocalPreferenceChange::PromptStatusDetail(None)
            }
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
            SettingsPreference::Provider
            | SettingsPreference::Profile
            | SettingsPreference::Credential
            | SettingsPreference::Source
            | SettingsPreference::Model
            | SettingsPreference::Mode
            | SettingsPreference::Approvals
            | SettingsPreference::Retention
            | SettingsPreference::Logging
            | SettingsPreference::GlyphCheck
            | SettingsPreference::KeyboardNavigation
            | SettingsPreference::StateIndicators
            | SettingsPreference::Workspace
            | SettingsPreference::ColorDepth
            | SettingsPreference::Version
            | SettingsPreference::ManageProviders
            | SettingsPreference::ConnectCredential
            | SettingsPreference::ConfigureModels
            | SettingsPreference::OpenSessions
            | SettingsPreference::OpenMemory => return Vec::new(),
            SettingsPreference::ThemePreset => {
                LocalPreferenceChange::ThemePreset(Some(ThemePreset::System))
            }
            SettingsPreference::ColorMode => {
                LocalPreferenceChange::ColorMode(Some(ColorMode::Color))
            }
            SettingsPreference::GlyphMode => {
                LocalPreferenceChange::GlyphMode(Some(GlyphMode::Unicode))
            }
            SettingsPreference::PromptStatusDetail => {
                LocalPreferenceChange::PromptStatusDetail(Some(PromptStatusDetail::Workspace))
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

/// Resolves legacy command spellings to one canonical palette entry.
fn canonical_command_id(id: &str) -> &str {
    match id {
        "profiles" => "provider",
        "agents" => "models",
        "user-profile" => "user",
        "new-session" => "new",
        "refresh-models" => "refresh",
        "connect-api-key" => "connect",
        "toggle-tools" => "tools",
        _ => id,
    }
}

fn open_settings_tab(model: &mut Model, tab: usize) {
    navigate_to_route(model, Route::Settings);
    let category = match tab {
        1 => SettingsCategory::Providers,
        2 => SettingsCategory::Profile,
        3 => SettingsCategory::ModelsThinking,
        _ => SettingsCategory::Appearance,
    };
    model.settings_workspace.nav_selected = SettingsCategory::ALL
        .iter()
        .position(|candidate| *candidate == category)
        .unwrap_or_default();
    model.settings_workspace.nav_focus = false;
    normalize_settings_selection(model);
    if category == SettingsCategory::Providers {
        model.profile_center.focus = ProfileCenterFocus::ProviderChoices;
    } else if category == SettingsCategory::ModelsThinking {
        sync_model_default_selection(model);
        model.settings_workspace.detail_open = true;
    }
    model.dirty = true;
}

/// Runs one shared command by its stable table identity.
fn run_command_by_id(model: &mut Model, id: &str) -> Result<Vec<UiEffect>, String> {
    let canonical = canonical_command_id(id);
    let entry = COMMANDS
        .iter()
        .find(|entry| entry.id == canonical)
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
        "memory" => {
            navigate_to_route(model, Route::Memory);
            Vec::new()
        }
        "remember" => {
            navigate_to_route(model, Route::Memory);
            open_memory_lifecycle(model, MemoryLifecycleMode::Remember);
            Vec::new()
        }
        "memory-actions" => {
            navigate_to_route(model, Route::Memory);
            open_memory_lifecycle(model, MemoryLifecycleMode::Actions);
            Vec::new()
        }
        "memory-export" => {
            navigate_to_route(model, Route::Memory);
            open_memory_lifecycle(model, MemoryLifecycleMode::Export);
            Vec::new()
        }
        "profile" => {
            open_settings_tab(model, 2);
            Vec::new()
        }
        "provider" => {
            navigate_to_route(model, Route::Profiles);
            Vec::new()
        }
        "models" => {
            open_settings_tab(model, 3);
            Vec::new()
        }
        "user" => {
            open_settings_tab(model, 2);
            open_user_profile(model);
            Vec::new()
        }
        "session-model" => {
            open_picker(model);
            Vec::new()
        }
        "connect" => {
            open_settings_tab(model, 1);
            open_credential(model);
            Vec::new()
        }
        "settings" => {
            navigate_to_route(model, Route::Settings);
            Vec::new()
        }
        "retry" => retry_attempt(model),
        "cancel" => cancel_attempt(model),
        "search" => {
            close_active_overlay_state(model);
            navigate_to_route(model, Route::Chat);
            open_search(model);
            Vec::new()
        }
        "tools" => {
            model.tools_expanded = !model.tools_expanded;
            model.dirty = true;
            Vec::new()
        }
        "refresh" => refresh_catalog(model),
        "new" => create_session(model),
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
    if model.route() == Route::Chat {
        follow_tail(model);
    }
}

fn close_palette(model: &mut Model) {
    model.palette.query.clear();
    model.palette.selected = None;
    let _ = model.close_overlay(OverlayKind::CommandPalette);
}

fn filtered_palette_commands(model: &Model) -> Vec<CommandEntry> {
    model.palette_entries()
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
        let query = model.palette.query.clone();
        if let Ok(effects) = run_command_by_id(model, &query) {
            close_palette(model);
            return effects;
        }
        close_palette(model);
        if !query.is_empty() {
            model.composer.editor.insert_str(format!("/{query}"));
            model.notice = Some(Notice::Failure(UiFailure::new(
                ErrorClass::Validation,
                format!("Unknown command '/{query}'"),
                RetryPolicy::Never,
            )));
            model.dirty = true;
        }
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
    if matches!(
        input,
        Input {
            key: Key::Char('/'),
            ctrl: false,
            alt: false,
            ..
        }
    ) && model.palette.query.is_empty()
    {
        close_palette(model);
        model.composer.editor.insert_str("//");
        model.dirty = true;
        return Vec::new();
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
        } if model.palette.query.is_empty() => {
            close_palette(model);
            model.dirty = true;
            Vec::new()
        }
        Input {
            key: Key::Backspace,
            ..
        } => {
            model.palette.query.pop();
            model.palette.selected = None;
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
            model.palette.selected = None;
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
            key: Key::Char('d' | 'D'),
            ctrl: false,
            alt: false,
            ..
        } => set_selected_profile_default_model(model),
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
    sync_model_default_selection(model);
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
        OverlayKind::MemoryLifecycle => {
            if model.memory_lifecycle_pending() {
                return;
            }
            model.memory_lifecycle = None;
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
        Route::Memory => model.sync_memory_selection(),
    }
    model.notice = None;
}

fn close_profile_center(model: &mut Model) {
    if model.route() == Route::Settings {
        model.settings_workspace.nav_focus = true;
        model.notice = None;
        model.dirty = true;
    } else {
        navigate_to_route(model, Route::Chat);
    }
}

fn create_profile_editor(model: &mut Model) -> Vec<UiEffect> {
    let choice = PROVIDER_CHOICES
        .get(model.profile_center.choice_selected)
        .copied()
        .unwrap_or(ProviderChoice::Gemini);
    match choice {
        ProviderChoice::Gemini => open_provider_setup(model, ProviderKindLabel::Gemini, ""),
        ProviderChoice::GoogleAiStudio => return begin_google_ai_studio_setup(model),
        ProviderChoice::Codex => {
            model.profile_center.auth_page = Some(ProviderChoice::Codex);
            model.profile_center.codex_login = CodexLoginState::Idle;
            model.notice = None;
        }
        ProviderChoice::OpenAiCompatible => {
            open_provider_setup(model, ProviderKindLabel::Router, "");
        }
        ProviderChoice::Cursor => {
            model.notice = Some(Notice::Info(
                "Cursor authentication is documented through 'agent login', but its AutoHarness CLI bridge is not installed"
                    .to_owned(),
            ));
        }
        ProviderChoice::ClaudeCode => {
            model.notice = Some(Notice::Info(
                "Claude Code authentication is documented through 'claude auth login', but its AutoHarness CLI bridge is not installed"
                    .to_owned(),
            ));
        }
    }
    model.dirty = true;
    Vec::new()
}

fn begin_codex_login(model: &mut Model) -> Vec<UiEffect> {
    if model.profile_center.auth_page != Some(ProviderChoice::Codex)
        || matches!(
            model.profile_center.codex_login,
            CodexLoginState::Starting | CodexLoginState::BrowserOpened
        )
    {
        return Vec::new();
    }
    model.profile_center.codex_login = CodexLoginState::Starting;
    model.notice = None;
    let request_id = model.allocate_request();
    model.pending.insert(request_id, PendingKind::CodexLogin);
    model.dirty = true;
    vec![UiEffect::Dispatch(UiIntent::StartCodexLogin { request_id })]
}

fn begin_google_ai_studio_setup(model: &mut Model) -> Vec<UiEffect> {
    let profile_id = "google-ai-studio".to_owned();
    let profile = ProviderProfileDraft {
        id: profile_id.clone(),
        kind: ProviderKindLabel::Gemini,
        base_url: String::new(),
        project: String::new(),
        auth_header: String::new(),
    };
    let request_id = model.allocate_request();
    model
        .pending
        .insert(request_id, PendingKind::UpsertProfile(profile.clone()));
    model.profile_center.open_credential_after_save = Some(profile_id);
    model.notice = Some(Notice::Info(
        "Preparing Google AI Studio key entry...".to_owned(),
    ));
    model.dirty = true;
    vec![UiEffect::Dispatch(UiIntent::UpsertProfile {
        request_id,
        profile,
    })]
}

fn open_provider_setup(model: &mut Model, kind: ProviderKindLabel, id: &str) {
    model.profile_center.editor = Some(ProfileEditorState {
        mode: ProfileEditorMode::Create,
        source_id: None,
        field: 0,
        id: id.to_owned(),
        kind,
        base_url: String::new(),
        project: String::new(),
        auth_header: String::new(),
    });
    model.notice = None;
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
    if model.profile_center.auth_page == Some(ProviderChoice::Codex) {
        return match input {
            Input {
                key: Key::Char('c' | 'C'),
                ctrl: true,
                ..
            } => {
                model.should_quit = true;
                vec![UiEffect::Quit]
            }
            Input { key: Key::Esc, .. } => cancel_codex_login(model),
            Input {
                key: Key::Enter, ..
            } if !matches!(
                model.profile_center.codex_login,
                CodexLoginState::Starting | CodexLoginState::BrowserOpened
            ) =>
            {
                begin_codex_login(model)
            }
            _ => Vec::new(),
        };
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
        Input { key: Key::Left, .. }
            if model.profile_center.focus == ProfileCenterFocus::ConnectedProfiles =>
        {
            model.profile_center.focus = ProfileCenterFocus::ProviderChoices;
            model.dirty = true;
            Vec::new()
        }
        Input {
            key: Key::Right, ..
        } if model.profile_center.focus == ProfileCenterFocus::ProviderChoices
            && model.filtered_profiles().next().is_some() =>
        {
            model.profile_center.focus = ProfileCenterFocus::ConnectedProfiles;
            model.sync_profile_selection();
            model.dirty = true;
            Vec::new()
        }
        Input { key: Key::Up, .. }
            if model.profile_center.focus == ProfileCenterFocus::ProviderChoices =>
        {
            move_provider_choice(model, -1);
            Vec::new()
        }
        Input { key: Key::Down, .. }
            if model.profile_center.focus == ProfileCenterFocus::ProviderChoices =>
        {
            move_provider_choice(model, 1);
            Vec::new()
        }
        Input { key: Key::Up, .. } => {
            move_connected_profile(model, -1);
            Vec::new()
        }
        Input { key: Key::Down, .. } => {
            move_connected_profile(model, 1);
            Vec::new()
        }
        Input {
            key: Key::Enter, ..
        } if model.profile_center.focus == ProfileCenterFocus::ProviderChoices => {
            create_profile_editor(model)
        }
        Input {
            key: Key::Enter, ..
        } => {
            let profile_id = model.profile_selection().map(str::to_owned);
            profile_id.map_or_else(Vec::new, |profile_id| activate_profile(model, profile_id))
        }
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
        } if model.profile_center.focus == ProfileCenterFocus::ConnectedProfiles => {
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
        } if !character.is_control()
            && model.profile_center.focus == ProfileCenterFocus::ConnectedProfiles =>
        {
            model.profile_center.query.push(character);
            model.sync_profile_selection();
            model.dirty = true;
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn move_provider_choice(model: &mut Model, direction: isize) {
    let last = PROVIDER_CHOICES.len().saturating_sub(1);
    if direction < 0 && model.profile_center.choice_selected == 0 {
        if model.route() == Route::Settings {
            model.settings_workspace.nav_focus = true;
            model.dirty = true;
            return;
        }
        model.profile_center.choice_selected = last;
    } else {
        model.profile_center.choice_selected = model
            .profile_center
            .choice_selected
            .saturating_add_signed(direction)
            .min(last);
    }
    model.dirty = true;
}

fn move_connected_profile(model: &mut Model, direction: isize) {
    let profile_ids = model
        .filtered_profiles()
        .map(|profile| profile.id.clone())
        .collect::<Vec<_>>();
    if profile_ids.is_empty() {
        model.profile_center.focus = ProfileCenterFocus::ProviderChoices;
        model.dirty = true;
        return;
    }
    let current = model
        .profile_center
        .selected
        .as_ref()
        .and_then(|selected| profile_ids.iter().position(|id| id == selected))
        .unwrap_or_default();
    let next = (isize::try_from(current).unwrap_or(0) + direction).clamp(
        0,
        isize::try_from(profile_ids.len().saturating_sub(1)).unwrap_or(0),
    );
    model.profile_center.selected = profile_ids.get(usize::try_from(next).unwrap_or(0)).cloned();
    model.dirty = true;
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
fn handle_model_defaults_input(model: &mut Model, input: Input) -> Vec<UiEffect> {
    match input {
        Input {
            key: Key::Char('c' | 'C'),
            ctrl: true,
            ..
        } => {
            model.should_quit = true;
            vec![UiEffect::Quit]
        }
        Input { key: Key::Esc, .. } => {
            model.settings_workspace.nav_focus = true;
            model.dirty = true;
            Vec::new()
        }
        Input { key: Key::Tab, .. } => {
            model.model_defaults.step = match model.model_defaults.step {
                ModelDefaultStep::Model => ModelDefaultStep::Thinking,
                ModelDefaultStep::Thinking => ModelDefaultStep::Model,
            };
            model.dirty = true;
            Vec::new()
        }
        Input { key: Key::Up, .. } => {
            if model.model_defaults.model_selected == 0 {
                model.settings_workspace.nav_focus = true;
                model.dirty = true;
            } else {
                move_model_default_selection(model, -1);
            }
            Vec::new()
        }
        Input { key: Key::Down, .. } => {
            move_model_default_selection(model, 1);
            Vec::new()
        }
        Input { key: Key::Left, .. } => {
            move_thinking_default_selection(model, -1);
            Vec::new()
        }
        Input {
            key: Key::Right, ..
        } => {
            move_thinking_default_selection(model, 1);
            Vec::new()
        }
        Input {
            key: Key::Enter, ..
        } => save_model_defaults(model),
        _ => Vec::new(),
    }
}

fn move_model_default_selection(model: &mut Model, direction: isize) {
    let count = model
        .catalog
        .models()
        .iter()
        .filter(|summary| summary.selectable)
        .count();
    if count == 0 {
        return;
    }
    model.model_defaults.model_selected = model
        .model_defaults
        .model_selected
        .saturating_add_signed(direction)
        .min(count.saturating_sub(1));
    model.model_defaults.step = ModelDefaultStep::Model;
    model.dirty = true;
}

fn move_thinking_default_selection(model: &mut Model, direction: isize) {
    model.model_defaults.thinking_selected = model
        .model_defaults
        .thinking_selected
        .saturating_add_signed(direction)
        .min(MODEL_THINKING_LEVELS.len().saturating_sub(1));
    model.model_defaults.step = ModelDefaultStep::Thinking;
    model.dirty = true;
}

fn save_model_defaults(model: &mut Model) -> Vec<UiEffect> {
    let Some(summary) = model
        .catalog
        .models()
        .iter()
        .filter(|summary| summary.selectable)
        .nth(model.model_defaults.model_selected)
    else {
        return Vec::new();
    };
    model.model_defaults.model = Some(summary.model.clone());
    persist_model_default(model)
}

fn sync_model_default_selection(model: &mut Model) {
    let active_profile = model
        .profiles()
        .profiles
        .iter()
        .find(|profile| profile.active);
    let default_model = active_profile
        .and_then(|profile| profile.default_model.as_deref())
        .or(model.profiles().user.default_model.as_deref())
        .map(str::to_owned);
    let default_thinking = active_profile
        .map(|profile| profile.default_mode.as_str())
        .filter(|effort| !effort.is_empty())
        .unwrap_or(model.profiles().user.default_mode.as_str())
        .to_owned();
    let selectable = model
        .catalog
        .models()
        .iter()
        .filter(|summary| summary.selectable)
        .collect::<Vec<_>>();
    let selected = default_model
        .as_deref()
        .and_then(|model_id| {
            selectable
                .iter()
                .position(|summary| summary.model.model_id().as_str() == model_id)
        })
        .unwrap_or(0)
        .min(selectable.len().saturating_sub(1));
    model.model_defaults.model_selected = selected;
    model.model_defaults.model = selectable
        .get(selected)
        .map(|summary| summary.model.clone());
    model.model_defaults.thinking_selected = MODEL_THINKING_LEVELS
        .iter()
        .position(|effort| effort.eq_ignore_ascii_case(default_thinking.as_str()))
        .unwrap_or(0);
}

fn persist_model_default(model: &mut Model) -> Vec<UiEffect> {
    let Some(profile_id) = model
        .profiles()
        .profiles
        .iter()
        .find(|profile| profile.active)
        .map(|profile| profile.id.clone())
    else {
        return Vec::new();
    };
    let Some(selected_model) = model.model_defaults.model.clone() else {
        return Vec::new();
    };
    let request_id = model.allocate_request();
    model.pending.insert(
        request_id,
        PendingKind::SetProfileDefault {
            profile_id: profile_id.clone(),
            model: selected_model.clone(),
        },
    );
    model.notice = Some(Notice::Info("Saving default model...".to_owned()));
    model.model_defaults.step = ModelDefaultStep::Model;
    model.dirty = true;
    vec![UiEffect::Dispatch(UiIntent::SetProfileDefault {
        request_id,
        profile_id,
        model: selected_model,
        reasoning_effort: MODEL_THINKING_LEVELS
            .get(model.model_defaults.thinking_selected)
            .filter(|effort| **effort != "provider default")
            .map(|effort| (*effort).to_owned()),
    })]
}

fn handle_profile_editor_input(model: &mut Model, input: Input) -> Vec<UiEffect> {
    match input {
        Input { key: Key::Esc, .. } => {
            close_profile_editor(model);
            Vec::new()
        }
        Input {
            key: Key::Enter, ..
        } => submit_profile_editor(model),
        Input { key: Key::Up, .. } => {
            move_profile_editor_field(model, -1);
            Vec::new()
        }
        Input { key: Key::Down, .. } => {
            move_profile_editor_field(model, 1);
            Vec::new()
        }
        Input { key: Key::Tab, .. } => Vec::new(),
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
                    ProviderKindLabel::Router => ProviderKindLabel::CodexCli,
                    ProviderKindLabel::CodexCli => ProviderKindLabel::Gemini,
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

fn close_profile_editor(model: &mut Model) {
    model.profile_center.editor = None;
    model.notice = None;
    model.dirty = true;
}

fn cancel_codex_login(model: &mut Model) -> Vec<UiEffect> {
    let login_request = model.pending.iter().find_map(|(request_id, pending)| {
        matches!(pending, PendingKind::CodexLogin).then_some(*request_id)
    });
    model.profile_center.auth_page = None;
    model.profile_center.codex_login = CodexLoginState::Idle;
    model.notice = None;
    model.dirty = true;
    login_request.map_or_else(Vec::new, |request_id| {
        vec![UiEffect::Dispatch(UiIntent::CancelCodexLogin {
            request_id,
        })]
    })
}

fn move_profile_editor_field(model: &mut Model, direction: isize) {
    let Some(editor) = model.profile_center.editor.as_mut() else {
        return;
    };
    let first = usize::from(editor.mode == ProfileEditorMode::Edit);
    let count = if editor.mode == ProfileEditorMode::Duplicate {
        1
    } else {
        editor.field_count()
    };
    let selectable = count.saturating_sub(first);
    if selectable <= 1 {
        return;
    }
    let current = editor.field.saturating_sub(first);
    let next = (isize::try_from(current).unwrap_or(0) + direction)
        .rem_euclid(isize::try_from(selectable).unwrap_or(1));
    editor.field = first.saturating_add(usize::try_from(next).unwrap_or(0));
    model.dirty = true;
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
    if profile.kind == ProviderKindLabel::CodexCli {
        model.profile_center.auth_page = Some(ProviderChoice::Codex);
        model.profile_center.codex_login = CodexLoginState::Idle;
        model.notice = None;
        model.dirty = true;
        return;
    }
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

fn activate_profile(model: &mut Model, profile_id: String) -> Vec<UiEffect> {
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
        | PendingKind::SetProfileDefault { .. }
        | PendingKind::DisconnectProfile(_)
        | PendingKind::UpdateLocalPreference(_)
        | PendingKind::DeleteProfile(_)
        | PendingKind::RefreshCatalog
        | PendingKind::SelectModel(_)
        | PendingKind::SubmitPrompt(_)
        | PendingKind::CancelAttempt(_)
        | PendingKind::RetryAttempt(_)
        | PendingKind::AnswerPermission(_)
        | PendingKind::ExportTranscript
        | PendingKind::CodexLogin
        | PendingKind::RememberMemory(_)
        | PendingKind::ReviseMemory { .. }
        | PendingKind::ApproveMemoryProposal(_)
        | PendingKind::RejectMemoryProposal(_)
        | PendingKind::RetractMemory(_)
        | PendingKind::DeleteMemory(_)
        | PendingKind::ExportMemory(_) => false,
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
    if entry.active && model.session.active_attempt().is_some() {
        model.notice = Some(Notice::Info(
            "Cancel or finish the active response before deleting this session".to_owned(),
        ));
        model.dirty = true;
        return Vec::new();
    }
    if entry.active
        && !model
            .sessions
            .sessions
            .iter()
            .any(|candidate| !candidate.active && !candidate.archived)
    {
        model.notice = Some(Notice::Info(
            "Create another session before deleting your only open session".to_owned(),
        ));
        model.dirty = true;
        return Vec::new();
    }
    model.browser.confirming_delete = Some(entry.session_id.clone());
    let _ = model.open_overlay(OverlayKind::Confirmation);
    model.notice = None;
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
        Some(OverlayKind::MemoryLifecycle) => {
            let result = model
                .memory_lifecycle
                .as_mut()
                .and_then(|state| state.editor.as_mut())
                .map_or(Ok(()), |editor| editor.append_text(&editable_safe(text)));
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
            follow_tail(model);
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
        follow_tail(model);
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
        follow_tail(model);
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
        && model.overlay().is_none()
        && matches!(&*model.catalog, CatalogProjection::Ready { models, .. } if !models.is_empty())
    {
        navigate_to_route(model, Route::Chat);
        open_picker(model);
    }
    model.sync_retry_deadline();
    model.dirty = true;
}

fn apply_catalog(model: &mut Model, catalog: Arc<CatalogProjection>) {
    if !matches!(&*catalog, CatalogProjection::Loading) {
        model.startup_complete = true;
    }
    model.catalog = catalog;
    model.sync_catalog_retry_deadline();
    normalize_picker_selection(model);
    sync_model_default_selection(model);
    if model.route() == Route::Chat
        && model.overlay().is_none()
        && !selected_model_available(model)
        && matches!(&*model.catalog, CatalogProjection::Ready { models, .. } if !models.is_empty())
    {
        open_picker(model);
    }
    model.dirty = true;
}

fn finish_memory_lifecycle(model: &mut Model, request_id: crate::model::RequestId) {
    let owns_request = model
        .memory_lifecycle
        .as_ref()
        .is_some_and(|state| state.pending_request == Some(request_id));
    if !owns_request {
        return;
    }
    if let Some(state) = model.memory_lifecycle.as_mut() {
        state.pending_request = None;
    }
    model.memory_lifecycle = None;
    let _ = model.close_overlay(OverlayKind::MemoryLifecycle);
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
                        let open_credential =
                            model.profile_center.open_credential_after_save.as_deref()
                                == Some(profile.id.as_str());
                        model.profile_center.selected = Some(profile.id.clone());
                        model.profile_center.editor = None;
                        if open_credential {
                            model.profile_center.open_credential_after_save = None;
                            model.profile_center.credential = Some(ProfileCredentialEditor::new(
                                profile.id,
                                ProfileCredentialAction::Save,
                            ));
                            let _ = model.open_overlay(OverlayKind::ProfileCredential);
                            model.notice = None;
                        } else {
                            model.notice = Some(Notice::Info("Provider profile saved".to_owned()));
                        }
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
                    PendingKind::SetProfileDefaultModel(_)
                    | PendingKind::SetProfileDefault { .. } => {
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
                        model.notice = None;
                    }
                    PendingKind::ExportTranscript => {
                        model.notice = Some(Notice::Info("Transcript exported".to_owned()));
                    }
                    PendingKind::CodexLogin => {
                        model.profile_center.codex_login = CodexLoginState::Idle;
                        model.notice = Some(Notice::Info("Codex sign-in cancelled".to_owned()));
                    }
                    PendingKind::RememberMemory(_) => {
                        finish_memory_lifecycle(model, request_id);
                        model.notice = Some(Notice::Info(
                            "Memory saved; waiting for the next ledger projection".to_owned(),
                        ));
                    }
                    PendingKind::ReviseMemory { .. } => {
                        finish_memory_lifecycle(model, request_id);
                        model.notice = Some(Notice::Info(
                            "Correction saved; prior revisions remain auditable".to_owned(),
                        ));
                    }
                    PendingKind::ApproveMemoryProposal(_) => {
                        finish_memory_lifecycle(model, request_id);
                        model.notice = Some(Notice::Info(
                            "Proposal approved for future eligible turns".to_owned(),
                        ));
                    }
                    PendingKind::RejectMemoryProposal(_) => {
                        finish_memory_lifecycle(model, request_id);
                        model.notice = Some(Notice::Info("Proposal rejected".to_owned()));
                    }
                    PendingKind::RetractMemory(_) => {
                        finish_memory_lifecycle(model, request_id);
                        model.notice = Some(Notice::Info(
                            "Memory retracted from future admission; dispatched turns cannot be recalled"
                                .to_owned(),
                        ));
                    }
                    PendingKind::DeleteMemory(_) => {
                        finish_memory_lifecycle(model, request_id);
                        model.notice = Some(Notice::Info(
                            "Logical deletion recorded; audit history remains and dispatched turns cannot be recalled"
                                .to_owned(),
                        ));
                    }
                    PendingKind::ExportMemory(_) => {
                        finish_memory_lifecycle(model, request_id);
                        model.notice = Some(Notice::Info("Memory exported".to_owned()));
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
                Some(PendingKind::ConfigureCredential) => {}
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
                    | PendingKind::SetProfileDefault { .. }
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
                Some(PendingKind::CodexLogin) => {
                    model.profile_center.codex_login = CodexLoginState::Failed;
                }
                Some(
                    PendingKind::RememberMemory(_)
                    | PendingKind::ReviseMemory { .. }
                    | PendingKind::ApproveMemoryProposal(_)
                    | PendingKind::RejectMemoryProposal(_)
                    | PendingKind::RetractMemory(_)
                    | PendingKind::DeleteMemory(_)
                    | PendingKind::ExportMemory(_),
                ) => {
                    if let Some(state) = model.memory_lifecycle.as_mut()
                        && state.pending_request == Some(request_id)
                    {
                        state.pending_request = None;
                    }
                }
            }
            model.notice = Some(Notice::Failure(failure));
        }
        UiNotice::CodexLoginBrowserOpened { request_id } => {
            if matches!(
                model.pending.get(&request_id),
                Some(PendingKind::CodexLogin)
            ) {
                model.profile_center.codex_login = CodexLoginState::BrowserOpened;
                model.notice = Some(Notice::Info(
                    "Finish signing in with Codex in your browser".to_owned(),
                ));
            }
        }
        UiNotice::CodexLoginCompleted { request_id } => {
            if matches!(
                model.pending.remove(&request_id),
                Some(PendingKind::CodexLogin)
            ) {
                model.profile_center.auth_page = None;
                model.profile_center.codex_login = CodexLoginState::Idle;
                model.notice = Some(Notice::Info(
                    "Codex subscription connected and ready".to_owned(),
                ));
            }
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
        navigate_to_route(model, Route::Settings);
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
            | PendingKind::SetProfileDefault { .. }
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
            | PendingKind::ExportTranscript
            | PendingKind::CodexLogin
            | PendingKind::RememberMemory(_)
            | PendingKind::ReviseMemory { .. }
            | PendingKind::ApproveMemoryProposal(_)
            | PendingKind::RejectMemoryProposal(_)
            | PendingKind::RetractMemory(_)
            | PendingKind::DeleteMemory(_)
            | PendingKind::ExportMemory(_) => false,
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
