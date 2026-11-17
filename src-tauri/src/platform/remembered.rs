//! Answers of the tools that rarely change (`vagrant --version`, `vagrant plugin list`), kept a
//! while instead of asked on every machine check: each is a process start, on Windows a Ruby one
//! of a second or two of CPU, and the Machine page checks the machine again whenever it is shown.
//! Only an answer that found the tool is kept: one installed since is found at the next check.
//! Installing or removing a tool from the app forgets them all ([`forget_all`]). An answer is kept
//! for the tools that gave it (`exec::ToolSet`): a unit test's scripted tools never read the
//! machine's, nor another test's.

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use crate::platform::exec::{ToolSet, tools_in_use};

/// How long a kept answer stands: a tool changed outside the app shows up after this at most.
pub const KEPT_FOR: Duration = Duration::from_secs(600);

/// The kept answers, by the tools that gave them and key, with when each was asked.
type Kept = BTreeMap<(ToolSet, String), (Instant, String)>;

/// The kept answers.
fn kept() -> MutexGuard<'static, Kept> {
    static KEPT: Mutex<Kept> = Mutex::new(BTreeMap::new());
    KEPT.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The answer kept for `key` if it is younger than `KEPT_FOR`, else `ask`'s, kept when it is `Ok`.
///
/// # Errors
///
/// `ask`'s, which isn't kept: the next call asks again.
pub async fn remembered<E>(key: &str, ask: impl Future<Output = Result<String, E>>) -> Result<String, E> {
    remembered_at(key, Instant::now(), ask).await
}

/// [`remembered`] at time `now`.
async fn remembered_at<E>(key: &str, now: Instant, ask: impl Future<Output = Result<String, E>>) -> Result<String, E> {
    let key = (tools_in_use(), key.to_string());
    if let Some((at, answer)) = kept().get(&key)
        && now.saturating_duration_since(*at) < KEPT_FOR
    {
        return Ok(answer.clone());
    }
    let answer = ask.await?;
    kept().insert(key, (now, answer.clone()));
    Ok(answer)
}

/// The programs whose `--version` the machine check keeps ([`Remembering`]): the ones it read
/// through [`remembered`] before isothrum-machine ran its probes. A hypervisor's is never kept:
/// its probe also tells a wedged service.
const KEPT_VERSIONS: [&str; 4] = ["docker", "vagrant", "aws", "gcloud"];

/// isothrum-machine's runner, the `--version` answers of [`KEPT_VERSIONS`] kept as [`remembered`]
/// keeps them (one that failed is asked again next time); every other call goes to `inner`.
pub struct Remembering {
    inner: std::sync::Arc<dyn isothrum_runner::ProcessRunner>,
}

impl Remembering {
    /// `inner`, its version answers kept.
    pub fn new(inner: std::sync::Arc<dyn isothrum_runner::ProcessRunner>) -> Remembering {
        Remembering { inner }
    }
}

/// Why a version wasn't kept: the tool didn't run, or didn't exit 0.
enum NotKept {
    Failed(isothrum_runner::ProcessError),
    Exited(isothrum_runner::Exit),
}

impl isothrum_runner::ProcessRunner for Remembering {
    fn run<'a>(
        &'a self,
        spec: &'a isothrum_runner::ProcessSpec,
        on_line: isothrum_runner::OnLine<'a>,
    ) -> isothrum_runner::BoxFuture<'a, std::result::Result<isothrum_runner::Exit, isothrum_runner::ProcessError>> {
        if spec.args != ["--version"] || !KEPT_VERSIONS.contains(&spec.program.as_str()) || !spec.env.is_empty() {
            return self.inner.run(spec, on_line);
        }
        Box::pin(async move {
            let ask = async {
                let mut stdout = Vec::new();
                let exit = self
                    .inner
                    .run(spec, &mut |stream, line| {
                        if stream == isothrum_runner::Stream::Stdout {
                            stdout.push(line.to_owned());
                        }
                    })
                    .await
                    .map_err(NotKept::Failed)?;
                if exit.success() { Ok(stdout.join("\n")) } else { Err(NotKept::Exited(exit)) }
            };
            match remembered(&spec.line(), ask).await {
                Ok(out) => {
                    for line in out.lines() {
                        on_line(isothrum_runner::Stream::Stdout, line);
                    }
                    Ok(isothrum_runner::Exit::new(Some(0)))
                }
                Err(NotKept::Exited(exit)) => Ok(exit),
                Err(NotKept::Failed(e)) => Err(e),
            }
        })
    }
}

