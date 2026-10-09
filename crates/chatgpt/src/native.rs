//! Official open-source Sign in with ChatGPT flow. Credentials are app-owned, protected
//! local files; no Codex/ChatGPT cookie or another app's OAuth client is reused.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError, TryLockError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header, jwk::JwkSet};
use photocraft_raster::Interrupt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;

use crate::{Account, AccountStatus, Accounts, Error, Result, check};

const ISSUER: &str = "https://auth.openai.com";
const AUTHORIZE: &str = "https://auth.openai.com/api/accounts/authorize";
const TOKEN: &str = "https://auth.openai.com/api/accounts/oauth/token";
const RESOURCE: &str = "https://api.openai.com/v1";
const DYNAMIC_CLIENT: &str = "dynamic_agent_client";
const SCOPES: &str = "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
const MAX_JSON: u64 = 2 * 1024 * 1024;
#[cfg(not(test))]
const CALLBACK_WAIT: Duration = Duration::from_secs(300);
#[cfg(test)]
const CALLBACK_WAIT: Duration = Duration::from_secs(5);

#[derive(Clone, Serialize, Deserialize)]
struct Credentials {
    access_token: String,
    refresh_token: Option<String>,
    id_token: String,
    scopes: Vec<String>,
    expires_at: u64,
    earliest_refresh_at: Option<u64>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Registration {
    account: Account,
    client_id: String,
    subject: String,
    tokens: Option<Credentials>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct Store {
    host_id: String,
    active: Option<String>,
    registrations: Vec<Registration>,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    id_token: Option<String>,
    token_type: String,
    expires_in: u64,
    scope: Option<String>,
    earliest_refresh_at: Option<u64>,
}

#[derive(Clone, Deserialize)]
struct Identity {
    sub: String,
    nonce: Option<String>,
    email: Option<String>,
    name: Option<String>,
    aud: serde_json::Value,
    azp: Option<String>,
}

/// Deliberately not Debug: private token records and authorization URL hints must never leak
/// through diagnostics. The public status below is the only serializable app-facing state.
pub struct NativeAccounts {
    root: PathBuf,
    store: Mutex<Store>,
    operation: Mutex<()>,
    http: ureq::Agent,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

fn random() -> Result<String> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| Error::Auth("secure randomness is unavailable".into()))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn host_id() -> Result<String> {
    let mut b = [0_u8; 16];
    getrandom::fill(&mut b).map_err(|_| Error::Auth("secure randomness is unavailable".into()))?;
    if let Some(v) = b.get_mut(6) {
        *v = (*v & 0x0f) | 0x40;
    }
    if let Some(v) = b.get_mut(8) {
        *v = (*v & 0x3f) | 0x80;
    }
    let hex: String = b.iter().map(|v| format!("{v:02x}")).collect();
    let parts = [(0, 8), (8, 12), (12, 16), (16, 20), (20, 32)].into_iter().filter_map(|(a, z)| hex.get(a..z)).collect::<Vec<_>>();
    Ok(format!("urn:uuid:{}", parts.join("-")))
}

fn same_secret(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes().zip(b.bytes()).fold(0_u8, |diff, (a, b)| diff | (a ^ b)) == 0
}

fn pkce(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn protect(path: &Path, directory: bool) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(if directory { 0o700 } else { 0o600 }))?;
    }
    #[cfg(windows)]
    {
        use std::os::windows::{ffi::OsStrExt, process::CommandExt};
        // Replace the DACL, rather than merely removing inherited rules while leaving an
        // existing Everyone grant. Windows PowerShell exposes the OS ACL APIs safely; no
        // unsafe Rust or extra runtime installation is needed. The SID handles Unicode names.
        let system = std::env::var_os("SystemRoot").ok_or_else(|| Error::Auth("Windows system directory is unavailable".into()))?;
        let executable = PathBuf::from(system).join("System32/WindowsPowerShell/v1.0/powershell.exe");
        let path_bytes: Vec<u8> = path.as_os_str().encode_wide().flat_map(u16::to_le_bytes).collect();
        let encoded_path = base64::engine::general_purpose::STANDARD.encode(path_bytes);
        let kind = if directory { "Directory" } else { "File" };
        let inheritance = if directory { 3 } else { 0 };
        let script = format!(
            r#"
$ErrorActionPreference = 'Stop'
$target = [Text.Encoding]::Unicode.GetString([Convert]::FromBase64String('{encoded_path}'))
$sid = [Security.Principal.WindowsIdentity]::GetCurrent().User
$acl = New-Object System.Security.AccessControl.{kind}Security
$acl.SetAccessRuleProtection($true, $false)
$acl.SetOwner($sid)
$rule = [Security.AccessControl.FileSystemAccessRule]::new($sid, [Security.AccessControl.FileSystemRights]::FullControl, [Security.AccessControl.InheritanceFlags]{inheritance}, [Security.AccessControl.PropagationFlags]::None, [Security.AccessControl.AccessControlType]::Allow)
$acl.AddAccessRule($rule)
[IO.{kind}]::SetAccessControl($target, $acl)
"#
        );
        let encoded_script = base64::engine::general_purpose::STANDARD.encode(script.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<_>>());
        let result = std::process::Command::new(executable)
            .creation_flags(0x08000000)
            .args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &encoded_script])
            .output()?;
        if !result.status.success() {
            return Err(Error::Auth("could not protect Windows account storage".into()));
        }
    }

    Ok(())
}

