//! Unit tests' stand-in for the tools: while a test holds a [`Tools`] guard, the test's machine
//! runs them through [`AsRunner`], a runner answering every call on its thread from the test's
//! script instead of starting a process, so code that drives docker, vagrant or terraform runs
//! without them (and never touches real ones).
//! Async tests run on one thread (`#[tokio::test]`), so the script is per thread: parallel tests
//! don't see each other's.

use std::cell::RefCell;
use std::rc::Rc;

/// A scripted tool's answer: its stdout (each line streamed, for `stream`), or its failure.
#[derive(Clone)]
pub enum Reply {
    Out(String),
    Fail(String),
}

/// A script's answer for a tool that printed `stdout`.
#[expect(clippy::unnecessary_wraps, reason = "a script answers an Option, None being a missing tool")]
pub fn ok(stdout: impl Into<String>) -> Option<Reply> {
    Some(Reply::Out(stdout.into()))
}

/// A script's answer for a tool that failed, printing `stderr`.
#[expect(clippy::unnecessary_wraps, reason = "a script answers an Option, None being a missing tool")]
pub fn fail(stderr: impl Into<String>) -> Option<Reply> {
    Some(Reply::Fail(stderr.into()))
}

/// The Azure CLI as testing an Azure account reads it: signed in to the "Pay-As-You-Go"
/// subscription, with no VM size or usage listed, and Terraform there.
pub fn azure_signed_in() -> [(&'static str, Option<Reply>); 4] {
    [("az vm list-skus", ok("")), ("az vm list-usage", ok("")), ("terraform version", ok("")), ("account show", ok("Pay-As-You-Go\n"))]
}

/// What a scripted call got: the script's reply, or nothing (the tool is missing).
enum Scripted {
    Reply(Reply),
    Missing,
}

type Answer = Box<dyn FnMut(&str, &[&str]) -> Option<Reply>>;

struct State {
    script: Answer,
    calls: Vec<String>,
    /// The environment each call was given, beside `calls`.
    envs: Vec<Vec<(String, String)>>,
}

thread_local! {
    static STATE: RefCell<Option<Rc<RefCell<State>>>> = const { RefCell::new(None) };
    /// The tool set (`exec::ToolSet`) of this thread's script: never the machine's (0), never
    /// another script's. Apart from the script's state, which a call the script makes may hold.
    static SET: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

/// The scripted tools of the running test, until dropped. A call the script doesn't answer
/// (`None`) is a missing tool.
pub struct Tools(Rc<RefCell<State>>, Option<Rc<dyn Checked>>);

/// How often a scripted call must be made, checked when the tools are dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Times {
    AtLeastOnce,
    Exactly(usize),
    Any,
}

/// One scripted call: a program and the arguments its call starts with (`*` is any one
/// argument), and its answer (`None`: the tool is missing).
#[derive(Clone)]
struct Expectation {
    program: String,
    args: Vec<String>,
    reply: Option<Reply>,
    times: Times,
}

impl Expectation {
    fn matches(&self, program: &str, args: &[&str]) -> bool {
        program == self.program && args.len() >= self.args.len() && self.args.iter().zip(args).all(|(want, got)| want == "*" || want == got)
    }

    fn describe(&self) -> String {
        std::iter::once(self.program.as_str()).chain(self.args.iter().map(String::as_str)).collect::<Vec<_>>().join(" ")
    }
}

/// A strict script for [`Tools`]: a call no expectation matches panics (and is reported again
/// when the tools are dropped, should a task have swallowed the panic); an expectation not met
/// panics when they are dropped; of two expectations matching a call the one naming more
/// arguments answers, two naming as many panic. The order it is written in never matters.
#[derive(Clone, Default)]
#[must_use]
pub struct Script(Vec<Expectation>);

impl Script {
    fn with(mut self, call: &str, reply: Option<Reply>, times: Times) -> Script {
        let mut words = call.split_whitespace().map(str::to_string);
        let program = words.next().expect("a scripted call names its program");
        self.0.push(Expectation { program, args: words.collect(), reply, times });
        self
    }

    /// `call` (`program arg…`, its leading arguments; `*` is any one) is made at least once, and
    /// answered `reply` (`ok(..)`, `fail(..)`, or `None`: missing).
    pub fn expect(self, call: &str, reply: Option<Reply>) -> Script {
        self.with(call, reply, Times::AtLeastOnce)
    }

    /// `call` is made exactly `times` times.
    pub fn expect_times(self, call: &str, times: usize, reply: Option<Reply>) -> Script {
        self.with(call, reply, Times::Exactly(times))
    }

    /// `call` may be made, any number of times.
    pub fn allow(self, call: &str, reply: Option<Reply>) -> Script {
        self.with(call, reply, Times::Any)
    }

    /// The tools of this thread answering from this script, until dropped.
    pub fn build(self) -> Tools {
        let strict = Rc::new(Strict { used: RefCell::new(vec![0; self.0.len()]), unscripted: RefCell::new(Vec::new()), script: self });
        let answering = strict.clone();
        let mut tools = Tools::new(move |program, args| answering.answer(program, args));
        tools.1 = Some(strict);
        tools
    }
}

/// What a strict fake has to say when the tools go: the calls it had no answer for, and the
/// expectations it wasn't given.
trait Checked {
    fn unmet(&self) -> Vec<String>;
}

/// A strict script's bookkeeping.
struct Strict {
    script: Script,
    used: RefCell<Vec<usize>>,
    unscripted: RefCell<Vec<String>>,
}

impl Strict {
    fn answer(&self, program: &str, args: &[&str]) -> Option<Reply> {
        let line = std::iter::once(program).chain(args.iter().copied()).collect::<Vec<_>>().join(" ");
        let matching: Vec<(usize, &Expectation)> = self.script.0.iter().enumerate().filter(|(_, e)| e.matches(program, args)).collect();
        let most = matching.iter().map(|(_, e)| e.args.len()).max();
        let best: Vec<(usize, &Expectation)> = matching.into_iter().filter(|(_, e)| Some(e.args.len()) == most).collect();
        let why = match best.as_slice() {
            [(i, e)] => {
                self.used.borrow_mut()[*i] += 1;
                return e.reply.clone();
            }
            [] => format!(
                "unscripted call `{line}`; the script expects:\n{}",
                self.script.0.iter().map(|e| format!("  {}", e.describe())).collect::<Vec<_>>().join("\n")
            ),
            many => format!(
                "call `{line}` matches {} expectations equally: {}",
                many.len(),
                many.iter().map(|(_, e)| format!("`{}`", e.describe())).collect::<Vec<_>>().join(", ")
            ),
        };
        self.unscripted.borrow_mut().push(why.clone());
        panic!("{why}");
    }
}

impl Checked for Strict {
    /// What wasn't run as scripted.
    fn unmet(&self) -> Vec<String> {
        let mut unmet = self.unscripted.borrow().clone();
        for (e, &n) in self.script.0.iter().zip(self.used.borrow().iter()) {
            let met = match e.times {
                Times::AtLeastOnce => n > 0,
                Times::Exactly(times) => n == times,
                Times::Any => true,
            };
            if !met {
                unmet.push(format!("`{}` was expected {:?}, made {n} time(s)", e.describe(), e.times));
            }
        }
        unmet
    }
}

/// [`Tools::strict_rules`]' bookkeeping.
struct SubstringRules {
    rules: Vec<(String, Option<Reply>)>,
    unscripted: RefCell<Vec<String>>,
}

impl SubstringRules {
    fn answer(&self, program: &str, args: &[&str]) -> Option<Reply> {
        let line = std::iter::once(program).chain(args.iter().copied()).collect::<Vec<_>>().join(" ");
        let matching: Vec<&(String, Option<Reply>)> = self.rules.iter().filter(|(k, _)| line.contains(k.as_str())).collect();
        let longest = matching.iter().map(|(k, _)| k.len()).max();
        let best: Vec<&&(String, Option<Reply>)> = matching.iter().filter(|(k, _)| Some(k.len()) == longest).collect();
        let why = match best.as_slice() {
            [(_, reply)] => return reply.clone(),
            [] => format!("unscripted call `{line}`; the rules are:\n{}", self.rules.iter().map(|(k, _)| format!("  {k}")).collect::<Vec<_>>().join("\n")),
            many => {
                format!("call `{line}` matches {} rules equally: {}", many.len(), many.iter().map(|(k, _)| format!("`{k}`")).collect::<Vec<_>>().join(", "))
            }
        };
        self.unscripted.borrow_mut().push(why.clone());
        panic!("{why}");
    }
}

impl Checked for SubstringRules {
    fn unmet(&self) -> Vec<String> {
        self.unscripted.borrow().clone()
    }
}

impl Tools {
    /// Tools answered by `script`, a model of the tools that answers whatever it is asked.
    /// A test that scripts a few calls uses [`Tools::script`] instead.
    pub fn new(script: impl FnMut(&str, &[&str]) -> Option<Reply> + 'static) -> Tools {
        static SETS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let set = SETS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let state = Rc::new(RefCell::new(State { script: Box::new(script), calls: Vec::new(), envs: Vec::new() }));
        STATE.with(|s| *s.borrow_mut() = Some(state.clone()));
        SET.with(|s| s.set(Some(set)));
        Tools(state, None)
    }

    /// A strict script to build: see [`Script`].
    pub fn script() -> Script {
        Script::default()
    }

    /// Strict tools for `expected`, each `(call, reply)` made at least once (see [`Script`]).
    pub fn expect(expected: &[(&str, Option<Reply>)]) -> Tools {
        expected.iter().fold(Script::default(), |s, (call, reply)| s.expect(call, reply.clone())).build()
    }

    /// Every call made to `call` (`program arg…`, matched as a script matches: on the program and
    /// its leading arguments, never on a quoted line), each as its arguments after the program.
    pub fn made(&self, call: &str) -> Vec<Vec<String>> {
        let mut words = call.split_whitespace();
        let program = words.next().unwrap_or_default();
        let want: Vec<&str> = words.collect();
        self.calls()
            .iter()
            .filter_map(|line| {
                let mut got = line.split(' ');
                (got.next() == Some(program)).then(|| got.map(str::to_string).collect::<Vec<_>>())
            })
            .filter(|args| args.len() >= want.len() && want.iter().zip(args).all(|(w, g)| *w == "*" || w == g))
            .collect()
    }

    /// Tools answering by rule, strictly: each rule answers the calls (`program arg arg…`) that
    /// contain its text; of several, the longest text answers (so order never matters), two as
    /// long panic, and a call no rule answers panics (and is said again when the tools go)
    /// rather than succeeding. A rule no call used is fine: one test helper's rules serve several
    /// tests. For the tests moved off [`Tools::rules`]; a new test scripts with [`Tools::script`].
    pub fn strict_rules(rules: &[(&str, Option<Reply>)]) -> Tools {
        let checked = Rc::new(SubstringRules { rules: rules.iter().map(|(k, r)| (k.to_string(), r.clone())).collect(), unscripted: RefCell::new(Vec::new()) });
        let answering = checked.clone();
        let mut tools = Tools::new(move |program, args| answering.answer(program, args));
        tools.1 = Some(checked);
        tools
    }

    /// Lenient tools, for the tests written before [`Tools::script`]: the first rule whose text
    /// the call (`program arg arg…`) contains answers; any other call succeeds with no output,
    /// so such a test can't see a call it didn't expect. New tests script their calls strictly.
    pub fn rules(rules: &[(&str, Option<Reply>)]) -> Tools {
        let rules: Vec<(String, Option<Reply>)> = rules.iter().map(|(k, r)| (k.to_string(), r.clone())).collect();
        Tools::new(move |program, args| {
            let line = std::iter::once(program).chain(args.iter().copied()).collect::<Vec<_>>().join(" ");
            match rules.iter().find(|(k, _)| line.contains(k.as_str())) {
                Some((_, r)) => r.clone(),
                None => ok(""),
            }
        })
    }

    /// Every call so far, as `program arg arg…`.
    pub fn calls(&self) -> Vec<String> {
        self.0.borrow().calls.clone()
    }

    /// Whether a call containing `text` was made.
    pub fn called(&self, text: &str) -> bool {
        self.calls().iter().any(|c| c.contains(text))
    }

    /// The value of `name` in the environment of the last call containing `text`.
    pub fn env_of(&self, text: &str, name: &str) -> Option<String> {
        let st = self.0.borrow();
        let at = st.calls.iter().rposition(|c| c.contains(text))?;
        st.envs[at].iter().rev().find(|(k, _)| k == name).map(|(_, v)| v.clone())
    }
}

impl Drop for Tools {
    fn drop(&mut self) {
        STATE.with(|s| *s.borrow_mut() = None);
        SET.with(|s| s.set(None));
        let unmet = self.1.as_ref().map(|strict| strict.unmet()).unwrap_or_default();
        if std::thread::panicking() {
            return;
        }
        assert!(unmet.is_empty(), "the scripted tools weren't run as scripted:\n{}\ncalls made:\n{}", unmet.join("\n"), self.calls().join("\n"));
    }
}

/// The tool set of the script on this thread, when a test scripts the tools, else of the machine
/// the test set (`runtime::providers::testing::Machine`).
pub fn set() -> Option<u64> {
    SET.with(std::cell::Cell::get).or_else(crate::runtime::providers::testing::tool_set)
}

/// Whether a test scripts the tools on this thread (so nothing real would run).
pub fn scripted() -> bool {
    STATE.with(|s| s.borrow().is_some())
}

/// The runner `platform::exec` starts a test's tools through: the test's machine's
/// (`runtime::providers::testing`), scripted, never the real tools.
pub fn runner() -> std::sync::Arc<dyn isothrum_runner::ProcessRunner> {
    crate::runtime::providers::machine().runner
}

/// This thread's scripted tools ([`Tools`]) as an isothrum runner, for isothrum-machine's calls
/// in a test that scripts the launcher's tools: each call answered as the launcher's exec layer
/// would answer it (standard output on success; the reply on standard error, exit 1, on a
/// failure; missing when unscripted). A test that sets isothrum's own fakes doesn't need it.
pub struct AsRunner;

impl isothrum_runner::ProcessRunner for AsRunner {
    fn run<'a>(
        &'a self,
        spec: &'a isothrum_runner::ProcessSpec,
        on_line: isothrum_runner::OnLine<'a>,
    ) -> isothrum_runner::BoxFuture<'a, std::result::Result<isothrum_runner::Exit, isothrum_runner::ProcessError>> {
        let args: Vec<&str> = spec.args.iter().map(String::as_str).collect();
        let scripted = reply(&spec.program, &args, &spec.env);
        let answer = match scripted {
            Some(Scripted::Reply(Reply::Out(text))) => Some((isothrum_runner::Stream::Stdout, text, 0)),
            Some(Scripted::Reply(Reply::Fail(text))) => Some((isothrum_runner::Stream::Stderr, text, 1)),
            Some(Scripted::Missing) | None => None,
        };
        let program = spec.program.clone();
        Box::pin(async move {
            match answer {
                Some((stream, text, code)) => {
                    text.lines().for_each(|l| on_line(stream, l));
                    Ok(isothrum_runner::Exit::new(Some(code)))
                }
                None => Err(isothrum_runner::ProcessError::Missing { program }),
            }
        })
    }
}

