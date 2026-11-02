//! The "SystemNotification" scheme sound, or the default beep without one.

use windows::core::w;
use windows::Win32::Media::Audio::{PlaySoundW, SND_ALIAS, SND_ASYNC, SND_NODEFAULT};
use windows::Win32::System::Diagnostics::Debug::MessageBeep;
use windows::Win32::UI::WindowsAndMessaging::MB_OK;

pub fn play_chime() {
    // SAFETY: plays a named system sound asynchronously; the alias is a
    // static string.
    let played = unsafe {
        PlaySoundW(
            w!("SystemNotification"),
            None,
            SND_ALIAS | SND_ASYNC | SND_NODEFAULT,
        )
    };
    // A sound scheme without that event (or with no alias at all) fails the
    // call above, which would leave the setting on and the chime silent.
    if !played.as_bool() {
        // SAFETY: `MessageBeep` takes a beep type and no pointers. A failed
        // beep has nothing left to fall back to.
        let _ = unsafe { MessageBeep(MB_OK) };
    }
}