fn read_store(path: &Path) -> Result<Option<Store>> {
    if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(Error::Auth("account storage must not be a symbolic link".into()));
    }
    let mut f = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    protect(path, false)?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut f).take(MAX_JSON + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_JSON {
        return Err(Error::Auth("account storage exceeds its size limit".into()));
    }
    serde_json::from_slice(&bytes).map(Some).map_err(|_| Error::Auth("account storage is invalid; it has been preserved".into()))
}

fn save_store(root: &Path, store: &Store) -> Result<()> {
    let bytes = serde_json::to_vec(store).map_err(|_| Error::Auth("could not encode account storage".into()))?;
    if bytes.len() as u64 > MAX_JSON {
        return Err(Error::Auth("too many account registrations".into()));
    }
    let mut file = tempfile::NamedTempFile::new_in(root)?;
    protect(file.path(), false)?;
    file.write_all(&bytes)?;
    file.as_file().sync_all()?;
    file.persist(root.join("accounts.json")).map_err(|e| Error::Storage(e.error))?;
    #[cfg(unix)]
    File::open(root)?.sync_all()?;
    Ok(())
}

fn lock_root(root: &Path) -> Result<File> {
    let path = root.join("account-operation.lock");
    if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(Error::Auth("account lock must not be a symbolic link".into()));
    }
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(&path)?;
    protect(&path, false)?;
    file.try_lock().map_err(|_| Error::Auth("another PhotoCraft window is updating this account; try again when it finishes".into()))?;
    Ok(file)
}

fn status(store: &Store) -> AccountStatus {
    let accounts = store
        .registrations
        .iter()
        .map(|r| {
            let mut account = r.account.clone();
            account.connected = r.tokens.is_some();
            account.plan_usage = r.tokens.as_ref().is_some_and(|t| t.scopes.iter().any(|s| s == "chatgpt.tokens.use.direct"));
            account
        })
        .collect();
    AccountStatus { active: store.active.clone(), accounts, sign_in_available: true }
}

impl NativeAccounts {
    pub fn new(root: PathBuf) -> Result<Self> {
        if fs::symlink_metadata(&root).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(Error::Auth("account storage directory must not be a symbolic link".into()));
        }
        fs::create_dir_all(&root)?;
        protect(&root, true)?;
        let _initialization = lock_root(&root)?;
        let store = match read_store(&root.join("accounts.json"))? {
            Some(store) => store,
            None => {
                let store = Store { host_id: host_id()?, ..Default::default() };
                save_store(&root, &store)?;
                store
            }
        };
        let http = ureq::Agent::config_builder()
            .https_only(true)
            .max_redirects(0)
            .timeout_global(Some(Duration::from_secs(30)))
            .http_status_as_error(false)
            .user_agent("PhotoCraft-dev")
            .build()
            .into();
        Ok(Self { root, store: Mutex::new(store), operation: Mutex::new(()), http })
    }

    /// A process lock serializes rotating-token operations across two app windows/processes.
    /// Callers return a useful busy error instead of racing another refresh or sign-in.
    fn lock(&self) -> Result<File> {
        lock_root(&self.root)
    }

    fn operation(&self) -> Result<MutexGuard<'_, ()>> {
        match self.operation.try_lock() {
            Ok(lock) => Ok(lock),
            Err(TryLockError::Poisoned(e)) => Ok(e.into_inner()),
            Err(TryLockError::WouldBlock) => Err(Error::Auth("another account operation is running".into())),
        }
    }

    fn load(&self) -> Result<Store> {
        read_store(&self.root.join("accounts.json"))?.ok_or_else(|| Error::Auth("account storage is missing".into()))
    }

    fn save(&self, store: Store) -> Result<()> {
        save_store(&self.root, &store)?;
        *self.store.lock().unwrap_or_else(PoisonError::into_inner) = store;
        Ok(())
    }

    fn json<T: for<'a> Deserialize<'a>>(&self, mut response: ureq::http::Response<ureq::Body>) -> Result<T> {
        if !response.status().is_success() {
            return Err(Error::Auth(format!("OpenAI request failed (HTTP {}); credentials were not replaced", response.status().as_u16())));
        }
        let bytes = response.body_mut().with_config().limit(MAX_JSON).read_to_vec().map_err(|_| Error::Auth("could not read OpenAI's response".into()))?;
        serde_json::from_slice(&bytes).map_err(|_| Error::Auth("OpenAI returned an invalid response".into()))
    }

    fn token(&self, fields: &[(&str, &str)]) -> Result<TokenResponse> {
        let response = self.http.post(TOKEN).send_form(fields.iter().copied()).map_err(|_| Error::Auth("could not reach OpenAI's token endpoint".into()))?;
        self.json(response)
    }

    fn identity(&self, token: &str, client: &str, nonce: &str) -> Result<Identity> {
        let response =
            self.http.get("https://auth.openai.com/.well-known/jwks.json").call().map_err(|_| Error::Auth("could not fetch OpenAI's signing keys".into()))?;
        validate_identity(token, client, Some(nonce), &self.json(response)?)
    }

    fn renew_with(&self, ctl: &Interrupt<'_>, exchange: impl FnOnce(&[(&str, &str)]) -> Result<TokenResponse>) -> Result<AccountStatus> {
        check(ctl)?;
        let _operation = self.operation()?;
        let _lock = self.lock()?;
        let mut store = self.load()?;
        let id = store.active.clone().ok_or_else(|| Error::Auth("Continue with ChatGPT first".into()))?;
        let registration = store.registrations.iter_mut().find(|r| r.account.id == id).ok_or_else(|| Error::Auth("the selected account is missing".into()))?;
        let tokens = registration.tokens.as_ref().ok_or_else(|| Error::Auth("sign in to this account again".into()))?;
        if tokens.expires_at > now().saturating_add(120) {
            return Ok(status(&store));
        }
        if tokens.earliest_refresh_at.is_some_and(|t| now() < t) {
            if tokens.expires_at <= now() {
                return Err(Error::Auth("token renewal is not available yet; sign in again".into()));
            }
            return Ok(status(&store));
        }
        let refresh = tokens.refresh_token.as_deref().ok_or_else(|| Error::Auth("sign in again to renew this account".into()))?;
        let response =
            exchange(&[("grant_type", "refresh_token"), ("client_id", &registration.client_id), ("refresh_token", refresh), ("resource", RESOURCE)])?;
        // Save rotated refresh tokens even if the caller is cancelled. Never retry a
        // successful refresh with the old rotating credential.
        let replacement = credentials(response, Some(tokens))?;
        registration.tokens = Some(replacement);
        let result = status(&store);
        self.save(store)?;
        check(ctl)?;
        Ok(result)
    }
}

