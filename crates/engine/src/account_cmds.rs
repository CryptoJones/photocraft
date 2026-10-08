//! App-owned ChatGPT accounts. Commands work without a document, run off the UI thread, and
//! expose public account status only. Neither tokens nor browser authorization URLs are results.
use std::sync::Arc;

pub use photocraft_chatgpt::{Account, AccountStatus, Accounts, MANAGE_USAGE};
use serde_json::{Value, json};

use crate::commands::CommandSpec;
use crate::{EngineError, Result, Session};

fn error(e: photocraft_chatgpt::Error) -> EngineError {
    match e {
        photocraft_chatgpt::Error::Cancelled => EngineError::Cancelled,
        e => EngineError::Other(e.to_string()),
    }
}

fn backend(s: &Session) -> Result<Arc<dyn Accounts>> {
    s.chatgpt_accounts
        .clone()
        .ok_or_else(|| EngineError::Other("ChatGPT sign-in is unavailable in this build; build the desktop app with --features chatgpt".into()))
}

fn available(s: &Session) -> std::result::Result<(), String> {
    if s.jobs().iter().any(|j| j.command.starts_with("account.chatgpt.")) {
        return Err("a ChatGPT account operation is already running".into());
    }
    backend(s).map(|_| ()).map_err(|e| e.to_string())
}

fn connected(s: &Session) -> std::result::Result<(), String> {
    available(s)?;
    s.chatgpt_status().active.map(|_| ()).ok_or_else(|| "Continue with ChatGPT first".into())
}

fn account_param(p: &Value, cmd: &str) -> Result<String> {
    p.get("account")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty() && id.len() <= 256)
        .map(str::to_owned)
        .ok_or_else(|| EngineError::BadParams { cmd: cmd.into(), msg: "account must be a nonempty saved account ID".into() })
}

impl Session {
    pub fn chatgpt_status(&self) -> AccountStatus {
        self.chatgpt_accounts.as_ref().map(|b| b.status()).unwrap_or_default()
    }

    #[cfg(all(feature = "chatgpt", not(target_arch = "wasm32")))]
    pub fn configure_chatgpt(&mut self, directory: std::path::PathBuf) -> Result<()> {
        self.chatgpt_accounts = Some(Arc::new(photocraft_chatgpt::NativeAccounts::new(directory).map_err(error)?));
        Ok(())
    }
}

fn sign_in(s: &mut Session, p: &Value) -> Result<Value> {
    if p.as_object().is_some_and(|o| !o.is_empty()) || (!p.is_null() && !p.is_object()) {
        return Err(EngineError::BadParams {
            cmd: "account.chatgpt.signIn".into(),
            msg: "signIn accepts no parameters; select a saved account with account.chatgpt.select".into(),
        });
    }
    let service = backend(s)?;
    crate::jobs::run(
        s,
        "Sign in with ChatGPT",
        false,
        move |ctx| ctx.stage(0.0, 1.0, "Finish sign-in in your browser", |ctl| service.sign_in(None, ctl)).map_err(error),
        |_, status| Ok(json!(status)),
    )
}

fn select(s: &mut Session, p: &Value) -> Result<Value> {
    let id = account_param(p, "account.chatgpt.select")?;
    let service = backend(s)?;
    if !service.status().accounts.iter().any(|a| a.id == id) {
        return Err(EngineError::Other("that saved ChatGPT account is missing".into()));
    }
    crate::jobs::run(
        s,
        "Switch ChatGPT account",
        false,
        move |ctx| ctx.stage(0.0, 1.0, "Confirm the selected account in your browser", |ctl| service.select(&id, ctl)).map_err(error),
        |_, status| Ok(json!(status)),
    )
}

fn refresh(s: &mut Session, _: &Value) -> Result<Value> {
    let service = backend(s)?;
    crate::jobs::run(
        s,
        "Refresh ChatGPT connection",
        false,
        move |ctx| ctx.stage(0.0, 1.0, "Checking ChatGPT connection", |ctl| service.refresh(ctl)).map_err(error),
        |_, status| Ok(json!(status)),
    )
}

fn sign_out(s: &mut Session, _: &Value) -> Result<Value> {
    let service = backend(s)?;
    crate::jobs::run(
        s,
        "Sign out of ChatGPT",
        false,
        move |ctx| {
            ctx.stage(0.0, 1.0, "Ending the ChatGPT session", |ctl| {
                let revoked = service.sign_out(ctl).map_err(error)?;
                Ok((revoked, service.status()))
            })
        },
        |_, (revoked, status)| {
            Ok(json!({"status":status,"remoteRevocationConfirmed":revoked,
            "message":if revoked { "Signed out of ChatGPT." } else { "Signed out locally. Remote revocation was not confirmed; disconnect PhotoCraft in ChatGPT Settings." }}))
        },
    )
}

