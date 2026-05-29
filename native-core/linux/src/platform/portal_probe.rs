#[cfg(not(test))]
use anyhow::Context;
#[cfg(not(test))]
use ashpd::desktop::global_shortcuts::GlobalShortcuts;

#[cfg(not(test))]
const GLOBAL_SHORTCUTS_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

pub fn wayland_global_shortcuts_available() -> bool {
    probe_global_shortcuts_portal().is_ok()
}

#[cfg(all(target_os = "linux", not(test)))]
fn probe_global_shortcuts_portal() -> anyhow::Result<()> {
    let (tx, rx) = std::sync::mpsc::channel();
    let _handle = std::thread::Builder::new()
        .name("Parrot Wayland Global Shortcuts Probe".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;

            let result = runtime.block_on(async {
                tokio::time::timeout(GLOBAL_SHORTCUTS_PROBE_TIMEOUT, GlobalShortcuts::new())
                    .await
                    .map_err(|_| anyhow::anyhow!("global shortcuts portal probe timed out"))?
                    .context("global shortcuts portal is unavailable")?;
                Ok::<(), anyhow::Error>(())
            });
            let _ = tx.send(result);
            Ok::<(), anyhow::Error>(())
        })
        .context("failed to start Wayland global shortcuts probe thread")?;

    match rx.recv_timeout(GLOBAL_SHORTCUTS_PROBE_TIMEOUT) {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            Err(anyhow::anyhow!("global shortcuts portal probe timed out"))
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            Err(anyhow::anyhow!("global shortcuts portal probe thread stopped"))
        }
    }
}

#[cfg(any(not(target_os = "linux"), test))]
fn probe_global_shortcuts_portal() -> anyhow::Result<()> {
    Ok(())
}