impl NativeAccounts {
    fn sign_in_with(
        &self,
        account: Option<&str>,
        ctl: &Interrupt<'_>,
        launch: impl FnOnce(&Url) -> Result<()>,
        exchange: impl FnOnce(&[(&str, &str)]) -> Result<TokenResponse>,
        verify: impl FnOnce(&str, &str, &str) -> Result<Identity>,
    ) -> Result<AccountStatus> {
        check(ctl)?;
        let _operation = self.operation()?;
        let _lock = self.lock()?;
        let mut store = self.load()?;
        let previous = account
            .map(|id| store.registrations.iter().find(|r| r.account.id == id).ok_or_else(|| Error::Auth("the selected account registration is missing".into())))
            .transpose()?
            .cloned();
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
        let redirect = format!("http://127.0.0.1:{}/auth/callback", listener.local_addr()?.port());
        let (state, nonce, verifier) = (random()?, random()?, random()?);
        let url = authorization_url(&store.host_id, previous.as_ref(), &redirect, &state, &nonce, &verifier)?;
        ctl.progress(0.1);
        launch(&url)?;
        let (code, client_id) = wait_callback(&listener, &state, previous.as_ref().map(|r| r.client_id.as_str()), ctl)?;
        check(ctl)?;
        ctl.progress(0.5);
        let response = exchange(&[
            ("grant_type", "authorization_code"),
            ("client_id", &client_id),
            ("code", &code),
            ("code_verifier", &verifier),
            ("redirect_uri", &redirect),
            ("resource", RESOURCE),
        ])?;
        let id_token = response.id_token.as_deref().ok_or_else(|| Error::Auth("OpenAI returned no ID token".into()))?;
        let identity = verify(id_token, &client_id, &nonce)?;
        if previous.as_ref().is_some_and(|r| r.subject != identity.sub || r.client_id != client_id) {
            return Err(Error::Auth("the account identity changed; the current registration was not replaced".into()));
        }
        check(ctl)?;
        let tokens = credentials(response, None)?;
        let id = URL_SAFE_NO_PAD.encode(Sha256::digest(format!("{ISSUER}:{client_id}:{}", identity.sub)));
        let name = identity.name.filter(|n| !n.is_empty()).or_else(|| identity.email.clone()).unwrap_or_else(|| "ChatGPT account".into());
        let label = format!("{name} ({})", id.get(..6).unwrap_or("account"));
        let welcomed = store.registrations.iter().find(|r| r.account.id == id).is_some_and(|r| r.account.welcomed);
        let registration = Registration {
            account: Account {
                id: id.clone(),
                label,
                email: identity.email,
                connected: true,
                plan_usage: tokens.scopes.iter().any(|s| s == "chatgpt.tokens.use.direct"),
                welcomed,
            },
            client_id,
            subject: identity.sub,
            tokens: Some(tokens),
        };
        if let Some(saved) = store.registrations.iter_mut().find(|r| r.account.id == id) {
            *saved = registration;
        } else {
            store.registrations.push(registration);
        }
        store.active = Some(id);
        let result = status(&store);
        self.save(store)?;
        ctl.progress(1.0);
        Ok(result)
    }
}

