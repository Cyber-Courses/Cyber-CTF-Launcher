use super::*;

/// A process as a record names it, with made-up numbers.
fn proc(pid: u32, boot: u128, start: u64) -> Proc {
    Proc { pid, id: Identity { boot, start } }
}

#[test]
fn a_record_reads_back_from_its_line_and_a_damaged_one_reads_as_none() {
    let owner = proc(4242, 0xab, 987_654);
    for record in [Record { owner, tool: proc(77, 0xab, 990_000), stream: true }, Record { owner, tool: proc(78, 0xab, 990_001), stream: false }] {
        assert_eq!(Record::parse(&record.line()), Some(record));
    }
    let tool = Record { owner, tool: proc(77, 0xab, 990_000), stream: true }.line();
    for damaged in [
        "",
        "tool",
        &tool[..tool.len() - 1],
        &tool.replace("stream", "maybe"),
        &tool.replace("tool ", "toll "),
        &tool.replace('\n', " more\n"),
        "spawn 4242:ab-987654 12 \n",
        "spawn 4242:ab-987654 x docker\n",
        "spawn 4242 12 docker\n",
    ] {
        assert_eq!(Record::parse(damaged), None, "{damaged:?}");
    }
}

#[cfg(unix)]
mod ending {
    use std::os::unix::process::CommandExt as _;

    use super::*;

    /// A long tool in a process group of its own, as `spawn_group` starts one.
    fn tool() -> std::process::Child {
        std::process::Command::new("sleep").arg("60").process_group(0).spawn().unwrap()
    }

    /// The process that recorded a tool, as a dead one would read: this process's id, with an
    /// identity that isn't this process's.
    fn dead_owner() -> Proc {
        let me = me().unwrap();
        Proc { pid: me.pid, id: Identity { start: me.id.start + 1, ..me.id } }
    }

    fn records(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().into_string().unwrap()).collect();
        names.sort();
        names
    }

    fn exited(child: &mut std::process::Child) -> bool {
        (0..100).any(|_| {
            let done = child.try_wait().unwrap().is_some();
            if !done {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            done
        })
    }

    /// Regression (#301): the tools of an app killed outright ran on, in groups of their own. The
    /// next start ends what a dead owner recorded, a streamed tool asked first, and removes its
    /// records; a live owner's tools run on, and a record whose id names another process now
    /// ends nothing.
    #[tokio::test]
    async fn the_next_start_ends_the_tools_a_dead_owner_left_and_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        let (mut probe, mut streamed, mut mine, mut other) = (tool(), tool(), tool(), tool());
        let dead = dead_owner();
        let of = |c: &std::process::Child| Proc::of(c.id()).unwrap();
        let kept = write(dir.path(), me().unwrap().pid, &Record { owner: me().unwrap(), tool: of(&mine), stream: false }).unwrap();
        for record in [
            Record { owner: dead, tool: of(&probe), stream: false },
            Record { owner: dead, tool: of(&streamed), stream: true },
            // Its id names another process now (`other`, a later start).
            Record { owner: dead, tool: Proc { pid: other.id(), id: Identity { start: of(&other).id.start + 1, ..of(&other).id } }, stream: false },
        ] {
            std::mem::forget(write(dir.path(), dead.pid, &record).unwrap());
        }
        let mut ended = end_orphans(dir.path()).await;
        ended.sort_unstable();
        let mut expected = vec![probe.id(), streamed.id()];
        expected.sort_unstable();
        assert_eq!(ended, expected);
        assert!(exited(&mut probe) && exited(&mut streamed), "the dead owner's tools ended");
        assert!(mine.try_wait().unwrap().is_none() && other.try_wait().unwrap().is_none(), "the others run on");
        assert_eq!(records(dir.path()), [kept.0.file_name().unwrap().to_str().unwrap()], "only the live owner's record is left");
        drop(kept);
        assert!(records(dir.path()).is_empty(), "a record goes with its tool");
        for mut c in [mine, other] {
            c.kill().unwrap();
            c.wait().unwrap();
        }
    }

    /// A record cut short as its owner died is removed; one being written by a live owner isn't.
    #[tokio::test]
    async fn a_damaged_record_goes_once_its_owner_is_gone() {
        let dir = tempfile::tempdir().unwrap();
        let mut gone = std::process::Command::new("true").spawn().unwrap();
        gone.wait().unwrap();
        std::fs::write(dir.path().join(format!("{}-0.tool", gone.id())), "tool 1").unwrap();
        let mine = format!("{}-0.tool", std::process::id());
        std::fs::write(dir.path().join(&mine), "").unwrap();
        std::fs::write(dir.path().join("notes.txt"), "not a record").unwrap();
        assert_eq!(end_orphans(dir.path()).await, [] as [u32; 0]);
        assert_eq!(records(dir.path()), [mine, "notes.txt".to_string()]);
    }

    /// A streamed tool that won't stop when asked is killed once the grace has passed.
    #[tokio::test(start_paused = true)]
    async fn a_streamed_tool_ignoring_the_stop_is_killed_after_the_grace() {
        let mut stubborn = std::process::Command::new("sh").args(["-c", "trap '' TERM; sleep 60"]).process_group(0).spawn().unwrap();
        // Its shell has set the trap before it is asked.
        std::thread::sleep(std::time::Duration::from_millis(300));
        let tool = Proc::of(stubborn.id()).unwrap();
        signal(tool.pid, libc::SIGTERM);
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(stubborn.try_wait().unwrap().is_none(), "it ignores SIGTERM");
        kill_once_stopped(vec![tool]).await;
        assert!(exited(&mut stubborn), "killed after the grace");
    }
}