/// The script's answer to `program args`, recorded with its environment; None when no test
/// scripts the tools on this thread.
fn reply(program: &str, args: &[&str], env: &[(String, String)]) -> Option<Scripted> {
    let state = STATE.with(|s| s.borrow().clone())?;
    let mut st = state.borrow_mut();
    st.calls.push(std::iter::once(program).chain(args.iter().copied()).collect::<Vec<_>>().join(" "));
    st.envs.push(env.to_vec());
    Some((st.script)(program, args).map_or(Scripted::Missing, Scripted::Reply))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The message `f` panics with.
    fn panic_of(f: impl FnOnce()) -> String {
        let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).expect_err("it panics");
        err.downcast_ref::<String>().cloned().or_else(|| err.downcast_ref::<&str>().map(|s| (*s).to_string())).unwrap_or_default()
    }

    /// `line` called on this thread's scripted tools (`_tools`, held so they answer).
    fn call(_tools: &Tools, line: &str) -> Option<Reply> {
        let mut words = line.split(' ');
        let program = words.next().unwrap();
        let args: Vec<&str> = words.collect();
        reply(program, &args, &[]).and_then(|s| match s {
            Scripted::Reply(r) => Some(r),
            Scripted::Missing => None,
        })
    }

    fn out(reply: Option<Reply>) -> Option<String> {
        match reply {
            Some(Reply::Out(text)) => Some(text),
            _ => None,
        }
    }

    #[test]
    fn a_scripted_call_is_answered_whatever_order_the_script_is_in() {
        let tools = Tools::script().allow("docker", ok("general")).allow("docker compose ps", ok("specific")).build();
        assert_eq!(out(call(&tools, "docker compose ps -q")).as_deref(), Some("specific"));
        assert_eq!(out(call(&tools, "docker info")).as_deref(), Some("general"));
        assert!(call(&Tools::expect(&[("vagrant", None)]), "vagrant up").is_none(), "None is a missing tool");
    }

    #[test]
    fn an_unscripted_call_panics_naming_it_and_the_script() {
        let tools = Tools::script().allow("docker compose ps", ok("")).build();
        let message = panic_of(|| drop(call(&tools, "docker info")));
        assert!(message.contains("unscripted call `docker info`") && message.contains("docker compose ps"), "{message}");
        let again = panic_of(move || drop(tools));
        assert!(again.contains("unscripted call `docker info`"), "reported again when dropped: {again}");
    }

    #[test]
    fn an_expectation_never_met_panics_when_the_tools_go() {
        let tools = Tools::script().expect("vagrant up", ok("")).build();
        let message = panic_of(move || drop(tools));
        assert!(message.contains("`vagrant up` was expected AtLeastOnce, made 0 time(s)"), "{message}");
    }

    #[test]
    fn a_call_made_more_often_than_its_count_panics_when_the_tools_go() {
        let tools = Tools::script().expect_times("vagrant status", 1, ok("")).build();
        call(&tools, "vagrant status");
        call(&tools, "vagrant status");
        let message = panic_of(move || drop(tools));
        assert!(message.contains("`vagrant status` was expected Exactly(1), made 2 time(s)"), "{message}");
    }

    #[test]
    fn strict_rules_answer_by_the_longest_text_and_refuse_a_call_none_answers() {
        let tools = Tools::strict_rules(&[("docker", ok("general")), ("compose ps", ok("specific"))]);
        assert_eq!(out(call(&tools, "docker compose ps -q")).as_deref(), Some("specific"));
        assert_eq!(out(call(&tools, "docker info")).as_deref(), Some("general"));
        let message = panic_of(|| drop(call(&tools, "vagrant up")));
        assert!(message.contains("unscripted call `vagrant up`") && message.contains("compose ps"), "{message}");
        let again = panic_of(move || drop(tools));
        assert!(again.contains("unscripted call `vagrant up`"), "reported again when dropped: {again}");
    }

    #[test]
    fn strict_rules_matching_a_call_equally_panic() {
        let tools = Tools::strict_rules(&[("compose", ok("a")), ("ps -q x", ok("b"))]);
        let message = panic_of(|| drop(call(&tools, "docker compose ps -q x")));
        assert!(message.contains("matches 2 rules equally"), "{message}");
        panic_of(move || drop(tools));
    }

    #[test]
    fn two_expectations_matching_a_call_equally_panic() {
        let tools = Tools::script().allow("docker * ps", ok("a")).allow("docker compose *", ok("b")).build();
        let message = panic_of(|| drop(call(&tools, "docker compose ps")));
        assert!(message.contains("matches 2 expectations equally"), "{message}");
        panic_of(move || drop(tools));
    }

    #[test]
    fn the_calls_made_are_read_by_program_and_arguments() {
        let tools = Tools::script().allow("docker", ok("")).build();
        call(&tools, "docker compose -p lab up");
        call(&tools, "docker info");
        assert_eq!(tools.made("docker compose -p * up"), [vec!["compose", "-p", "lab", "up"]]);
        assert_eq!(tools.made("dockerd"), Vec::<Vec<String>>::new());
    }
}
