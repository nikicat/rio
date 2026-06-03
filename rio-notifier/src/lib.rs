/// Request notification authorization from the OS.
/// On macOS this triggers the permission prompt on first call.
/// No-op on other platforms.
pub fn request_authorization() {
    #[cfg(target_os = "macos")]
    platform::request_authorization();
}

/// A callback run when the user clicks/activates a notification, off the main
/// thread. Boxed because it is moved onto the notifier's background thread,
/// which outlives the call. The argument is the xdg-activation token the
/// daemon supplied with the click (Linux/Wayland), if any — the caller uses it
/// to raise the window past focus-stealing prevention.
pub type ActivateHandler = Box<dyn FnOnce(Option<String>) + Send + 'static>;

/// Send a desktop notification using the platform's native API.
///
/// - **macOS**: `UNUserNotificationCenter` (requires app bundle with identifier).
/// - **Linux**: D-Bus `org.freedesktop.Notifications`.
/// - **Windows**: Toast notifications via `windows` crate.
///
/// When `on_activate` is `Some`, the notification is made clickable and the
/// handler runs (on the background thread) if the user activates it. Today
/// only the Linux/D-Bus backend wires this up; macOS and Windows ignore it
/// and stay fire-and-forget.
///
/// Spawns a background thread so the caller is never blocked.
pub fn send_notification(title: &str, body: &str, on_activate: Option<ActivateHandler>) {
    let title = if title.is_empty() {
        "Rio".to_string()
    } else {
        title.to_string()
    };
    let body = body.to_string();

    // Single breadcrumb at the hand-off to the platform notification backend —
    // enough to confirm a notification was attempted (and whether it carries a
    // click action) without enabling debug-level noise.
    tracing::info!(
        "sending desktop notification: title={title:?} actionable={}",
        on_activate.is_some(),
    );

    std::thread::spawn(move || {
        platform::notify(&title, &body, on_activate);
    });
}

