use block2::RcBlock;
use objc::runtime::Object;
use objc::{class, msg_send, sel, sel_impl};
use objc2::runtime::Bool;
use objc2_foundation::{NSArray, NSError, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationRequest,
    UNUserNotificationCenter,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Once;

/// Source of per-notification identifiers. A unique id per notification
/// keeps them independent — so two panes ringing show two notifications and
/// `Handle::close` withdraws exactly the one for the focused pane, matching
/// the Linux and Windows backends. (Reusing one id would make a new request
/// silently replace the previous notification.)
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

fn next_identifier() -> String {
    format!("rio-notification-{}", NEXT_ID.fetch_add(1, Ordering::Relaxed))
}

/// No-op: `Handle::close` withdraws synchronously on macOS, so nothing is in
/// flight at shutdown.
pub(crate) fn shutdown() {}

/// Whether the app has a bundle identifier. `UNUserNotificationCenter`
/// crashes without one (e.g. `cargo run`), so every entry point guards on
/// it the way Kitty does.
fn has_bundle_identifier() -> bool {
    unsafe {
        let bundle: *mut Object = msg_send![class!(NSBundle), mainBundle];
        if bundle.is_null() {
            return false;
        }
        let bundle_id: *mut Object = msg_send![bundle, bundleIdentifier];
        !bundle_id.is_null()
    }
}

pub(crate) fn request_authorization() {
    static INIT: Once = Once::new();
    INIT.call_once(|| unsafe {
        if !has_bundle_identifier() {
            return;
        }

        let center = UNUserNotificationCenter::currentNotificationCenter();
        center.requestAuthorizationWithOptions_completionHandler(
            UNAuthorizationOptions::UNAuthorizationOptionAlert
                | UNAuthorizationOptions::UNAuthorizationOptionSound,
            &RcBlock::new(|_ok: Bool, _err: *mut NSError| {}),
        );
    });
}

pub(crate) struct Handle {
    identifier: String,
}

impl Handle {
    pub(crate) fn close(self) {
        // Withdraw exactly this notification by its unique identifier; a
        // no-op if it was already dismissed or never delivered.
        if !has_bundle_identifier() {
            return;
        }
        unsafe {
            let center = UNUserNotificationCenter::currentNotificationCenter();
            let identifier = NSString::from_str(&self.identifier);
            let identifiers = NSArray::from_slice(&[&*identifier]);
            center.removeDeliveredNotificationsWithIdentifiers(&identifiers);
        }
    }
}

// TODO: wire `_on_activate` through a `UNUserNotificationCenterDelegate`
// so clicking the notification raises the right tab on macOS too.
pub(crate) fn notify(
    title: String,
    body: String,
    on_activate: Option<super::ActivateHandler>,
) -> Handle {
    let identifier = next_identifier();
    let thread_identifier = identifier.clone();
    std::thread::spawn(move || {
        notify_inner(&thread_identifier, &title, &body, on_activate)
    });
    Handle { identifier }
}

fn notify_inner(
    identifier: &str,
    title: &str,
    body: &str,
    _on_activate: Option<super::ActivateHandler>,
) {
    if !has_bundle_identifier() {
        return;
    }
    unsafe {
        let center = UNUserNotificationCenter::currentNotificationCenter();

        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(title));
        content.setBody(&NSString::from_str(body));

        let identifier = NSString::from_str(identifier);
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
            &identifier,
            &content,
            None,
        );

        center.addNotificationRequest_withCompletionHandler(&request, None);
    }
}