fn welcome(s: &mut Session, p: &Value) -> Result<Value> {
    let id = account_param(p, "account.chatgpt.acknowledgeWelcome")?;
    backend(s)?.acknowledge_welcome(&id).map_err(error)?;
    Ok(json!(s.chatgpt_status()))
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "account.chatgpt.status",
            label: "ChatGPT account status",
            menu: &[],
            shortcut: None,
            params: "{} → {active,accounts,signInAvailable}; no credentials",
            enabled: |_| Ok(()),
            run: |s, _| Ok(json!(s.chatgpt_status())),
            journal: false,
        },
        CommandSpec {
            id: "account.chatgpt.signIn",
            label: "Continue with ChatGPT",
            menu: &[],
            shortcut: None,
            params: "{} → public account status; opens the system browser; cancellable; no document edit",
            enabled: available,
            run: sign_in,
            journal: false,
        },
        CommandSpec {
            id: "account.chatgpt.select",
            label: "Switch ChatGPT account",
            menu: &[],
            shortcut: None,
            params: "{account:savedID} → public account status; requires browser reauthorization",
            enabled: available,
            run: select,
            journal: false,
        },
        CommandSpec {
            id: "account.chatgpt.refresh",
            label: "Refresh ChatGPT connection",
            menu: &[],
            shortcut: None,
            params: "{} → public account status; renews only near token expiry",
            enabled: connected,
            run: refresh,
            journal: false,
        },
        CommandSpec {
            id: "account.chatgpt.signOut",
            label: "Sign out of ChatGPT",
            menu: &[],
            shortcut: None,
            params: "{} → {status,remoteRevocationConfirmed,message}; removes local tokens after revocation attempt",
            enabled: connected,
            run: sign_out,
            journal: false,
        },
        CommandSpec {
            id: "account.chatgpt.acknowledgeWelcome",
            label: "Dismiss ChatGPT welcome",
            menu: &[],
            shortcut: None,
            params: "{account:savedID} → public account status; remembers dismissal",
            enabled: available,
            run: welcome,
            journal: false,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use photocraft_raster::Interrupt;
    use std::sync::{Mutex, PoisonError};

    struct Fake(Mutex<AccountStatus>);
    impl Fake {
        fn new() -> Self {
            Self(Mutex::new(AccountStatus {
                sign_in_available: true,
                accounts: vec![Account { id: "saved".into(), label: "Test account".into(), ..Default::default() }],
                ..Default::default()
            }))
        }
    }
    impl Accounts for Fake {
        fn status(&self) -> AccountStatus {
            self.0.lock().unwrap_or_else(PoisonError::into_inner).clone()
        }
        fn sign_in(&self, _: Option<&str>, ctl: &Interrupt<'_>) -> photocraft_chatgpt::Result<AccountStatus> {
            ctl.check().map_err(|_| photocraft_chatgpt::Error::Cancelled)?;
            let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
            state.active = Some("saved".into());
            state.accounts[0].connected = true;
            state.accounts[0].plan_usage = true;
            Ok(state.clone())
        }
        fn select(&self, _: &str, ctl: &Interrupt<'_>) -> photocraft_chatgpt::Result<AccountStatus> {
            self.sign_in(None, ctl)
        }
        fn refresh(&self, _: &Interrupt<'_>) -> photocraft_chatgpt::Result<AccountStatus> {
            Ok(self.status())
        }
        fn sign_out(&self, _: &Interrupt<'_>) -> photocraft_chatgpt::Result<bool> {
            let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
            state.active = None;
            state.accounts[0].connected = false;
            Ok(false)
        }
        fn acknowledge_welcome(&self, _: &str) -> photocraft_chatgpt::Result<()> {
            self.0.lock().unwrap_or_else(PoisonError::into_inner).accounts[0].welcomed = true;
            Ok(())
        }
    }

    #[test]
    fn default_sessions_never_read_credentials_and_invalid_requests_fail() {
        let mut s = Session::new();
        assert_eq!(s.execute("account.chatgpt.status", json!({})).unwrap()["signInAvailable"], false);
        for cmd in ["signIn", "select", "refresh", "signOut", "acknowledgeWelcome"] {
            assert!(s.execute(&format!("account.chatgpt.{cmd}"), json!({})).is_err());
        }
        s.chatgpt_accounts = Some(Arc::new(Fake::new()));
        for p in [json!({}), json!({"account":0}), json!({"account":""}), json!({"account":"unknown"}), json!({"account":"x".repeat(257)})] {
            assert!(s.execute("account.chatgpt.select", p).is_err());
        }
        assert!(s.execute("account.chatgpt.signIn", json!({"accessToken":"untrusted"})).is_err());
    }

    #[test]
    fn accounts_work_without_a_document_and_keep_document_history_and_floating_pixels() {
        let mut s = Session::new();
        s.chatgpt_accounts = Some(Arc::new(Fake::new()));
        assert_eq!(s.execute("account.chatgpt.signIn", json!({})).unwrap()["active"], "saved");
        s.execute("file.new", json!({"width":16,"height":16})).unwrap();
        s.execute("layer.new.layer", json!({"name":"Photo"})).unwrap();
        s.execute("select.rect", json!({"x":2,"y":2,"width":8,"height":8})).unwrap();
        s.execute("select.float", json!({"dx":1,"dy":1})).unwrap();
        let before = s.active().unwrap().history.past_len();
        s.execute("account.chatgpt.refresh", json!({})).unwrap();
        s.execute("account.chatgpt.acknowledgeWelcome", json!({"account":"saved"})).unwrap();
        assert!(s.chatgpt_status().accounts[0].welcomed);
        let out = s.execute("account.chatgpt.signOut", json!({})).unwrap();
        assert_eq!(out["remoteRevocationConfirmed"], false);
        assert!(out["message"].as_str().unwrap().contains("not confirmed"));
        assert_eq!(s.active().unwrap().history.past_len(), before);
        assert!(s.active().unwrap().floating.is_some());
    }

    #[test]
    fn sign_in_is_a_document_independent_background_job() {
        let mut s = Session::new();
        s.chatgpt_accounts = Some(Arc::new(Fake::new()));
        let r = s.start("account.chatgpt.signIn", json!({})).unwrap();
        let crate::jobs::Started::Job(id) = r else { panic!("expected a background job") };
        assert!(s.job(id).unwrap().document.is_none());
        assert_eq!(s.wait_job(id).unwrap()["active"], "saved");
    }
}
