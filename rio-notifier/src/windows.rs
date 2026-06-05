use std::sync::{Arc, Mutex};
use windows::UI::Notifications::{ToastNotification, ToastNotifier};

/// State shared between the delivery thread (which builds and shows the
/// toast) and the [`Handle`] held by the UI thread (which may withdraw it).
#[derive(Default)]
struct Shared {
    /// The shown toast and its notifier, kept so `close` can `Hide` it.
    /// `None` until the toast is shown, or after a withdraw.
    toast: Option<(ToastNotifier, ToastNotification)>,
    /// `close` ran before the toast was shown; the delivery thread hides it
    /// as soon as it has shown it.
    withdraw_pending: bool,
}

pub(crate) struct Handle {
    shared: Arc<Mutex<Shared>>,
}

impl Handle {
    pub(crate) fn close(self) {
        let mut shared = self.shared.lock().unwrap();
        match shared.toast.take() {
            Some((notifier, toast)) => {
                let _ = notifier.Hide(&toast);
            }
            None => shared.withdraw_pending = true,
        }
    }
}

/// No-op: `Handle::close` withdraws synchronously on Windows, so nothing is in
/// flight at shutdown.
pub(crate) fn shutdown() {}

// TODO: handle toast activation so a click raises the right tab on Windows.
pub(crate) fn notify(
    title: String,
    body: String,
    _on_activate: Option<super::ActivateHandler>,
) -> Handle {
    let shared = Arc::new(Mutex::new(Shared::default()));
    let thread_shared = Arc::clone(&shared);
    std::thread::spawn(move || {
        let Some((notifier, toast)) = build(&title, &body) else {
            return;
        };
        let _ = notifier.Show(&toast);

        // A withdraw may have raced ahead of the show; honor it now,
        // otherwise hand the toast to the handle for a later `close`.
        let mut shared = thread_shared.lock().unwrap();
        if shared.withdraw_pending {
            let _ = notifier.Hide(&toast);
        } else {
            shared.toast = Some((notifier, toast));
        }
    });
    Handle { shared }
}

fn build(title: &str, body: &str) -> Option<(ToastNotifier, ToastNotification)> {
    use windows::core::HSTRING;
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::UI::Notifications::ToastNotificationManager;

    let xml = XmlDocument::new().ok()?;
    let toast_xml = format!(
        r#"<toast><visual><binding template="ToastGeneric"><text>{}</text><text>{}</text></binding></visual></toast>"#,
        title, body,
    );
    xml.LoadXml(&HSTRING::from(&toast_xml)).ok()?;
    let toast = ToastNotification::CreateToastNotification(&xml).ok()?;
    let notifier =
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from("Rio"))
            .ok()?;
    Some((notifier, toast))
}
