# Signing in

The launcher signs in to [Cyber Auth](https://www.cyberauth.co) the way desktop and CLI apps should (RFC 8252): the player never copies a token. The code is in [`src-tauri/src/account/`](../src-tauri/src/account/).

1. **Sign in** opens the system browser on cyberauth.co with an OAuth 2.0 authorization code request protected by PKCE (S256). A player already signed in there only confirms the account (`prompt=select_account`).
2. cyber-auth redirects to a one-shot server the launcher runs on `http://127.0.0.1:<port>/callback` (the first free one of 47290, 47291, 47292). The launcher checks the `state`, exchanges the code with its PKCE verifier, and only then tells the browser tab "You're signed in. You can close this tab."
3. The tokens go to the OS keychain (macOS Keychain, Secret Service on Linux, Windows Credential Manager), never to a file or the webview. The access token is refreshed silently a minute before it expires, so the session survives restarts until the refresh token is revoked. **Sign out** deletes them from the keychain; when the keychain refuses (locked, its prompt dismissed), it says so and the player stays signed in.

While it waits, the launcher offers two fallbacks under the spinner:

- **Browser didn't open? Copy the sign-in link**: for a default browser that takes the link and shows nothing.
- **Use a code instead**: the OAuth device authorization grant (RFC 8628), for a machine whose browser can't be used (a remote desktop, a kiosk, an SSH-forwarded window). The launcher shows a short code and the cyberauth.co page to enter it on, from any device, and polls until it is approved, declined or expired (at most 30 minutes).

The sidebar and **Settings > Account** show who is signed in (name and email from the ID token). Errors say what to do: a sign-in not finished within 5 minutes (or a code that expired) is cancelled, a declined consent says so, cyber-auth unreachable says to check the connection, and a keychain that can't be read (locked, its prompt dismissed, no Secret Service on Linux) is shown in Settings. Cancel always ends the wait.

## The OAuth client

The OAuth client in cyber-auth must be first-party and public (`token_endpoint_auth_method: none`, PKCE required, consent skipped), use the `authorization_code` and `refresh_token` grants, and have these redirect URIs: `http://127.0.0.1:47290/callback`, `http://127.0.0.1:47291/callback`, `http://127.0.0.1:47292/callback`. **Use a code instead** also needs the `urn:ietf:params:oauth:grant-type:device_code` grant on that client; until it has it, cyber-auth answers `unauthorized_client` and the launcher says signing in with a code isn't available yet, pointing to the copy-the-link fallback.

The endpoints and the client id can be overridden for testing: see [reference/configuration.md](reference/configuration.md).
