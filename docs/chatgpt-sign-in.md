# Sign in with ChatGPT (native preview)

PhotoCraft can connect an account through the official open-source [Sign in with ChatGPT
flow](https://developers.openai.com/siwc/token-sharing-open-source/sign-in). This is an optional
native integration, built with `chatgpt`; ordinary and web builds do not initialize it. The
OpenAI preview currently requires an eligible ChatGPT account (see the [preview
limitations](https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations)).

**This change provides account connection and coming-soon generative draft interfaces.** It makes no inference requests and consumes
no ChatGPT plan usage. Image generation, extension, reframing and prompted object edits have
editable local forms, but generation is disabled: the documented ChatGPT-plan preview excludes image generation. See
[Generative editing drafts](generative-editing.md) for what is implemented and what remains. Remove
Background, Select Subject and Object Selection continue to run on the device, with the
classical methods or separately downloaded models. Signing in does not change those methods.

## Build and try

From the repository root:

```sh
cargo run --release -p photocraft --features local-ml,chatgpt
```

Open **Window › ChatGPT Account…**, or **Edit › Preferences › Integrations › ChatGPT account
settings…**. Choose **Continue with ChatGPT** and finish authorization on OpenAI in your system
browser. PhotoCraft becomes connected only after it validates the callback and ID token. A
first-connection message explains plan permission and the absence of usage in this build.

The account window shows the active account, granted plan permission, **Manage usage**, saved
registrations, **Refresh connection**, and **Sign out**. A saved registration's **Continue as…**
action repeats browser authorization with its existing client ID. **Add another ChatGPT
account** creates a separate registration. Registrations remain distinct even when they have
identical names or email addresses. Refresh renews credentials only near expiry; it does not
send an AI request. Cancel and progress use the app's ordinary background jobs.

## Implementation and privacy

`photocraft-chatgpt` is an L4 service with no document or UI dependency. The engine injects it;
creating a default `Session` does not read credentials or access the network. The desktop
frontend supplies its configuration directory. Windows, Linux and macOS use the same Rust
OAuth implementation and open the system browser. No Python, webview, client secret, OpenAI
API key, Codex credential file or private ChatGPT backend is used.

The flow binds an IPv4 loopback listener on an available port **before** launching the browser.
It requests the documented resource and scopes using fresh cryptographic state, nonce and
PKCE S256. Dynamic registration returns an issued client ID; that ID is used for code exchange
and retained with the verified issuer/client/subject identity and stable installation host ID.
Returning authorization uses that registration's client ID and sends ID-token/login hints only
to OpenAI's authorization endpoint. Signature, signing key, issuer, audience, expiration, nonce
and multi-audience authorized-party validation must succeed before an account becomes active.
Duplicate parameters, mismatched state/client/identity and unsupported JWT algorithms fail
without replacing the current account. Callback headers and response/storage sizes are bounded.

Credentials live in the host-supplied `ChatGPT/accounts.json` inside a private directory.
Unix permissions are `0700` for the directory and `0600` for files. Windows replaces the DACL with the current
user SID using the built-in Windows PowerShell ACL APIs (no additional installation); failure to protect storage disables the
integration. A process file lock serializes rotating-token operations, and private temporary
files replace the complete store atomically. Successful refresh replacements are saved even
when cancellation arrives afterward. Cancellation does not roll back an already completed
credential rotation or local sign-out. Corrupt stores are preserved and reported; they are
never silently reset. Tokens and authorization URLs are excluded from command results,
serialized UI state and diagnostics.

Sign-out attempts revocation at the same-origin endpoint from OpenAI discovery, with backoff,
then removes the selected registration's local tokens. Its client mapping and host ID remain
for a later sign-in. A failed or cancelled remote attempt still clears local tokens and reports
that remote revocation was not confirmed; **Manage usage** lets the user disconnect PhotoCraft
in ChatGPT Settings. Other saved registrations retain their own credentials.

The local account operations are engine commands (`account.chatgpt.status`, `signIn`, `select`,
`refresh`, `signOut`, `acknowledgeWelcome`) and never edit a document or its undo history.
Untrusted control/MCP sessions may read public status, but cannot start authorization or mutate
private credentials, including through nested action playback. The user starts those operations
in the local UI. `ui.inspect` contains public account status only; `ui.set {"chatgptAccount":true}`
opens the account window without signing in.

## Validation

Backend tests use ephemeral RSA test keys and a real loopback callback with simulated browser
and token transport. They cover first registration, returning authorization and PKCE, distinct
account registration, identity substitution, forged signatures, issuer/audience/nonce/time
checks, partial HTTP packets, cancellation, process locking, corrupt storage, rotating-token
persistence and sign-out success/failure. No real account, paid request or production token is
used by tests. Engine tests cover document-independent jobs, graceful errors and unchanged
history/floating selections; UI and automation tests cover discovery and authorization gates.

```sh
cargo test -p photocraft-chatgpt --features native
cargo test -p photocraft-engine --features chatgpt
cargo test -p photocraft-ui-egui -p photocraft-automation
cargo clippy -p photocraft-chatgpt -p photocraft-engine -p photocraft-ui-egui -p photocraft-automation -p photocraft --features photocraft/chatgpt --all-targets -- -D warnings
cargo xtask layers
cargo xtask wasm
cargo xtask parity
cargo test -p photocraft-engine --test panic_hunt -- --ignored
```

Live OpenAI authorization and Windows/Linux runtime checks are separate from simulated-flow
and compile checks. Record their actual results when exercised; passing mocks does not confirm
account eligibility or platform browser/ACL behavior.

See [contextual-generative-validation.md](contextual-generative-validation.md) for the
recorded local results and screenshots.
