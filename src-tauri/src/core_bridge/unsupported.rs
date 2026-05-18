use anyhow::anyhow;
use parrot_protocol::NativeCoreMethod;
use serde_json::Value;
use tauri::AppHandle;

#[derive(Clone)]
pub struct CoreBridge {
    app: AppHandle,
}

impl CoreBridge {
    pub async fn spawn(app: AppHandle) -> anyhow::Result<Self> {
        Ok(Self { app })
    }

    pub fn app(&self) -> &AppHandle {
        &self.app
    }

    pub async fn reconnect(&self) -> anyhow::Result<()> {
        Ok(())
    }

    pub async fn request(
        &self,
        method: NativeCoreMethod,
        _payload: Value,
    ) -> anyhow::Result<Value> {
        Err(anyhow!(
            "Native core sidecar is not implemented for this platform yet: {}",
            method.as_str()
        ))
    }
}

pub fn is_native_core_disconnect(_error: &anyhow::Error) -> bool {
    false
}
