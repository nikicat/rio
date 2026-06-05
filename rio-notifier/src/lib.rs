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

/// A handle to a posted notification, used to withdraw it from the desktop
/// before the user acts on it — e.g. when they open the pane that rang, so the
/// stale "Terminal bell" toast disappears the same way the tab-bar dot does.
pub struct NotificationHandle {
    inner: Inner,
}

enum Inner {
    /// Backed by a real platform notification.
    Platform(platform::Handle),
    /// Withdrawal delegated to a caller-supplied closure. Used by tests to
    /// observe `close` without a desktop, and available to embedders that post
    /// notifications themselves.
    Detached(Box<dyn FnOnce() + Send>),
}

impl NotificationHandle {
    /// Withdraw the notification if it is still showing. A no-op once it has
    /// been activated, dismissed, or expired. Implemented on all three
    /// backends (D-Bus `CloseNotification`, `removeDeliveredNotifications…`,
    /// `ToastNotifier::Hide`). Consumes the handle.
    pub fn close(self) {
        match self.inner {
            Inner::Platform(handle) => handle.close(),
            Inner::Detached(on_close) => on_close(),
        }
    }

    /// A handle whose [`close`](Self::close) runs `on_close` instead of touching
    /// any platform API. For tests that need to observe withdrawal, and for
    /// embedders supplying their own notification transport.
    #[doc(hidden)]
    pub fn detached(on_close: impl FnOnce() + Send + 'static) -> Self {
        Self {
            inner: Inner::Detached(Box::new(on_close)),
        }
    }
}

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
/// Returns a [`NotificationHandle`] for withdrawing the notification later.
/// Spawns a background thread so the caller is never blocked.
pub fn send_notification(
    title: &str,
    body: &str,
    on_activate: Option<ActivateHandler>,
) -> NotificationHandle {
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

    NotificationHandle {
        inner: Inner::Platform(platform::notify(title, body, on_activate)),
    }
}

/// Give just-requested withdraws a brief moment to reach the desktop before the
/// process exits. Call once from the app's exit path, *after* closing the
/// notification handles. Only the Linux backend defers withdraws to a
/// background thread, so this is a no-op on macOS and Windows (where `close`
/// withdraws synchronously).
pub fn shutdown() {
    platform::shutdown();
}

#[cfg(target_os = "macos")]
#[path = "macos.rs"]
mod platform;

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
#[path = "linux.rs"]
mod platform;

#[cfg(target_os = "windows")]
#[path = "windows.rs"]
mod platform;
