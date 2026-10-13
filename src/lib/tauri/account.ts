import { invoke } from "@tauri-apps/api/core";
import type { ApiOperation, AuthStatus } from "@/lib/tauri/generated";

export type { ApiOperation, AuthStatus };

export const authStatus = () => invoke<AuthStatus>("auth_status");
/** Opens the system browser on cyberauth.co and resolves once the player is back. */
export const authLogin = () => invoke<AuthStatus>("auth_login");
export const authLogout = () => invoke<void>("auth_logout");

/**
 * Runs a CyberBackend operation; the Rust side attaches the player's token. The GraphQL
 * documents live on the Rust side (`account/api_operation.rs`), which refuses anything else.
 */
export const apiQuery = <T>(operation: ApiOperation, variables?: Record<string, unknown>) => invoke<T>("api_query", { operation, variables });
