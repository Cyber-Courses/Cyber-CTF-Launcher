//! The container self-test: a throwaway two-container lab.

use std::path::Path;
use std::time::Duration;

use super::Reporter;
use super::downloads::ensure_image;
use crate::error::{Error, Result};
use crate::platform::exec::run_env_timed;

const PROJECT: &str = "cyberctf-selftest";
pub(crate) const IMAGE: &str = "busybox:1.36";
/// What the test web server serves, so a reply is known to come from it.
const MARKER: &str = "cyberctf-selftest-ok";
/// Tries (500 ms apart) for the lab's web server to answer: httpd needs a moment.
const ANSWER_TRIES: u32 = 10;
const RETRY: Duration = Duration::from_millis(500);

fn compose_file() -> String {
    format!(
        r#"services:
  web:
    image: {IMAGE}
    command: ["sh", "-c", "mkdir -p /www && echo {MARKER} > /www/index.html && httpd -f -p 80 -h /www"]
    ports: ["127.0.0.1::80"]
  probe:
    image: {IMAGE}
    command: ["sleep", "300"]
"#
    )
}

/// How long the test lab gets to start (its image is local by then), and each probe and the
/// clean-up to answer: a wedged engine fails the step instead of hanging the test before its
/// clean-up.
const START_LIMIT: Duration = Duration::from_secs(180);
const PROBE_LIMIT: Duration = Duration::from_secs(20);
const DOWN_LIMIT: Duration = Duration::from_secs(60);

async fn compose(dir: &Path, rest: &[&str], limit: Duration) -> Result<String> {
    let mut args = vec!["compose", "-p", PROJECT, "-f", "docker-compose.yml"];
    args.extend_from_slice(rest);
    run_env_timed("docker", &args, Some(dir), &[], limit).await
}

pub(super) async fn run_test(dir: &Path, r: &Reporter) -> Result<()> {
    std::fs::write(dir.join("docker-compose.yml"), compose_file())?;
    let res = steps(dir, r).await;
    // Always tear the test lab down, even after a failed step.
    let down = compose(dir, &["down", "-v", "--remove-orphans", "-t", "0"], DOWN_LIMIT).await;
    r.cleanup("Clean up", &res, down, None);
    res
}

async fn steps(dir: &Path, r: &Reporter) -> Result<()> {
    r.step("engine", "Container engine answers", async {
        let v = crate::platform::exec::run_read("docker", &["info", "--format", "{{.ServerVersion}}"], None).await?;
        Ok(((), Some(format!("Docker {}", v.trim()))))
    })
    .await?;

    r.step("pull", "Download a test image", async {
        let had = ensure_image().await?;
        Ok(((), Some(if had { format!("{IMAGE} (ready)") } else { IMAGE.to_string() })))
    })
    .await?;

    r.step("start", "Start a two-container test lab", async {
        compose(dir, &["up", "-d", "--quiet-pull"], START_LIMIT).await?;
        Ok(((), None))
    })
    .await?;

    r.step("network", "Containers reach each other", async {
        let mut last = String::new();
        for _ in 0..ANSWER_TRIES {
            match compose(dir, &["exec", "-T", "probe", "wget", "-qO-", "-T", "2", "http://web"], PROBE_LIMIT).await {
                Ok(out) if out.contains(MARKER) => return Ok(((), Some("probe → web on the lab network".into()))),
                Ok(out) => last = out,
                Err(e) => last = e.to_string(),
            }
            tokio::time::sleep(RETRY).await;
        }
        Err(Error::Invalid(format!("probe could not reach web: {last}")))
    })
    .await?;

    r.step("port", "Lab port reachable from this machine", async {
        let addr = compose(dir, &["port", "web", "80"], PROBE_LIMIT).await?;
        let url = format!("http://{}", addr.trim());
        let client = reqwest::Client::builder().timeout(Duration::from_secs(3)).build().map_err(|e| Error::Invalid(e.to_string()))?;
        let mut last = String::new();
        for _ in 0..ANSWER_TRIES {
            match client.get(&url).send().await {
                Ok(res) => match res.text().await {
                    Ok(body) if body.contains(MARKER) => return Ok(((), Some(addr.trim().to_string()))),
                    Ok(body) => last = body,
                    Err(e) => last = e.to_string(),
                },
                Err(e) => last = e.to_string(),
            }
            tokio::time::sleep(RETRY).await;
        }
        Err(Error::Invalid(format!("{url} did not answer: {last}")))
    })
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_test_lab_serves_its_marker_on_a_loopback_port() {
        let f = super::compose_file();
        assert!(f.contains("image: busybox:1.36") && f.contains("echo cyberctf-selftest-ok") && f.contains("127.0.0.1::80"), "{f}");
    }

    #[test]
    fn the_compose_file_is_yaml_whose_shell_command_carries_the_marker_whole() {
        // The web command is a `sh -c` script inside a YAML flow sequence inside a double-quoted
        // scalar: it must parse as three list items, with the redirect intact in the third, and
        // the ports entry must stay a string (YAML would read `127.0.0.1::80` oddly unquoted).
        let f = super::compose_file();
        assert!(!f.contains('\t'), "YAML forbids tabs in indentation");
        let doc: serde_yaml_ng::Value = serde_yaml_ng::from_str(&f).unwrap();
        let web = &doc["services"]["web"];
        let command: Vec<&str> = web["command"].as_sequence().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        assert_eq!(command.len(), 3);
        assert_eq!(&command[..2], ["sh", "-c"]);
        assert!(command[2].contains(&format!("echo {} > /www/index.html", super::MARKER)) && command[2].ends_with("httpd -f -p 80 -h /www"), "{}", command[2]);
        assert_eq!(web["ports"][0].as_str(), Some("127.0.0.1::80"));
        assert_eq!(doc["services"]["probe"]["command"][0].as_str(), Some("sleep"));
    }
}