impl NativeAccounts {
    fn revoke(&self, client_id: &str, refresh: &str, ctl: &Interrupt<'_>) -> bool {
        #[derive(Deserialize)]
        struct Discovery {
            issuer: String,
            revocation_endpoint: Option<String>,
        }
        let discovery = self.http.get("https://auth.openai.com/.well-known/openid-configuration").call().ok().and_then(|r| self.json::<Discovery>(r).ok());
        let endpoint = discovery.filter(|d| d.issuer == ISSUER).and_then(|d| d.revocation_endpoint).filter(|s| {
            Url::parse(s).is_ok_and(|u| {
                u.scheme() == "https"
                    && u.host_str() == Some("auth.openai.com")
                    && u.port_or_known_default() == Some(443)
                    && u.username().is_empty()
                    && u.password().is_none()
            })
        });
        let Some(endpoint) = endpoint else { return false };
        for attempt in 0..2 {
            if ctl.cancelled() {
                return false;
            }
            let result = self.http.post(&endpoint).send_form([("token", refresh), ("token_type_hint", "refresh_token"), ("client_id", client_id)]);
            if result.is_ok_and(|r| r.status().as_u16() == 200) {
                return true;
            }
            if attempt == 0 {
                std::thread::sleep(Duration::from_millis(250));
            }
        }
        false
    }

    fn sign_out_with(&self, ctl: &Interrupt<'_>, revoke: impl FnOnce(&str, &str, &Interrupt<'_>) -> bool) -> Result<bool> {
        check(ctl)?;
        let _operation = self.operation()?;
        let _lock = self.lock()?;
        let mut store = self.load()?;
        let Some(active) = &store.active else { return Ok(true) };
        let registration =
            store.registrations.iter_mut().find(|r| &r.account.id == active).ok_or_else(|| Error::Auth("the selected account is missing".into()))?;
        let revoked = registration.tokens.as_ref().and_then(|t| t.refresh_token.as_deref()).is_none_or(|refresh| revoke(&registration.client_id, refresh, ctl));
        registration.tokens = None;
        registration.account.connected = false;
        registration.account.plan_usage = false;
        store.active = None;
        self.save(store)?;
        Ok(revoked)
    }
}

fn validate_identity(token: &str, client: &str, nonce: Option<&str>, jwks: &JwkSet) -> Result<Identity> {
    let header = decode_header(token).map_err(|_| Error::Auth("the ID token is invalid".into()))?;
    if !matches!(header.alg, Algorithm::RS256 | Algorithm::RS384 | Algorithm::RS512 | Algorithm::ES256 | Algorithm::ES384) {
        return Err(Error::Auth("the ID token uses an unsupported signing algorithm".into()));
    }
    let kid = header.kid.ok_or_else(|| Error::Auth("the ID token has no signing-key ID".into()))?;
    let jwk = jwks.find(&kid).ok_or_else(|| Error::Auth("the ID token signing key is unknown".into()))?;
    let key = DecodingKey::from_jwk(jwk).map_err(|_| Error::Auth("invalid OpenAI signing key".into()))?;
    let mut validation = Validation::new(header.alg);
    validation.set_audience(&[client]);
    validation.set_issuer(&[ISSUER]);
    validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
    validation.validate_nbf = true;
    let claims = decode::<Identity>(token, &key, &validation)
        .map_err(|_| Error::Auth("ID-token signature, issuer, audience or expiration validation failed".into()))?
        .claims;
    if claims.sub.is_empty() || nonce.is_some_and(|expected| !claims.nonce.as_deref().is_some_and(|n| same_secret(n, expected))) {
        return Err(Error::Auth("ID-token identity or nonce validation failed".into()));
    }
    if claims.aud.as_array().is_some_and(|a| a.len() > 1) && claims.azp.as_deref() != Some(client) {
        return Err(Error::Auth("ID-token authorized party does not match this app".into()));
    }
    Ok(claims)
}

fn credentials(response: TokenResponse, previous: Option<&Credentials>) -> Result<Credentials> {
    if !response.token_type.eq_ignore_ascii_case("bearer") || response.access_token.is_empty() || response.expires_in == 0 || response.expires_in > 86400 {
        return Err(Error::Auth("invalid token response".into()));
    }
    let id_token = response.id_token.or_else(|| previous.map(|p| p.id_token.clone())).ok_or_else(|| Error::Auth("OpenAI returned no ID token".into()))?;
    let scopes = response.scope.map(|s| s.split_whitespace().map(str::to_owned).collect()).or_else(|| previous.map(|p| p.scopes.clone())).unwrap_or_default();
    if previous.is_some() && response.refresh_token.is_none() {
        return Err(Error::Auth("token renewal did not return a replacement refresh token".into()));
    }
    Ok(Credentials {
        access_token: response.access_token,
        refresh_token: response.refresh_token,
        id_token,
        scopes,
        expires_at: now().saturating_add(response.expires_in),
        earliest_refresh_at: response.earliest_refresh_at,
    })
}

fn authorization_url(host: &str, client: Option<&Registration>, redirect: &str, state: &str, nonce: &str, verifier: &str) -> Result<Url> {
    let mut url = Url::parse(AUTHORIZE).map_err(|_| Error::Auth("invalid authorization endpoint".into()))?;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("client_id", client.map_or(DYNAMIC_CLIENT, |r| r.client_id.as_str()));
        if client.is_none() {
            q.append_pair("agent_name_hint", "PhotoCraft");
        }
        q.append_pair("ext_agent_host_id", host)
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", redirect)
            .append_pair("scope", SCOPES)
            .append_pair("resource", RESOURCE)
            .append_pair("state", state)
            .append_pair("nonce", nonce)
            .append_pair("code_challenge_method", "S256")
            .append_pair("code_challenge", &pkce(verifier));
        if let Some(r) = client {
            if let Some(tokens) = &r.tokens {
                q.append_pair("id_token_hint", &tokens.id_token);
            }
            if let Some(email) = &r.account.email {
                q.append_pair("login_hint", email);
            }
        }
    }
    Ok(url)
}

