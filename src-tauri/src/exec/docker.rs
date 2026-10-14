//! The Docker CLI's quirks: which engine it talks to, and telling a refused socket from a
//! daemon that isn't running.

use crate::error::Error;

/// Environment variables that would redirect the Docker CLI to a different engine than the one
/// the user configured as current (`docker context use`, which Docker Desktop's UI reflects).
/// A process that launches the app (an IDE, a terminal integration, another tool) can carry a
/// stale `DOCKER_HOST` or `DOCKER_CONTEXT` — e.g. pointing at OrbStack's `/var/run/docker.sock`
/// while the user works in Docker Desktop. Every lab would then deploy to, and be looked for in,
/// an engine the user never sees: status reads "not running" and a launch looks like it reset.
/// Stripping these makes the launcher follow the configured current context, so it always agrees
/// with `docker context show` and the Desktop UI.
pub(super) const ENGINE_OVERRIDES: [&str; 2] = ["DOCKER_HOST", "DOCKER_CONTEXT"];

/// Whether `program` talks to the Docker engine (`docker`, including `docker compose`, a CLI
/// plugin of the same binary), and so must not inherit an engine override.
pub(super) fn uses_engine(program: &str) -> bool {
    program == "docker"
}

/// Whether the inherited engine overrides go: only when the user picked a context of their own
/// (`docker context use`, recorded as `currentContext` in the CLI config). Without one, an
/// inherited `DOCKER_HOST` is how the user reaches their engine (rootless Docker on Linux sets
/// it to `$XDG_RUNTIME_DIR/docker.sock`), and stripping it pointed the CLI at the root daemon.
pub(super) fn strips_engine_overrides(cli_config: Option<&str>) -> bool {
    cli_config
        .and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok())
        .and_then(|v| v.get("currentContext").and_then(|c| c.as_str()).map(|c| !c.is_empty() && c != "default"))
        .unwrap_or(false)
}

/// The Docker CLI's config file (`$DOCKER_CONFIG/config.json`, else `~/.docker/config.json`).
pub(super) fn cli_config() -> Option<String> {
    let dir = std::env::var_os("DOCKER_CONFIG").map(std::path::PathBuf::from).or_else(|| crate::platform::home_dir().map(|h| h.join(".docker")))?;
    std::fs::read_to_string(dir.join("config.json")).ok()
}

/// An image reference safe to pass to the Docker CLI: a normal `registry/name:tag@digest`,
/// nothing that reads as a flag.
pub fn valid_image(image: &str) -> bool {
    !image.is_empty()
        && image.len() <= 200
        && !image.starts_with('-')
        && image.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/' | ':' | '@'))
}

/// An image reference's repository, without registry, tag or digest:
/// `registry.local:5000/team/wordpress:6.4` -> `team/wordpress`, `docker.io/x/y` -> `x/y`.
pub fn image_repository(image: &str) -> &str {
    let image = image.split('@').next().unwrap_or(image);
    let image = match image.rfind(':') {
        Some(i) if !image[i..].contains('/') => &image[..i],
        _ => image,
    };
    match image.split_once('/') {
        Some((host, rest)) if host.contains('.') || host.contains(':') || host == "localhost" => rest,
        _ => image,
    }
}

/// Whether a failed `docker` call means this account can't reach the daemon's socket (not in
/// the `docker` group, or not since its last login) rather than a daemon that isn't running.
/// Told apart, the app says which: offering to start an engine that already runs only spun.
pub fn docker_denied(err: &Error) -> bool {
    matches!(err, Error::CommandFailed { stderr, .. } if denied_text(stderr))
}

fn denied_text(stderr: &str) -> bool {
    let s = stderr.to_lowercase();
    s.contains("permission denied") && (s.contains("docker.sock") || s.contains("docker api") || s.contains("docker daemon"))
}

/// What to tell a player whose account can't use the running Docker daemon.
pub fn docker_denied_message() -> String {
    let user = login_name().unwrap_or_else(|| "$USER".into());
    format!(
        "Docker is running, but your account can't use it yet. Add yourself to the docker group in a terminal (sudo usermod -aG docker {user}), then log out and back in."
    )
}

/// The login name of the user running the app.
pub fn login_name() -> Option<String> {
    #[cfg(unix)]
    {
        // SAFETY: getpwuid returns a pointer into static storage (or null), read at once.
        let from_passwd = unsafe {
            let pw = libc::getpwuid(libc::getuid());
            if pw.is_null() { None } else { std::ffi::CStr::from_ptr((*pw).pw_name).to_str().ok().map(str::to_string) }
        };
        from_passwd.or_else(|| std::env::var("USER").ok())
    }
    #[cfg(not(unix))]
    {
        std::env::var("USERNAME").ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_image_repository_has_no_registry_tag_or_digest() {
        assert_eq!(image_repository("wordpress:6.4"), "wordpress");
        assert_eq!(image_repository("registry.local:5000/team/novnc:1@sha256:abc"), "team/novnc");
        assert_eq!(image_repository("docker.io/cyberctf/attack-box:latest"), "cyberctf/attack-box");
        assert_eq!(image_repository("index.docker.io/cyberctf/attack-box@sha256:abc"), "cyberctf/attack-box");
        assert_eq!(image_repository("localhost/app"), "app");
        assert_eq!(image_repository("team/app"), "team/app");
    }

    #[test]
    fn a_denied_docker_socket_is_not_a_stopped_daemon() {
        let failed = |stderr: &str| Error::CommandFailed { command: "docker info".into(), stderr: stderr.into() };
        // Both wordings the Docker CLI has used.
        assert!(docker_denied(&failed("permission denied while trying to connect to the docker API at unix:///var/run/docker.sock")));
        assert!(docker_denied(&failed(
            "Got permission denied while trying to connect to the Docker daemon socket at unix:///var/run/docker.sock: Get \"http://%2Fvar%2Frun%2Fdocker.sock/v1.24/info\""
        )));
        assert!(!docker_denied(&failed("Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?")));
        assert!(!docker_denied(&Error::ToolMissing { tool: "docker" }));
    }

    #[test]
    fn only_docker_drops_inherited_engine_overrides() {
        // Regression: an inherited DOCKER_HOST pointed the app at OrbStack while the user worked
        // in Docker Desktop, so labs were deployed to and looked for in the wrong engine.
        assert!(uses_engine("docker"));
        assert!(!uses_engine("vagrant"));
        assert!(!uses_engine("terraform"));
        assert_eq!(ENGINE_OVERRIDES, ["DOCKER_HOST", "DOCKER_CONTEXT"]);
    }

    #[test]
    fn engine_overrides_go_only_when_the_user_picked_a_context() {
        use super::strips_engine_overrides as strips;
        assert!(strips(Some(r#"{"currentContext":"desktop-linux"}"#)));
        // No context of their own: an inherited DOCKER_HOST (rootless Docker) is how they reach it.
        assert!(!strips(Some(r#"{"currentContext":"default"}"#)));
        assert!(!strips(Some(r#"{"auths":{}}"#)));
        assert!(!strips(None));
        assert!(!strips(Some("not json")));
    }

    #[test]
    fn image_references_never_read_as_flags() {
        assert!(valid_image("cyberctf/attack-box") && valid_image("ghcr.io/org/img:1.2@sha256:abc"));
        assert!(!valid_image("--privileged") && !valid_image("a b") && !valid_image("") && !valid_image(&"a".repeat(201)));
    }

    #[test]
    fn the_denied_message_names_the_command_to_run() {
        assert!(docker_denied_message().contains("sudo usermod -aG docker "));
    }
}
