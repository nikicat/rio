use futures_lite::StreamExt;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};

const DEST: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";

/// Withdraws that have been signalled to a notifier thread but may not yet
/// have reached the daemon. [`shutdown`] drains this so a `CloseNotification`
/// requested right before quit isn't lost when the process exits and kills
/// the notifier threads.
static IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);

/// Handle to a posted notification. Owns one end of a cancel channel; the
/// notifier thread owns the D-Bus connection and the other end and performs
/// the actual withdraw. The UI thread therefore never touches D-Bus.
pub(crate) struct Handle {
    cancel: async_channel::Sender<()>,
}

impl Handle {
    pub(crate) fn close(self) {
        // Just signal the owning thread; it withdraws on its own connection.
        // `try_send` fails only if that thread already exited (notification
        // already clicked/dismissed), so there is then nothing to withdraw.
        if self.cancel.try_send(()).is_ok() {
            IN_FLIGHT.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Wait briefly for in-flight withdraws to reach the daemon before the
/// process exits. Bounded, so a count left dangling by a rare cancel/dismiss
/// race can't hang quit.
pub(crate) fn shutdown() {
    for _ in 0..50 {
        if IN_FLIGHT.load(Ordering::Relaxed) == 0 {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

pub(crate) fn notify(
    title: String,
    body: String,
    on_activate: Option<super::ActivateHandler>,
) -> Handle {
    // Capacity 1: a single `close` is all that can ever be sent (the handle
    // is consumed), and it must never block the UI thread.
    let (cancel_tx, cancel_rx) = async_channel::bounded(1);
    std::thread::spawn(move || {
        zbus::block_on(run(title, body, on_activate, cancel_rx))
    });
    Handle { cancel: cancel_tx }
}

/// One signal value the notifier loop waits on: either the daemon emitted a
/// D-Bus signal, or the pane asked us to withdraw.
enum Wakeup {
    Signal(Option<zbus::Result<zbus::Message>>),
    Cancelled,
}

async fn run(
    title: String,
    body: String,
    on_activate: Option<super::ActivateHandler>,
    cancel_rx: async_channel::Receiver<()>,
) {
    let connection = match zbus::Connection::session().await {
        Ok(connection) => connection,
        Err(err) => {
            tracing::warn!("notifier: no session bus: {err}");
            return;
        }
    };
    let proxy = match zbus::Proxy::new(&connection, DEST, PATH, DEST).await {
        Ok(proxy) => proxy,
        Err(err) => {
            tracing::warn!("notifier: could not build notifications proxy: {err}");
            return;
        }
    };
    let hints: HashMap<&str, zbus::zvariant::Value<'_>> = HashMap::new();

    // Without a click handler there is nothing to wait on and nothing to
    // withdraw on focus (these are app-driven OSC notifications): post and
    // exit. Dropping `cancel_rx` here also makes a stray `close` a no-op.
    let Some(on_activate) = on_activate else {
        let _ = proxy
            .call::<_, _, u32>(
                "Notify",
                &(
                    "Rio",
                    0u32,
                    "rio",
                    title.as_str(),
                    body.as_str(),
                    &[] as &[&str], // actions
                    &hints,
                    -1i32, // expire_timeout
                ),
            )
            .await;
        return;
    };

    // Subscribe to the daemon's signals *before* posting, so a fast click
    // can't slip through the gap between `Notify` returning and the match
    // rule being installed. If we can't subscribe, fall back to a plain
    // notification rather than dropping it.
    let Some(mut signals) = subscribe(&connection).await else {
        tracing::warn!(
            "notifier: could not subscribe to daemon signals; fire-and-forget"
        );
        let _ = proxy
            .call::<_, _, u32>(
                "Notify",
                &(
                    "Rio",
                    0u32,
                    "rio",
                    title.as_str(),
                    body.as_str(),
                    &[] as &[&str],
                    &hints,
                    -1i32,
                ),
            )
            .await;
        return;
    };

    // "default" is the special action a notifier invokes when the body
    // (not a button) is clicked; the label is only shown by daemons that
    // surface it as an explicit button.
    let id: u32 = match proxy
        .call(
            "Notify",
            &(
                "Rio",
                0u32,
                "rio",
                title.as_str(),
                body.as_str(),
                &["default", "Open"] as &[&str],
                &hints,
                -1i32,
            ),
        )
        .await
    {
        Ok(id) => id,
        Err(err) => {
            tracing::warn!("notifier: Notify call failed: {err}");
            return;
        }
    };

    // Wait on two things at once: the daemon's signals, and a withdraw
    // request from the pane (focus or close). On a withdraw we issue
    // `CloseNotification` over *this* thread's connection; on a click we run
    // the activate callback; on dismiss/expire we just exit. The daemon
    // emits `ActivationToken` (an xdg-activation token for raising our
    // window) just before `ActionInvoked`, so we stash the most recent one.
    let mut pending_token: Option<String> = None;
    let mut on_activate = Some(on_activate);
    loop {
        let signal = async { Wakeup::Signal(signals.next().await) };
        let cancel = async {
            match cancel_rx.recv().await {
                Ok(()) => Wakeup::Cancelled,
                // Handle dropped without a `close`: keep watching signals
                // only (this future never resolves again).
                Err(_) => std::future::pending::<Wakeup>().await,
            }
        };

        match futures_lite::future::or(signal, cancel).await {
            Wakeup::Cancelled => {
                let _ = proxy.call_noreply("CloseNotification", &(id,)).await;
                IN_FLIGHT.fetch_sub(1, Ordering::Relaxed);
                break;
            }
            Wakeup::Signal(None) => break,
            Wakeup::Signal(Some(Err(_))) => continue,
            Wakeup::Signal(Some(Ok(msg))) => {
                let header = msg.header();
                let Some(member) = header.member() else {
                    continue;
                };
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
                                if let Some(callback) = on_activate.take() {
                                    callback(pending_token.take());
                                }
                                break;
                            }
                        }
                    }
                    "NotificationClosed" => {
                        if let Ok((nid, _reason)) =
                            msg.body().deserialize::<(u32, u32)>()
                        {
                            if nid == id {
                                break;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    // If a withdraw was signalled but we exited via a click/dismiss first,
    // the notification is already gone — balance the in-flight count so
    // `shutdown` doesn't wait on a withdraw that will never happen.
    while cancel_rx.try_recv().is_ok() {
        IN_FLIGHT.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Start buffering `ActionInvoked` / `NotificationClosed` signals from the
/// notification daemon. Returns `None` if the match rule can't be built or
/// installed.
async fn subscribe(connection: &zbus::Connection) -> Option<zbus::MessageStream> {
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface(DEST)
        .ok()?
        .path(PATH)
        .ok()?
        .build();
    zbus::MessageStream::for_match_rule(rule, connection, Some(16))
        .await
        .ok()
}