fn callback_fields(target: &str, expected_state: &str, expected_client: Option<&str>) -> Result<(String, String)> {
    let url = Url::parse(&format!("http://127.0.0.1{target}")).map_err(|_| Error::Auth("invalid sign-in callback".into()))?;
    if url.path() != "/auth/callback" {
        return Err(Error::Auth("invalid callback path".into()));
    }
    let mut fields = HashMap::new();
    for (key, value) in url.query_pairs() {
        if fields.insert(key.into_owned(), value.into_owned()).is_some() {
            return Err(Error::Auth("duplicate sign-in callback parameter".into()));
        }
    }
    if !fields.get("state").is_some_and(|s| same_secret(s, expected_state)) {
        return Err(Error::Auth("sign-in state did not match; start a new sign-in".into()));
    }
    if fields.contains_key("error") {
        return Err(Error::Auth("sign-in was declined or failed; the current account is unchanged".into()));
    }
    let client = match (expected_client, fields.get("client_id")) {
        (Some(saved), Some(received)) if saved != received => return Err(Error::Auth("the returning client ID did not match".into())),
        (Some(saved), _) => saved.to_owned(),
        (None, Some(issued)) if !issued.is_empty() && issued != DYNAMIC_CLIENT => issued.clone(),
        _ => return Err(Error::Auth("registration returned no issued client ID".into())),
    };
    let code = fields.get("code").filter(|s| !s.is_empty()).ok_or_else(|| Error::Auth("sign-in returned no authorization code".into()))?;
    Ok((code.clone(), client))
}

