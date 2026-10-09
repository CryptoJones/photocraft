# photocraft-chatgpt

Optional app-owned Sign in with ChatGPT for native PhotoCraft. The public `Accounts` trait
returns credential-free `AccountStatus`, runs cancellable account operations, and has no
document/UI dependency. The `native` feature provides `NativeAccounts`; default/wasm builds
contain only the portable interface. Host code must explicitly inject the service and a
protected configuration directory. Creating a default Session performs no account I/O.

See [Sign in with ChatGPT](../../docs/chatgpt-sign-in.md) for the OAuth contract, storage,
revocation, commands, build instructions and tests. This crate sends no inference or image
requests and never uses credentials belonging to Codex or another app.
