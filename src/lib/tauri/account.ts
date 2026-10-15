import { invoke } from "@tauri-apps/api/core";
import type { ApiOperation, AuthStatus } from "@/lib/tauri/generated";
import { invokeStreaming } from "@/lib/tauri/stream";

export type { ApiOperation, AuthStatus };

export const authStatus = () => invoke<AuthStatus>("auth_status");
/** Opens the system browser on cyberauth.co and resolves once the player is back. `onOpened`
 *  gets the sign-in link as soon as the browser was asked to open it. Use `signIn` (features/account/sign-in),
 *  which keeps one sign-in at a time and its state for every view. */
export const authLogin = (onOpened: (url: string) => void) => invokeStreaming<string, AuthStatus>("auth_login", {}, onOpened, "events");
/** Ends the sign-in waiting for the browser: `authLogin` then rejects with the `signInCancelled` code. */
export const authLoginCancel = () => invoke<void>("auth_login_cancel");
export const authLogout = () => invoke<void>("auth_logout");

/**
 * Runs a CyberBackend operation; the Rust side attaches the player's token. The GraphQL
 * documents live on the Rust side (`account/api_operation.rs`), which refuses anything else.
 */
export const apiQuery = <T>(operation: ApiOperation, variables?: Record<string, unknown>) => invoke<T>("api_query", { operation, variables });
