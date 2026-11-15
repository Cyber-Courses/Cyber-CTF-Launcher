//! How big an attack-box image is to download, as Docker Hub reports it for this machine's
//! architecture. Facts only: an image Docker Hub doesn't describe (another registry, a
//! private or unpublished tag) has no size, rather than a guessed one.

use std::time::Duration;

/// Docker Hub's `namespace/repo` and tag for an image reference, or None when the image
/// lives on another registry. `kali` -> `library/kali`, no tag -> `latest`.
fn hub_ref(image: &str) -> Option<(String, String)> {
    let r = crate::platform::exec::ImageRef::parse(image);
    if r.registry.is_some_and(|host| host != "docker.io") {
        return None;
    }
    let repo = if r.repository.contains('/') { r.repository.to_owned() } else { format!("library/{}", r.repository) };
    Some((repo, r.tag.unwrap_or("latest").to_owned()))
}

/// Docker Hub's name for this machine's CPU architecture.
fn hub_arch() -> &'static str {
    crate::platform::host::Arch::HERE.docker_name()
}

/// Compressed download size in bytes of `image` for this machine, from Docker Hub.
pub async fn image_download_size(image: String) -> Option<u64> {
    // Same guard as the attack box.
    if !crate::platform::exec::valid_image(&image) {
        return None;
    }
    let (repo, tag) = hub_ref(&image)?;
    match hub_tag_size(&repo, &tag).await {
        Ok(size) => size,
        // No size is what the page shows; why (no client, no network, Hub's refusal, an answer
        // without one) goes to the log, not into silence.
        Err(why) => {
            log::debug!("no download size for {image}: {why}");
            None
        }
    }
}

/// The size Docker Hub gives for `repo:tag` on this architecture, or why it couldn't be asked.
async fn hub_tag_size(repo: &str, tag: &str) -> Result<Option<u64>, String> {
    let url = format!("https://hub.docker.com/v2/repositories/{repo}/tags/{tag}");
    let client = crate::platform::http::probe_client(Duration::from_secs(8), false).map_err(|e| format!("no HTTP client: {e}"))?;
    let res = client.get(url).send().await.map_err(|e| format!("Docker Hub didn't answer: {e}"))?;
    let res = res.error_for_status().map_err(|e| format!("Docker Hub refused: {e}"))?;
    let tag: HubTag = res.json().await.map_err(|e| format!("Docker Hub's answer isn't a tag's description: {e}"))?;
    Ok(size_for_arch(&tag, hub_arch()))
}

/// What the launcher reads of Docker Hub's tag description (`/v2/repositories/<repo>/tags/<tag>`):
/// its image per architecture. Unknown fields are skipped.
#[derive(Debug, Default, serde::Deserialize)]
struct HubTag {
    #[serde(default)]
    images: Vec<HubImage>,
}

/// One architecture's image of a tag, and its compressed size in bytes.
#[derive(Debug, serde::Deserialize)]
struct HubImage {
    architecture: Option<String>,
    size: Option<u64>,
}

/// The size Docker Hub's tag description gives for `arch`, when it gives one.
fn size_for_arch(tag: &HubTag, arch: &str) -> Option<u64> {
    tag.images.iter().find(|i| i.architecture.as_deref() == Some(arch)).and_then(|i| i.size).filter(|s| *s > 0)
}

#[cfg(test)]
mod tests {
    use super::{HubTag, hub_arch, hub_ref, image_download_size, size_for_arch};

    #[test]
    fn maps_image_references_to_docker_hub() {
        let r = |s: &str| hub_ref(s).map(|(a, b)| format!("{a}:{b}"));
        assert_eq!(r("cyberctf/attack-box").as_deref(), Some("cyberctf/attack-box:latest"));
        assert_eq!(r("nwodtuhs/exegol:free").as_deref(), Some("nwodtuhs/exegol:free"));
        assert_eq!(r("kali").as_deref(), Some("library/kali:latest"));
        assert_eq!(r("docker.io/kalilinux/kali-rolling:2026.3").as_deref(), Some("kalilinux/kali-rolling:2026.3"));
        assert_eq!(r("ghcr.io/org/image:1"), None);
        assert_eq!(r("localhost:5000/x"), None);
    }

    #[test]
    fn the_size_is_this_architectures_and_never_zero() {
        // As Docker Hub answers it (fields trimmed to a few).
        let tag: HubTag = serde_json::from_str(
            r#"{"creator":1,"id":2,"images":[{"architecture":"amd64","features":"","variant":null,"digest":"sha256:aa","os":"linux","os_features":"","os_version":null,"size":1000,"status":"active","last_pulled":"2026-11-01T00:00:00Z","last_pushed":"2026-10-01T00:00:00Z"},{"architecture":"arm64","digest":"sha256:bb","os":"linux","size":0,"status":"active"}],"last_updated":"2026-10-01T00:00:00Z","name":"latest","repository":3,"full_size":1000,"v2":true,"tag_status":"active","digest":"sha256:cc"}"#,
        )
        .unwrap();
        assert_eq!(size_for_arch(&tag, "amd64"), Some(1000));
        assert_eq!(size_for_arch(&tag, "arm64"), None);
        assert_eq!(size_for_arch(&tag, "riscv64"), None);
        assert_eq!(size_for_arch(&HubTag::default(), "amd64"), None);
        assert_ne!(hub_arch(), "");
        assert_eq!(hub_ref("a/b@sha256:abc").map(|r| r.1).as_deref(), Some("latest"));
    }

    #[tokio::test]
    async fn an_invalid_or_foreign_image_has_no_size_without_asking() {
        assert_eq!(image_download_size("bad image; rm -rf /".into()).await, None);
        assert_eq!(image_download_size("ghcr.io/org/image:1".into()).await, None);
    }
}
