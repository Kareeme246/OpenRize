//! Developer-only controls for testing permission and onboarding states by hand. They simulate:
//! never TCC, the onboarding marker (shared with the installed app) or app data. Commands
//! refuse unless `allowed` passes, which a release build never does (debug_assertions off).

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::onboarding::Rerun;
use crate::permissions::{AccessibilityState, Monitor};

/// Dev scripts run under sub-identifiers of the release one (`.dev`, `.devshotNNNN`,
/// `.driveNNNN`), each with its own app data. The release identifier never qualifies.
pub fn allowed(identifier: &str, debug_build: bool) -> bool {
    debug_build
        && identifier
            .strip_prefix(openrize_core::APP_IDENTIFIER)
            .is_some_and(|rest| rest.starts_with('.') && rest.len() > 1)
}

fn require(app: &AppHandle) -> Result<(), String> {
    if allowed(&app.config().identifier, cfg!(debug_assertions)) {
        Ok(())
    } else {
        Err("Developer tools are only available in a dev build.".to_string())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DevToolsInfo {
    pub identifier: String,
    pub release_identifier: String,
    pub accessibility: Option<AccessibilityState>,
    pub automation: Option<bool>,
    pub onboarding_rerun: bool,
}

#[tauri::command]
pub fn dev_tools_info(app: AppHandle) -> Result<DevToolsInfo, String> {
    require(&app)?;
    let monitor = app.state::<Monitor>();
    Ok(DevToolsInfo {
        identifier: app.config().identifier.clone(),
        release_identifier: openrize_core::APP_IDENTIFIER.to_string(),
        accessibility: monitor.simulated(),
        automation: monitor.automation(),
        onboarding_rerun: app.state::<Rerun>().is_set(),
    })
}

/// `None` shows the real state again.
#[tauri::command]
pub fn dev_simulate_accessibility(
    app: AppHandle,
    state: Option<AccessibilityState>,
) -> Result<(), String> {
    require(&app)?;
    app.state::<Monitor>().simulate(&app, state);
    Ok(())
}

/// `None` shows the real answer again.
#[tauri::command]
pub fn dev_simulate_automation(app: AppHandle, granted: Option<bool>) -> Result<(), String> {
    require(&app)?;
    app.state::<Monitor>().simulate_automation(&app, granted);
    Ok(())
}

/// The simulation a re-run of the first-run screen needs. While Accessibility is off or a
/// restart is needed the screen opens onto a button; any other state would hide it or strand
/// its disabled button, so those become a simulated "not granted".
fn rerun_simulation(current: AccessibilityState) -> Option<AccessibilityState> {
    match current {
        AccessibilityState::NotGranted | AccessibilityState::RelaunchRequired => None,
        _ => Some(AccessibilityState::NotGranted),
    }
}

/// Opens the first-run screen again, simulating "not granted" if it would not show.
#[tauri::command]
pub fn dev_rerun_onboarding(app: AppHandle) -> Result<(), String> {
    require(&app)?;
    app.state::<Rerun>().set();
    let monitor = app.state::<Monitor>();
    if let Some(state) = rerun_simulation(monitor.state()) {
        monitor.simulate(&app, Some(state));
    }
    let _ = app.emit(crate::EVENT_ONBOARDING_CHANGED, ());
    Ok(())
}

#[tauri::command]
pub fn dev_reset(app: AppHandle) -> Result<(), String> {
    require(&app)?;
    let monitor = app.state::<Monitor>();
    monitor.simulate(&app, None);
    monitor.simulate_automation(&app, None);
    app.state::<Rerun>().clear();
    let _ = app.emit(crate::EVENT_ONBOARDING_CHANGED, ());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rerun_over_a_simulated_checking_state_does_not_strand_the_button() {
        assert_eq!(
            rerun_simulation(AccessibilityState::Checking),
            Some(AccessibilityState::NotGranted)
        );
    }

    #[test]
    fn a_rerun_leaves_the_states_that_already_show_the_screen_alone() {
        assert_eq!(rerun_simulation(AccessibilityState::NotGranted), None);
        assert_eq!(rerun_simulation(AccessibilityState::RelaunchRequired), None);
    }

    #[test]
    fn a_rerun_turns_every_other_state_into_not_granted() {
        for state in [
            AccessibilityState::Working,
            AccessibilityState::NotWorking,
            AccessibilityState::NotApplicable,
        ] {
            assert_eq!(
                rerun_simulation(state),
                Some(AccessibilityState::NotGranted)
            );
        }
    }

    #[test]
    fn the_release_identifier_is_refused_even_in_a_debug_build() {
        assert!(!allowed("com.offlinestudios.openrize", true));
    }

    #[test]
    fn a_dev_identifier_is_allowed_only_in_a_debug_build() {
        assert!(allowed("com.offlinestudios.openrize.dev", true));
        assert!(!allowed("com.offlinestudios.openrize.dev", false));
    }

    #[test]
    fn the_screenshot_and_drive_identifiers_are_allowed_in_a_debug_build() {
        assert!(allowed("com.offlinestudios.openrize.devshot4123", true));
        assert!(allowed("com.offlinestudios.openrize.drive5001", true));
    }

    #[test]
    fn other_identifiers_are_refused() {
        for identifier in [
            "com.offlinestudios.openrize.",
            "com.offlinestudios.openrizex",
            "com.example.dev",
        ] {
            assert!(!allowed(identifier, true), "{identifier}");
        }
    }
}