/// Forgets every kept answer: a tool was installed or removed.
pub fn forget_all() {
    kept().clear();
}

/// Keeps `answer` for `key` now, as a machine check would: for a test of who forgets it.
#[cfg(test)]
pub fn keep(key: &str, answer: &str) {
    kept().insert((tools_in_use(), key.to_string()), (Instant::now(), answer.to_string()));
}

/// Whether an answer is kept for `key`.
#[cfg(test)]
pub fn is_kept(key: &str) -> bool {
    kept().contains_key(&(tools_in_use(), key.to_string()))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use isothrum_runner::fake::{FakeProcesses, Reply};
    use isothrum_runner::{ProcessRunner, ProcessSpec, capture};

    use super::*;

    /// A version kept by the machine check's runner is asked once; another call every time, and
    /// one that failed is asked again.
    #[tokio::test]
    async fn the_checks_runner_keeps_the_versions_it_kept_before() {
        forget_all();
        let fake = Arc::new(
            FakeProcesses::script()
                .expect_times("gcloud --version", 1, Reply::ok("Google Cloud SDK 500.0.0\nbq 2.1"))
                .expect_times("aws --version", 2, Reply::fail(1, "broken"))
                .expect_times("VBoxManage --version", 2, Reply::ok("7.1.4r165100"))
                .build(),
        );
        let inner: Arc<dyn ProcessRunner> = Arc::<FakeProcesses>::clone(&fake);
        let runner = Remembering::new(inner);
        for _ in 0..2 {
            let gcloud = capture(&runner, &ProcessSpec::new("gcloud", ["--version"])).await.unwrap();
            assert_eq!((gcloud.success(), gcloud.stdout.as_slice()), (true, ["Google Cloud SDK 500.0.0", "bq 2.1"].map(String::from).as_slice()));
            assert!(!capture(&runner, &ProcessSpec::new("aws", ["--version"])).await.unwrap().success());
            assert!(capture(&runner, &ProcessSpec::new("VBoxManage", ["--version"])).await.unwrap().success());
        }
        forget_all();
    }

    #[tokio::test]
    async fn an_answer_is_kept_a_while_a_failure_never() {
        let key = format!("test-{}", rand::random::<u32>());
        let start = Instant::now();
        let asked = std::cell::Cell::new(0);
        let ask = |answer: Result<&'static str, ()>| {
            let asked = &asked;
            async move {
                asked.set(asked.get() + 1);
                answer.map(str::to_string)
            }
        };
        assert_eq!(remembered_at(&key, start, ask(Err(()))).await, Err(()));
        assert_eq!(remembered_at(&key, start, ask(Ok("Vagrant 2.4.9"))).await.unwrap(), "Vagrant 2.4.9");
        assert_eq!(asked.get(), 2, "a failure isn't kept: asked again");
        let later = start + KEPT_FOR.saturating_sub(Duration::from_secs(1));
        assert_eq!(remembered_at(&key, later, ask(Err(()))).await.unwrap(), "Vagrant 2.4.9");
        assert_eq!(asked.get(), 2, "the kept answer stood: not asked");
        assert_eq!(remembered_at(&key, start + KEPT_FOR, ask(Ok("Vagrant 2.5.0"))).await.unwrap(), "Vagrant 2.5.0", "asked again once old");
        forget_all();
        assert_eq!(remembered_at(&key, start + KEPT_FOR, ask(Ok("Vagrant 2.5.1"))).await.unwrap(), "Vagrant 2.5.1", "forgotten after an install");
    }
}
