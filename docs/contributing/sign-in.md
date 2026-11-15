# How sign-in works

The launcher signs in to [Cyber Auth](https://www.cyberauth.co) the way desktop and CLI apps
should (RFC 8252): the player never copies a token. The code is in
[`src-tauri/src/account/`](../../src-tauri/src/account/). What a player does is in
[signing in](../players/signing-in.md).

## The browser sign-in

1. **Sign in** opens the system browser on cyberauth.co with an OAuth 2.0 authorization code
   request protected by PKCE (S256). A player already signed in there only confirms the account
   (`prompt=select_account`).
2. cyber-auth redirects to a one-shot server the launcher runs on
   `http://127.0.0.1:<port>/callback` (the first free one of 47290, 47291, 47292). The launcher
   checks the `state`, exchanges the code with its PKCE verifier, and only then tells the browser
   tab "You're signed in. You can close this tab."
3. The tokens go to the OS keychain (macOS Keychain, Secret Service on Linux, Windows Credential
   Manager), never to a file or the webview. The access token is refreshed silently a minute
   before it expires, so the session survives restarts until the refresh token is revoked.
   **Sign out** deletes them from the keychain.

**Use a code instead** is the OAuth device authorization grant (RFC 8628): the launcher polls the
token endpoint until the code is approved, declined or expired (at most 30 minutes). A browser
sign-in not finished within 5 minutes is cancelled. The name and email shown come from the ID
token.

## The OAuth client

The OAuth client in cyber-auth must be first-party and public
(`token_endpoint_auth_method: none`, PKCE required, consent skipped), use the
`authorization_code` and `refresh_token` grants, and have these redirect URIs:
`http://127.0.0.1:47290/callback`, `http://127.0.0.1:47291/callback`,
`http://127.0.0.1:47292/callback`. **Use a code instead** also needs the
`urn:ietf:params:oauth:grant-type:device_code` grant on that client; until it has it, cyber-auth
answers `unauthorized_client` and the launcher says signing in with a code isn't available yet,
pointing to the copy-the-link fallback.

The endpoints and the client id can be overridden for testing: see
[configuration](../admins/configuration.md).
