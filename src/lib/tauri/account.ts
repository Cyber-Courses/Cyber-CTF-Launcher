import { invoke } from "@tauri-apps/api/core";
import type { ApiOperation, AuthStatus, DeviceCode } from "@/lib/tauri/generated";
import { invokeStreaming, invokeUnit } from "@/lib/tauri/invoke";

export type { ApiOperation, AuthStatus, DeviceCode };

export const authStatus = () => invoke<AuthStatus>("auth_status");
/** Opens the system browser on cyberauth.co and resolves once the player is back. `onOpened`
 *  gets the sign-in link as soon as the browser was asked to open it. Use `signIn` (features/account/sign-in),
 *  which keeps one sign-in at a time and its state for every view. */
export const authLogin = (onOpened: (url: string) => void) => invokeStreaming<AuthStatus>("auth_login", {}, onOpened, "events");
/** Signs in with a code entered on any device, for when this machine's browser can't be used.
 *  `onCode` gets the code and where to enter it; resolves once the player approved it. Use
 *  `signInWithCode` (features/account/sign-in). Starting it ends a browser sign-in waiting. */
export const authLoginDevice = (onCode: (code: DeviceCode) => void) => invokeStreaming<AuthStatus>("auth_login_device", {}, onCode, "events");
/** Ends the sign-in waiting for the browser or a code: it then rejects with the `signInCancelled` code. */
export const authLoginCancel = () => invokeUnit("auth_login_cancel");
export const authLogout = () => invokeUnit("auth_logout");

/**
 * Runs a CyberBackend operation; the Rust side attaches the player's token. The GraphQL
 * documents live on the Rust side (`account/api_operation.rs`), which refuses anything else.
 */
export const apiQuery = <T>(operation: ApiOperation, variables?: Record<string, unknown>) => invoke<T>("api_query", { operation, variables });
