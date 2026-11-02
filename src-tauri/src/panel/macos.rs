//! `NSPanel` conversion, shared by the Pulse and reminder panels.

use objc2::runtime::{AnyClass, AnyObject};
use objc2::{define_class, ClassType, MainThreadOnly};
use objc2_app_kit::{NSPanel, NSWindowCollectionBehavior, NSWindowStyleMask};
use objc2_foundation::NSObjectProtocol;
use tauri::WebviewWindow;

use super::Kind;

define_class!(
    // SAFETY: NSPanel has no subclassing requirements, the class adds no
    // instance variables, and it overrides nothing but two predicates.
    #[unsafe(super(NSPanel))]
    #[thread_kind = MainThreadOnly]
    #[name = "OpenRizeReminderPanel"]
    struct ReminderPanel;

    impl ReminderPanel {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key_window(&self) -> bool {
            false
        }

        #[unsafe(method(canBecomeMainWindow))]
        fn can_become_main_window(&self) -> bool {
            false
        }
    }
);

define_class!(
    // SAFETY: NSPanel has no subclassing requirements, the class adds no
    // instance variables, and it overrides nothing but two predicates.
    #[unsafe(super(NSPanel))]
    #[thread_kind = MainThreadOnly]
    #[name = "OpenRizePulsePanel"]
    struct PulsePanel;

    impl PulsePanel {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key_window(&self) -> bool {
            true
        }

        #[unsafe(method(canBecomeMainWindow))]
        fn can_become_main_window(&self) -> bool {
            false
        }
    }
);

/// The reminder never takes focus: it is a non-activating panel on every
/// Space. Pulse takes key focus (never main), so tiling window managers that
/// adopt an app's main window leave it alone; it opens on whichever Space is
/// current, including a full-screen app's, since without `FullScreenAuxiliary`
/// showing it there would switch the display away from that app.
pub fn make_panel(window: &WebviewWindow, kind: Kind) {
    let target = window.clone();
    let _ = window.run_on_main_thread(move || {
        let Ok(pointer) = target.ns_window() else {
            return;
        };
        let object = pointer.cast::<AnyObject>();
        let class: &AnyClass = match kind {
            Kind::Reminder => ReminderPanel::class(),
            Kind::Pulse => PulsePanel::class(),
        };
        // SAFETY: `ns_window` is the live NSWindow behind this Tauri window and
        // this runs on the main thread. Swapping an NSWindow to an NSPanel
        // subclass with no extra instance variables is how the
        // `tauri-nspanel` crate makes windows non-activating; the panel then
        // overrides the predicates tao's own subclass used for focusability.
        unsafe { objc2::ffi::object_setClass(object, class) };
        // SAFETY: the object is now a `ReminderPanel` or `PulsePanel`, an NSPanel.
        let panel = unsafe { &*object.cast::<NSPanel>() };
        match kind {
            Kind::Reminder => {
                panel.setStyleMask(panel.styleMask() | NSWindowStyleMask::NonactivatingPanel);
                panel.setFloatingPanel(true);
                panel.setHidesOnDeactivate(false);
                panel.setBecomesKeyOnlyIfNeeded(true);
                panel.setCollectionBehavior(
                    panel.collectionBehavior()
                        | NSWindowCollectionBehavior::CanJoinAllSpaces
                        | NSWindowCollectionBehavior::FullScreenAuxiliary
                        | NSWindowCollectionBehavior::Stationary
                        | NSWindowCollectionBehavior::IgnoresCycle,
                );
            }
            Kind::Pulse => {
                // Panels hide when the app deactivates; the blur handler already
                // hides this one, and two paths racing to hide it confuse the toggle.
                panel.setHidesOnDeactivate(false);
                panel.setCollectionBehavior(
                    panel.collectionBehavior()
                        | NSWindowCollectionBehavior::CanJoinAllSpaces
                        | NSWindowCollectionBehavior::FullScreenAuxiliary,
                );
            }
        }
    });
}

/// Only the reminder panel uses this, once it is on screen.
pub fn prevent_activation(window: &WebviewWindow) {
    let target = window.clone();
    let _ = window.run_on_main_thread(move || {
        let Ok(pointer) = target.ns_window() else {
            return;
        };
        // SAFETY: `make_panel` made this window an NSPanel, and this runs on the
        // main thread.
        let panel = unsafe { &*pointer.cast::<NSPanel>() };
        let mask = panel.styleMask();
        panel.setStyleMask(mask - NSWindowStyleMask::NonactivatingPanel);
        panel.setStyleMask(mask | NSWindowStyleMask::NonactivatingPanel);
        // The style mask alone does not stop a click from activating the app
        // (the mask bit is only honoured when the window is created with it).
        // AppKit's own flag for it is private, so it is used only when present.
        let selector = objc2::sel!(_setPreventsActivation:);
        if panel.respondsToSelector(selector) {
            // SAFETY: the selector takes one BOOL and returns nothing.
            let _: () = unsafe { objc2::msg_send![panel, _setPreventsActivation: true] };
        }
    });
}