fn reply(stream: &mut TcpStream, code: &str, message: &str) {
    let body =
        format!("<!doctype html><html><head><meta charset=\"utf-8\"><title>PhotoCraft</title></head><body><h1>PhotoCraft</h1><p>{message}</p></body></html>");
    let _ = write!(
        stream,
        "HTTP/1.1 {code}\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\nContent-Security-Policy: default-src 'none'\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
}

fn wait_callback(listener: &TcpListener, state: &str, client: Option<&str>, ctl: &Interrupt<'_>) -> Result<(String, String)> {
    listener.set_nonblocking(true)?;
    let deadline = Instant::now() + CALLBACK_WAIT;
    loop {
        check(ctl)?;
        if Instant::now() >= deadline {
            return Err(Error::Auth("sign-in timed out; try again".into()));
        }
        let (mut stream, peer) = match listener.accept() {
            Ok(pair) => pair,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            Err(e) => return Err(e.into()),
        };
        if !peer.ip().is_loopback() {
            continue;
        }
        // Accepted sockets can inherit O_NONBLOCK on macOS. Read the whole bounded HTTP
        // header with a timeout, including when a browser sends it in several packets.
        stream.set_nonblocking(false)?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        let mut request = Vec::new();
        let mut buf = [0_u8; 512];
        while request.len() < 8192 {
            let n = match stream.read(&mut buf) {
                Ok(n) => n,
                Err(_) => break,
            };
            if n == 0 {
                break;
            }
            request.extend_from_slice(buf.get(..n).unwrap_or(&[]));
            if request.windows(4).any(|v| v == b"\r\n\r\n") {
                break;
            }
        }
        if request.len() > 8192 || !request.windows(4).any(|v| v == b"\r\n\r\n") {
            reply(&mut stream, "431 Request Header Fields Too Large", "Invalid callback request.");
            continue;
        }
        let line = std::str::from_utf8(&request).ok().and_then(|r| r.lines().next()).unwrap_or("");
        let mut parts = line.split_whitespace();
        let (method, target) = (parts.next(), parts.next().unwrap_or(""));
        if method != Some("GET") || !target.starts_with("/auth/callback?") {
            reply(&mut stream, "404 Not Found", "This endpoint is only used for PhotoCraft sign-in.");
            continue;
        }
        let parsed = callback_fields(target, state, client);
        reply(
            &mut stream,
            if parsed.is_ok() { "200 OK" } else { "400 Bad Request" },
            if parsed.is_ok() {
                "Authorization received. Return to PhotoCraft to see whether account verification completed."
            } else {
                "Authorization could not be verified. Return to PhotoCraft and start a new sign-in."
            },
        );
        return parsed;
    }
}

impl Accounts for NativeAccounts {
    fn status(&self) -> AccountStatus {
        status(&self.store.lock().unwrap_or_else(PoisonError::into_inner))
    }

    fn sign_in(&self, account: Option<&str>, ctl: &Interrupt<'_>) -> Result<AccountStatus> {
        self.sign_in_with(
            account,
            ctl,
            |url| open::that_detached(url.as_str()).map_err(|_| Error::Auth("could not open the system browser".into())),
            |fields| self.token(fields),
            |token, client, nonce| self.identity(token, client, nonce),
        )
    }

    fn select(&self, account: &str, ctl: &Interrupt<'_>) -> Result<AccountStatus> {
        // Validate the selected account again before activating it; never combine registrations.
        self.sign_in(Some(account), ctl)
    }

    fn refresh(&self, ctl: &Interrupt<'_>) -> Result<AccountStatus> {
        self.renew_with(ctl, |fields| self.token(fields))
    }

    fn sign_out(&self, ctl: &Interrupt<'_>) -> Result<bool> {
        self.sign_out_with(ctl, |client, refresh, ctl| self.revoke(client, refresh, ctl))
    }

    fn acknowledge_welcome(&self, account: &str) -> Result<()> {
        let _operation = self.operation()?;
        let _lock = self.lock()?;
        let mut store = self.load()?;
        let r = store.registrations.iter_mut().find(|r| r.account.id == account).ok_or_else(|| Error::Auth("the selected account is missing".into()))?;
        r.account.welcomed = true;
        self.save(store)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pkce_and_dynamic_registration_match_the_documented_contract() {
        assert_eq!(pkce("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
        let url = authorization_url("urn:uuid:test", None, "http://127.0.0.1:4567/auth/callback", "state", "nonce", "verifier").unwrap();
        let fields: HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(fields.get("client_id").unwrap(), DYNAMIC_CLIENT);
        assert_eq!(fields.get("agent_name_hint").unwrap(), "PhotoCraft");
        assert_eq!(fields.get("redirect_uri").unwrap(), "http://127.0.0.1:4567/auth/callback");
        assert!(fields.get("scope").unwrap().contains("chatgpt.tokens.use.direct"));
    }
    #[test]
    fn callbacks_reject_forged_states_duplicates_and_client_substitution() {
        assert!(callback_fields("/auth/callback?state=bad&code=abc&client_id=oaiapp_x", "state", None).is_err());
        assert!(callback_fields("/auth/callback?state=state&state=state&code=abc&client_id=oaiapp_x", "state", None).is_err());
        assert!(callback_fields("/auth/callback?state=state&code=abc&client_id=dynamic_agent_client", "state", None).is_err());
        assert!(callback_fields("/auth/callback?state=state&error=access_denied&code=abc", "state", None).is_err());
        assert!(callback_fields("/auth/callback?state=state&code=abc&client_id=oaiapp_y", "state", Some("oaiapp_x")).is_err());
        assert_eq!(callback_fields("/auth/callback?state=state&code=abc", "state", Some("oaiapp_x")).unwrap(), ("abc".into(), "oaiapp_x".into()));
    }
    #[test]
    fn private_storage_preserves_host_and_public_status_omits_tokens() {
        let root = tempfile::tempdir().unwrap();
        let backend = NativeAccounts::new(root.path().to_path_buf()).unwrap();
        let host = backend.load().unwrap().host_id;
        assert!(host.starts_with("urn:uuid:"));
        let mut store = backend.load().unwrap();
        store.registrations.push(Registration {
            account: Account { id: "profile".into(), ..Default::default() },
            client_id: "oaiapp_x".into(),
            subject: "sub".into(),
            tokens: Some(Credentials {
                access_token: "SECRET_ACCESS".into(),
                refresh_token: Some("SECRET_REFRESH".into()),
                id_token: "SECRET_ID".into(),
                scopes: vec!["chatgpt.tokens.use.direct".into()],
                expires_at: u64::MAX,
                earliest_refresh_at: None,
            }),
        });
        store.active = Some("profile".into());
        backend.save(store).unwrap();
        let public = serde_json::to_string(&backend.status()).unwrap();
        assert!(!public.contains("SECRET"));
        assert_eq!(NativeAccounts::new(root.path().to_path_buf()).unwrap().load().unwrap().host_id, host);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(root.path().join("accounts.json")).unwrap().permissions().mode() & 0o777, 0o600);
        }
    }
}

#[cfg(test)]
mod security_tests {
    use super::*;
    use jsonwebtoken::{EncodingKey, Header, encode};
    use rsa::{RsaPrivateKey, pkcs1::EncodeRsaPrivateKey, traits::PublicKeyParts};
    use serde_json::json;
    use std::sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    };

    fn fixture() -> &'static (EncodingKey, JwkSet) {
        static FIXTURE: OnceLock<(EncodingKey, JwkSet)> = OnceLock::new();
        FIXTURE.get_or_init(|| {
            // Ephemeral test keys: no production key or private fixture enters the repository.
            let key = RsaPrivateKey::new(&mut rand::thread_rng(), 2048).unwrap();
            let der = key.to_pkcs1_der().unwrap();
            let jwks = json!({"keys":[{"kty":"RSA","alg":"RS256","use":"sig","kid":"test",
                "n":URL_SAFE_NO_PAD.encode(key.n().to_bytes_be()), "e":URL_SAFE_NO_PAD.encode(key.e().to_bytes_be())}]});
            (EncodingKey::from_rsa_der(der.as_bytes()), serde_json::from_value(jwks).unwrap())
        })
    }

    fn signed(mut claims: serde_json::Value) -> String {
        let defaults = json!({"iss":ISSUER,"aud":"issued-client","sub":"test-subject","exp":now()+3600,
            "nonce":"nonce","email":"test@example.invalid","name":"Test account"});
        for (k, v) in defaults.as_object().unwrap() {
            claims.as_object_mut().unwrap().entry(k).or_insert(v.clone());
        }
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some("test".into());
        encode(&header, &claims, &fixture().0).unwrap()
    }

    #[test]
    fn id_token_requires_signature_issuer_audience_expiry_nonce_and_authorized_party() {
        let token = signed(json!({}));
        assert!(validate_identity(&token, "issued-client", Some("nonce"), &fixture().1).is_ok());
        for claims in [
            json!({"iss":"https://example.invalid"}),
            json!({"aud":"other-client"}),
            json!({"exp":now()-120}),
            json!({"nonce":"wrong"}),
            json!({"nonce":null}),
            json!({"sub":""}),
            json!({"nbf":now()+3600}),
            json!({"aud":["issued-client","other"],"azp":"other"}),
        ] {
            assert!(validate_identity(&signed(claims.clone()), "issued-client", Some("nonce"), &fixture().1).is_err(), "claims: {claims}");
        }
        assert!(
            validate_identity(&signed(json!({"aud":["issued-client","other"],"azp":"issued-client"})), "issued-client", Some("nonce"), &fixture().1).is_ok()
        );
        let mut parts: Vec<_> = token.split('.').map(str::to_owned).collect();
        parts[1] = URL_SAFE_NO_PAD
            .encode(b"{\"iss\":\"https://auth.openai.com\",\"aud\":\"issued-client\",\"sub\":\"forged\",\"exp\":9999999999,\"nonce\":\"nonce\"}");
        assert!(validate_identity(&parts.join("."), "issued-client", Some("nonce"), &fixture().1).is_err());
        let hs = encode(&Header::new(Algorithm::HS256), &json!({"sub":"test-subject"}), &EncodingKey::from_secret(b"secret")).unwrap();
        assert!(validate_identity(&hs, "issued-client", Some("nonce"), &fixture().1).is_err());
    }

    fn simulated_sign_in(backend: &NativeAccounts, saved: Option<&str>, client: &str, subject: &str) -> Result<AccountStatus> {
        let query = Mutex::new(HashMap::<String, String>::new());
        backend.sign_in_with(
            saved,
            &Interrupt::NONE,
            |url| {
                let fields: HashMap<_, _> = url.query_pairs().into_owned().collect();
                assert_eq!(fields["code_challenge_method"], "S256");
                assert_eq!(fields["client_id"], if saved.is_some() { client } else { DYNAMIC_CLIENT });
                if saved.is_some() {
                    assert!(!fields.contains_key("agent_name_hint"));
                }
                let mut callback = Url::parse(&fields["redirect_uri"]).unwrap();
                assert_eq!(callback.host_str(), Some("127.0.0.1"));
                callback.query_pairs_mut().append_pair("state", &fields["state"]).append_pair("code", "test-code").append_pair("client_id", client);
                let mut stream = TcpStream::connect(("127.0.0.1", callback.port().unwrap())).unwrap();
                write!(stream, "GET {}?{} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n", callback.path(), callback.query().unwrap()).unwrap();
                *query.lock().unwrap() = fields;
                Ok(())
            },
            |fields| {
                let fields: HashMap<_, _> = fields.iter().copied().collect();
                let query = query.lock().unwrap();
                assert_eq!(fields["client_id"], client);
                assert_eq!(fields["redirect_uri"], query["redirect_uri"]);
                assert_eq!(pkce(fields["code_verifier"]), query["code_challenge"]);
                assert_eq!(fields["resource"], RESOURCE);
                Ok(TokenResponse {
                    access_token: "test-access".into(),
                    refresh_token: Some("test-refresh".into()),
                    id_token: Some(signed(json!({"aud":client,"sub":subject,"nonce":query["nonce"]}))),
                    token_type: "Bearer".into(),
                    expires_in: 3600,
                    scope: Some(SCOPES.into()),
                    earliest_refresh_at: None,
                })
            },
            |token, client, nonce| validate_identity(token, client, Some(nonce), &fixture().1),
        )
    }

    #[test]
    fn full_loopback_flow_reuses_client_preserves_other_registrations_and_rejects_identity_substitution() {
        let dir = tempfile::tempdir().unwrap();
        let backend = NativeAccounts::new(dir.path().to_path_buf()).unwrap();
        let first = simulated_sign_in(&backend, None, "issued-client", "subject-a").unwrap();
        let first_id = first.active.unwrap();
        let host = backend.load().unwrap().host_id;
        backend.acknowledge_welcome(&first_id).unwrap();
        let second = simulated_sign_in(&backend, None, "second-client", "subject-b").unwrap();
        let second_id = second.active.unwrap();
        assert_ne!(first_id, second_id);
        assert_ne!(second.accounts[0].label, second.accounts[1].label, "identical display names/emails need distinct registration labels");
        let returning = simulated_sign_in(&backend, Some(&first_id), "issued-client", "subject-a").unwrap();
        assert_eq!(returning.accounts.len(), 2);
        assert_eq!(returning.active.as_deref(), Some(first_id.as_str()));
        assert!(returning.accounts.iter().find(|a| a.id == first_id).unwrap().welcomed);
        assert_eq!(backend.load().unwrap().host_id, host);
        let before = serde_json::to_vec(&backend.load().unwrap()).unwrap();
        assert!(simulated_sign_in(&backend, Some(&first_id), "issued-client", "wrong-subject").is_err());
        assert_eq!(serde_json::to_vec(&backend.load().unwrap()).unwrap(), before);
        assert!(returning.accounts.iter().any(|a| a.id == second_id && a.connected));
    }

    #[test]
    fn refresh_persists_rotation_even_when_cancelled_and_failure_preserves_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let backend = NativeAccounts::new(dir.path().to_path_buf()).unwrap();
        simulated_sign_in(&backend, None, "issued-client", "subject-a").unwrap();
        let mut store = backend.load().unwrap();
        store.registrations[0].tokens.as_mut().unwrap().expires_at = now();
        backend.save(store).unwrap();
        let cancelled = AtomicBool::new(false);
        let cancel = || cancelled.load(Ordering::Relaxed);
        let progress = |_| {};
        let ctl = Interrupt::new(&cancel, &progress);
        let result = backend.renew_with(&ctl, |fields| {
            let fields: HashMap<_, _> = fields.iter().copied().collect();
            assert_eq!(fields["client_id"], "issued-client");
            assert_eq!(fields["refresh_token"], "test-refresh");
            assert_eq!(fields["resource"], RESOURCE);
            assert!(!fields.contains_key("scope"));
            cancelled.store(true, Ordering::Relaxed);
            Ok(TokenResponse {
                access_token: "replacement-access".into(),
                refresh_token: Some("replacement-refresh".into()),
                id_token: None,
                token_type: "Bearer".into(),
                expires_in: 3600,
                scope: None,
                earliest_refresh_at: None,
            })
        });
        assert!(matches!(result, Err(Error::Cancelled)));
        let mut store = backend.load().unwrap();
        assert_eq!(store.registrations[0].tokens.as_ref().unwrap().refresh_token.as_deref(), Some("replacement-refresh"));
        store.registrations[0].tokens.as_mut().unwrap().expires_at = now();
        backend.save(store).unwrap();
        let before = fs::read(dir.path().join("accounts.json")).unwrap();
        assert!(backend.renew_with(&Interrupt::NONE, |_| Err(Error::Auth("simulated network error".into()))).is_err());
        assert_eq!(fs::read(dir.path().join("accounts.json")).unwrap(), before);
    }

    #[test]
    fn process_lock_cancellation_and_corrupt_storage_leave_existing_data_intact() {
        let dir = tempfile::tempdir().unwrap();
        let backend = NativeAccounts::new(dir.path().to_path_buf()).unwrap();
        let _lock = backend.lock().unwrap();
        let before = fs::read(dir.path().join("accounts.json")).unwrap();
        assert!(NativeAccounts::new(dir.path().to_path_buf()).is_err());
        assert_eq!(fs::read(dir.path().join("accounts.json")).unwrap(), before);
        let cancel = || true;
        let progress = |_| {};
        let ctl = Interrupt::new(&cancel, &progress);
        assert!(matches!(backend.sign_in(None, &ctl), Err(Error::Cancelled)));
        drop(_lock);
        fs::write(dir.path().join("accounts.json"), b"corrupt data").unwrap();
        assert!(NativeAccounts::new(dir.path().to_path_buf()).is_err());
        assert_eq!(fs::read(dir.path().join("accounts.json")).unwrap(), b"corrupt data");
    }
    #[test]
    fn sign_out_clears_only_active_tokens_and_reports_unconfirmed_revocation() {
        let dir = tempfile::tempdir().unwrap();
        let backend = NativeAccounts::new(dir.path().to_path_buf()).unwrap();
        let first = simulated_sign_in(&backend, None, "issued-client", "subject-a").unwrap().active.unwrap();
        let second = simulated_sign_in(&backend, None, "second-client", "subject-b").unwrap().active.unwrap();
        let host = backend.load().unwrap().host_id;
        assert!(
            !backend
                .sign_out_with(&Interrupt::NONE, |client, refresh, _| {
                    assert_eq!(client, "second-client");
                    assert_eq!(refresh, "test-refresh");
                    false
                })
                .unwrap()
        );
        let store = backend.load().unwrap();
        assert!(store.active.is_none());
        assert_eq!(store.host_id, host);
        assert!(store.registrations.iter().find(|r| r.account.id == first).unwrap().tokens.is_some());
        assert!(store.registrations.iter().find(|r| r.account.id == second).unwrap().tokens.is_none());
        assert_eq!(store.registrations.iter().find(|r| r.account.id == second).unwrap().client_id, "second-client");
        simulated_sign_in(&backend, Some(&second), "second-client", "subject-b").unwrap();
        assert_eq!(backend.status().accounts.len(), 2);
        assert!(backend.sign_out_with(&Interrupt::NONE, |_, _, _| true).unwrap());
    }

    #[test]
    fn callback_can_arrive_in_separate_packets() {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let writer = std::thread::spawn(move || {
            let mut stream = TcpStream::connect(address).unwrap();
            stream.write_all(b"GET /auth/callback?state=state&code=code&client_id=issued-client HTTP/1.1\r\n").unwrap();
            std::thread::sleep(Duration::from_millis(40));
            stream.write_all(b"Host: 127.0.0.1\r\n\r\n").unwrap();
            let mut response = Vec::new();
            stream.read_to_end(&mut response).unwrap();
            assert!(String::from_utf8_lossy(&response).starts_with("HTTP/1.1 200"));
        });
        assert_eq!(wait_callback(&listener, "state", None, &Interrupt::NONE).unwrap(), ("code".into(), "issued-client".into()));
        writer.join().unwrap();
    }
}