#[cfg(target_os = "macos")]
mod platform {
    use block2::RcBlock;
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};
    use objc2::runtime::Bool;
    use objc2_foundation::{NSError, NSString};
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationRequest,
        UNUserNotificationCenter,
    };
    use std::sync::Once;

    pub(crate) fn request_authorization() {
        static INIT: Once = Once::new();
        INIT.call_once(|| unsafe {
            let bundle: *mut Object = msg_send![class!(NSBundle), mainBundle];
            if bundle.is_null() {
                return;
            }
            let bundle_id: *mut Object = msg_send![bundle, bundleIdentifier];
            if bundle_id.is_null() {
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

    // TODO: wire `_on_activate` through a `UNUserNotificationCenterDelegate`
    // so clicking the notification raises the right tab on macOS too.
    pub fn notify(
        title: &str,
        body: &str,
        _on_activate: Option<super::ActivateHandler>,
    ) {
        unsafe {
            // UNUserNotificationCenter crashes if the app has no bundle
            // identifier (e.g. cargo run). Guard like Kitty does.
            let bundle: *mut Object = msg_send![class!(NSBundle), mainBundle];
            if bundle.is_null() {
                return;
            }
            let bundle_id: *mut Object = msg_send![bundle, bundleIdentifier];
            if bundle_id.is_null() {
                return;
            }

            let center = UNUserNotificationCenter::currentNotificationCenter();

            let content = UNMutableNotificationContent::new();
            content.setTitle(&NSString::from_str(title));
            content.setBody(&NSString::from_str(body));

            let identifier = NSString::from_str("rio-notification");
            let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
                &identifier,
                &content,
                None,
            );

            center.addNotificationRequest_withCompletionHandler(&request, None);
        }
    }
}

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
mod platform {
    use std::collections::HashMap;

    const DEST: &str = "org.freedesktop.Notifications";
    const PATH: &str = "/org/freedesktop/Notifications";

    pub fn notify(title: &str, body: &str, on_activate: Option<super::ActivateHandler>) {
        let connection = match zbus::blocking::Connection::session() {
            Ok(connection) => connection,
            Err(err) => {
                tracing::warn!("notifier: no session bus: {err}");
                return;
            }
        };
        let proxy = match zbus::blocking::Proxy::new(&connection, DEST, PATH, DEST) {
            Ok(proxy) => proxy,
            Err(err) => {
                tracing::warn!("notifier: could not build notifications proxy: {err}");
                return;
            }
        };
        let hints: HashMap<&str, zbus::zvariant::Value<'_>> = HashMap::new();

        // Without a click handler, stay fire-and-forget: emit the notification
        // with no actions and let the thread exit immediately.
        let Some(on_activate) = on_activate else {
            let _: Result<u32, _> = proxy.call(
                "Notify",
                &(
                    "Rio", 0u32, "rio", title, body,
                    &[] as &[&str], // actions
                    &hints,
                    -1i32, // expire_timeout
                ),
            );
            return;
        };

        // Subscribe to the daemon's signals *before* posting, so a fast click
        // can't slip through the gap between `Notify` returning and the match
        // rule being installed. If we can't subscribe, fall back to a plain
        // notification rather than dropping it.
        let Some(mut signals) = subscribe(&connection) else {
            tracing::warn!("notifier: could not subscribe to daemon signals; fire-and-forget");
            let _: Result<u32, _> = proxy.call(
                "Notify",
                &("Rio", 0u32, "rio", title, body, &[] as &[&str], &hints, -1i32),
            );
            return;
        };

        // "default" is the special action a notifier invokes when the body
        // (not a button) is clicked; the label is only shown by daemons that
        // surface it as an explicit button.
        let id: u32 = match proxy.call(
            "Notify",
            &(
                "Rio", 0u32, "rio", title, body,
                &["default", "Open"] as &[&str],
                &hints,
                -1i32,
            ),
        ) {
            Ok(id) => id,
            Err(err) => {
                tracing::warn!("notifier: Notify call failed: {err}");
                return;
            }
        };

        // Block this background thread until our notification is activated or
        // closed. `NotificationClosed` (dismiss/expire/replace) guarantees the
        // loop terminates, so the thread never leaks when the user ignores it.
        //
        // The daemon emits `ActivationToken` (an xdg-activation token for
        // raising our window) just before `ActionInvoked`, so we stash the most
        // recent one for our id and hand it to the callback on activation.
        let mut pending_token: Option<String> = None;
        for msg in signals.by_ref() {
            let Ok(msg) = msg else { continue };
            let header = msg.header();
            let Some(member) = header.member() else { continue };
            match member.as_str() {
                "ActivationToken" => {
                    if let Ok((nid, token)) =
                        msg.body().deserialize::<(u32, String)>()
                    {
                        if nid == id {
                            pending_token = Some(token);
                        }
                    }
                }
                "ActionInvoked" => {
                    if let Ok((nid, action)) =
                        msg.body().deserialize::<(u32, String)>()
                    {
                        if nid == id && action == "default" {
                            on_activate(pending_token.take());
                            return;
                        }
                    }
                }
                "NotificationClosed" => {
                    if let Ok((nid, _reason)) =
                        msg.body().deserialize::<(u32, u32)>()
                    {
                        if nid == id {
                            return;
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Start buffering `ActionInvoked` / `NotificationClosed` signals from the
    /// notification daemon. Returns `None` if the match rule can't be built or
    /// installed.
    fn subscribe(
        connection: &zbus::blocking::Connection,
    ) -> Option<zbus::blocking::MessageIterator> {
        let rule = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface(DEST)
            .ok()?
            .path(PATH)
            .ok()?
            .build();
        zbus::blocking::MessageIterator::for_match_rule(rule, connection, Some(16)).ok()
    }
}

#[cfg(target_os = "windows")]
mod platform {
    // TODO: handle toast activation so a click raises the right tab on Windows.
    pub fn notify(
        title: &str,
        body: &str,
        _on_activate: Option<super::ActivateHandler>,
    ) {
        use windows::core::HSTRING;
        use windows::Data::Xml::Dom::XmlDocument;
        use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};

        let Ok(xml) = XmlDocument::new() else {
            return;
        };
        let toast_xml = format!(
            r#"<toast><visual><binding template="ToastGeneric"><text>{}</text><text>{}</text></binding></visual></toast>"#,
            title, body,
        );
        if xml.LoadXml(&HSTRING::from(&toast_xml)).is_err() {
            return;
        }
        let Ok(toast) = ToastNotification::CreateToastNotification(&xml) else {
            return;
        };
        let Ok(notifier) =
            ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from("Rio"))
        else {
            return;
        };
        let _ = notifier.Show(&toast);
    }
}
