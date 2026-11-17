//! The Docker CLI's image references, and a refused socket in the player's words.

use crate::error::{Code, Error};
use isothrum_machine::HostOs;

/// An image reference safe to pass to the Docker CLI, as a pattern: a normal
/// `registry/name:tag@digest` of plain ASCII name characters, 1 to 200 of them, not starting with
/// a dash (it would read as a flag). The webview checks the attack box's custom image by it as it
/// is typed (the generated `Patterns.ts`), so it is written in the syntax both regex engines read.
pub const IMAGE_PATTERN: &str = r"^[A-Za-z0-9._/:@][A-Za-z0-9._/:@\-]{0,199}$";

/// [`IMAGE_PATTERN`], compiled; None never happens (a test compiles it), and would refuse every image.
static IMAGE: std::sync::LazyLock<Option<regex::Regex>> = std::sync::LazyLock::new(|| regex::Regex::new(IMAGE_PATTERN).ok());

/// Whether `image` is a reference safe to pass to the Docker CLI ([`IMAGE_PATTERN`]).
pub fn valid_image(image: &str) -> bool {
    IMAGE.as_ref().is_some_and(|re| re.is_match(image))
}

/// An image reference's parts, `[registry/]repository[:tag][@digest]`, the digest left out:
/// `registry.local:5000/team/wordpress:6.4` is `team/wordpress` at `6.4` on `registry.local:5000`.
/// The one reader of an image reference.
#[derive(Debug, PartialEq, Eq)]
pub struct ImageRef<'a> {
    /// The registry host, when the reference names one (a first part with a `.` or a `:`, or
    /// `localhost`); Docker Hub's when none.
    pub registry: Option<&'a str>,
    /// The repository, without registry, tag or digest: `x/y`, `wordpress`.
    pub repository: &'a str,
    /// The tag, when the reference names one.
    pub tag: Option<&'a str>,
}

impl<'a> ImageRef<'a> {
    /// Reads `image`'s parts, its digest left out.
    pub fn parse(image: &'a str) -> ImageRef<'a> {
        let image = image.split('@').next().unwrap_or(image);
        let (path, tag) = match image.rsplit_once(':') {
            Some((path, tag)) if !tag.contains('/') => (path, Some(tag)),
            _ => (image, None),
        };
        match path.split_once('/') {
            Some((host, repository)) if host.contains('.') || host.contains(':') || host == "localhost" => ImageRef { registry: Some(host), repository, tag },
            _ => ImageRef { registry: None, repository: path, tag },
        }
    }
}

/// The error of a player whose account can't use the running Docker daemon (not in the `docker`
/// group, or not since its last login), in the launcher's words: isothrum-machine tells a refusal
/// from a daemon that isn't running ([`says_denied`](isothrum_machine::docker::says_denied)), so
/// the app never offers to start an engine that already runs.
pub fn docker_denied_error() -> Error {
    Error::coded(denied_code(isothrum_runner::this_host(), login_name().as_deref()))
}

/// [`docker_denied_error`]'s code on `os`, for the account `user`. Windows grants the engine's
/// pipe to the `docker-users` group (which Docker Desktop's installer fills only with the account
/// that installed it); elsewhere the socket belongs to the `docker` group.
fn denied_code(os: HostOs, user: Option<&str>) -> Code {
    if os == HostOs::Windows {
        Code::DockerDeniedWindows { user: user.unwrap_or("%USERNAME%").into() }
    } else {
        Code::DockerDenied { user: user.unwrap_or("$USER").into() }
    }
}

/// The login name of the user running the app.
pub fn login_name() -> Option<String> {
    #[cfg(unix)]
    {
        crate::platform::sys::login_name().or_else(|| std::env::var("USER").ok())
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
        let repository = |image| ImageRef::parse(image).repository;
        assert_eq!(repository("wordpress:6.4"), "wordpress");
        assert_eq!(repository("registry.local:5000/team/novnc:1@sha256:abc"), "team/novnc");
        assert_eq!(repository("docker.io/cyberctf/attack-box:latest"), "cyberctf/attack-box");
        assert_eq!(repository("index.docker.io/cyberctf/attack-box@sha256:abc"), "cyberctf/attack-box");
        assert_eq!(repository("localhost/app"), "app");
        assert_eq!(repository("team/app"), "team/app");
        assert_eq!(
            ImageRef::parse("registry.local:5000/team/novnc:1@sha256:abc"),
            ImageRef { registry: Some("registry.local:5000"), repository: "team/novnc", tag: Some("1") }
        );
        assert_eq!(ImageRef::parse("registry.local:5000/novnc"), ImageRef { registry: Some("registry.local:5000"), repository: "novnc", tag: None });
    }

    #[test]
    fn image_references_never_read_as_flags() {
        assert!(valid_image("cyberctf/attack-box") && valid_image("ghcr.io/org/img:1.2@sha256:abc") && valid_image("nwodtuhs/exegol:free"));
        assert!(!valid_image("--privileged") && !valid_image("-v") && !valid_image("a b") && !valid_image("") && !valid_image(&"a".repeat(201)));
        assert!(!valid_image("img; rm -rf /"));
        assert!(IMAGE.is_some(), "the pattern compiles");
        assert!(valid_image(&"a".repeat(200)) && valid_image("a-") && !valid_image("é") && !valid_image("img\n"));
    }

    #[test]
    fn the_denied_message_names_the_command_to_run() {
        let message = |os, user| Error::coded(denied_code(os, user)).to_string();
        assert!(message(HostOs::Linux, Some("ada")).contains("sudo usermod -aG docker ada"));
        assert!(message(HostOs::Linux, None).contains("usermod -aG docker $USER"));
        // Windows (#218): the docker-users group, not a Linux command.
        assert_eq!(denied_code(HostOs::Windows, Some("Ada Lovelace")), Code::DockerDeniedWindows { user: "Ada Lovelace".into() });
        let windows = message(HostOs::Windows, Some("Ada Lovelace"));
        assert!(windows.contains("net localgroup docker-users \"Ada Lovelace\" /add") && windows.contains("sign out"), "{windows}");
        assert!(!windows.contains("usermod"));
        assert!(docker_denied_error().to_string().contains(if cfg!(windows) { "docker-users" } else { "usermod -aG docker" }));
    }
}
