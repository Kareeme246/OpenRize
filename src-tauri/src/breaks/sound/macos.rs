//! The system "Glass" sound.

use objc2_app_kit::NSSound;
use objc2_foundation::NSString;

pub fn play_chime() {
    if let Some(sound) = NSSound::soundNamed(&NSString::from_str("Glass")) {
        sound.play();
    }
}
