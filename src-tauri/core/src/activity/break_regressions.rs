use super::*;

fn paused_store() -> ActivityStore {
    let mut store = ActivityStore::from_conn(Connection::open_in_memory().unwrap()).unwrap();
    store.tracking_hours.enabled = false;
    store.set_capture_enabled(false, 1_000).unwrap();
    store
}

fn background_sample() -> Option<WindowSample> {
    Some(WindowSample {
        app: "Player".into(),
        title: "Video".into(),
        bundle_id: None,
        url: None,
        domain: None,
        keeps_display_awake: true,
    })
}

#[test]
fn explicitly_started_break_survives_paused_capture_and_background_awake_signals() {
    let mut store = paused_store();
    store
        .start_session(KIND_BREAK, Some("Break"), 2_000)
        .unwrap();
    let id = store.live_state(2_000).segment_info().unwrap().id;
    for now in (3_000..=180_000).step_by(1_000) {
        store.tick(background_sample(), now - 2_000, now).unwrap();
        let live = store.live_state(now);
        assert_eq!(live.segment_info().unwrap().id, id);
        // No display-awake assertion can masquerade as fresh user input.
        assert_eq!(live.last_idle_ms, now - 2_000);
    }
    assert!(!store.capture_enabled);
    store.stop_session(181_000).unwrap();
    store.tick(background_sample(), 180_000, 182_000).unwrap();
    assert!(store.live_state(182_000).segment_info().is_none());
}

#[test]
fn paused_capture_still_suppresses_automatic_idle_and_focus_capture() {
    let mut store = paused_store();
    store.tick(background_sample(), 300_000, 2_000).unwrap();
    assert!(store.live_state(2_000).segment_info().is_none());
    store
        .start_session(KIND_FOCUS, Some("Focus"), 3_000)
        .unwrap();
    store.tick(background_sample(), 0, 4_000).unwrap();
    assert!(store.live_state(4_000).segment_info().is_none());
}
