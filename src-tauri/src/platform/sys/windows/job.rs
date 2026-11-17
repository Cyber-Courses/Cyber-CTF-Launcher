//! Job objects: a tool's whole process tree, ended with it, and the tool created in one.

use std::ffi::{OsStr, OsString, c_void};
use std::os::windows::ffi::OsStrExt as _;
use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, HANDLE_FLAG_INHERIT, SetHandleInformation, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::System::JobObjects::{
    CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_BASIC_LIMIT_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, CreateProcessW, DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT, GetExitCodeProcess,
    InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST, PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROC_THREAD_ATTRIBUTE_JOB_LIST, PROCESS_INFORMATION,
    STARTF_USESTDHANDLES, STARTUPINFOEXW, UpdateProcThreadAttribute, WaitForSingleObject,
};

/// A job object a tool's process (and every process it starts) runs in.
pub struct Job(HANDLE);

// SAFETY: a job handle is a kernel handle, usable from any thread.
unsafe impl Send for Job {}
// SAFETY: as above; every call on it is a kernel call the OS serializes.
unsafe impl Sync for Job {}

impl Job {
    /// A new job, ending every process in it when its last handle closes (this process's exit,
    /// however it exits).
    pub fn new() -> std::io::Result<Job> {
        // SAFETY: no attributes, no name: a new job's handle, or null.
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if job.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let job = Job(job);
        if !job.set_limit_flags(JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE) {
            return Err(std::io::Error::last_os_error());
        }
        Ok(job)
    }

    /// Sets the job's limits to `flags` (none: 0); whether Windows took them.
    fn set_limit_flags(&self, flags: u32) -> bool {
        let info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
            BasicLimitInformation: JOBOBJECT_BASIC_LIMIT_INFORMATION { LimitFlags: flags, ..JOBOBJECT_BASIC_LIMIT_INFORMATION::default() },
            ..JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default()
        };
        let Ok(size) = u32::try_from(std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>()) else { return false };
        // SAFETY: `info` is a valid, fully initialised struct of the size given.
        unsafe { SetInformationJobObject(self.0, JobObjectExtendedLimitInformation, std::ptr::from_ref(&info).cast(), size) != 0 }
    }

    /// Ends every process in the job.
    pub fn kill(&self) {
        // SAFETY: a live job handle.
        unsafe {
            TerminateJobObject(self.0, 1);
        }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: the handle is ours and closed once.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

/// Ends process `pid` (a test's leftover).
#[cfg(test)]
pub fn terminate(pid: u32) {
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_TERMINATE, TerminateProcess};
    // SAFETY: a process handle opened, used and closed here.
    unsafe {
        let process = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if !process.is_null() {
            TerminateProcess(process, 1);
            CloseHandle(process);
        }
    }
}

/// When process `pid` was created (`GetProcessTimes`, 100 ns ticks since 1601): stored by the
/// kernel at its creation, so a clock set later doesn't move it. None for a process gone or not
/// readable.
pub fn process_created(pid: u32) -> Option<u64> {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    let zero = || FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
    let (mut created, mut exited, mut kernel, mut user) = (zero(), zero(), zero(), zero());
    // SAFETY: a process handle opened, used and closed here; the four FILETIMEs are writable.
    let read = unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }
        let read = GetProcessTimes(process, &raw mut created, &raw mut exited, &raw mut kernel, &raw mut user);
        CloseHandle(process);
        read
    };
    (read != 0).then(|| (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
}

/// A tool created in its job: its id and process handle, and the read ends of its output.
pub struct Created {
    /// Its process id.
    pub pid: u32,
    /// Its process handle, which tells when it exited and how.
    pub process: OwnedHandle,
    /// The read end of its standard output.
    pub stdout: OwnedHandle,
    /// The read end of its standard error.
    pub stderr: OwnedHandle,
}

/// Creates `application` (`command_line` its command line, `vars` its whole environment, `cwd`
/// its working folder) inside `job`, headless: the kernel puts it there as it creates it
/// (`PROC_THREAD_ATTRIBUTE_JOB_LIST`), so there is no moment it runs, or waits suspended, outside
/// it. Its input is the null device and its output two pipes, the only handles it inherits
/// (`PROC_THREAD_ATTRIBUTE_HANDLE_LIST`).
pub fn create_in_job(application: &OsStr, command_line: &OsStr, vars: &[(OsString, OsString)], cwd: Option<&OsStr>, job: &Job) -> std::io::Result<Created> {
    let (stdout, stdout_end) = std::io::pipe()?;
    let (stderr, stderr_end) = std::io::pipe()?;
    let ends: [OwnedHandle; 3] = [std::fs::File::open("NUL")?.into(), stdout_end.into(), stderr_end.into()];
    for end in &ends {
        inheritable(end)?;
    }
    let application = wide(application);
    let mut line = wide(command_line);
    let environment = block(vars);
    let cwd = cwd.map(wide);
    let jobs = [job.0];
    let inherited: [HANDLE; 3] = ends.each_ref().map(std::os::windows::io::AsRawHandle::as_raw_handle);
    let mut attributes = Attributes::new(2)?;
    attributes.set(PROC_THREAD_ATTRIBUTE_JOB_LIST, &jobs)?;
    attributes.set(PROC_THREAD_ATTRIBUTE_HANDLE_LIST, &inherited)?;
    // SAFETY: plain data structures, all-zero is their empty value.
    let mut startup: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
    startup.StartupInfo.cb = u32::try_from(std::mem::size_of::<STARTUPINFOEXW>()).map_err(std::io::Error::other)?;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    let [stdin_end, out_end, err_end] = inherited;
    startup.StartupInfo.hStdInput = stdin_end;
    startup.StartupInfo.hStdOutput = out_end;
    startup.StartupInfo.hStdError = err_end;
    startup.lpAttributeList = attributes.list();
    // SAFETY: as above.
    let mut info: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    let flags = CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT;
    // SAFETY: every pointer is to a buffer alive for the call (`application`, `line`, which the
    // call may write to, `environment`, `cwd`, `startup` and its attribute list, which `jobs` and
    // `inherited` back); the handles in them are open, owned by `job` and `ends`. On success
    // `info` holds two handles opened for this process, owned below.
    let created = unsafe {
        CreateProcessW(
            application.as_ptr(),
            line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
            flags,
            environment.as_ptr().cast::<c_void>(),
            cwd.as_ref().map_or(std::ptr::null(), Vec::as_ptr),
            &raw const startup.StartupInfo,
            &raw mut info,
        )
    };
    if created == 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: CreateProcessW just opened these two handles for this process; nothing else owns them.
    let (process, thread) = unsafe { (OwnedHandle::from_raw_handle(info.hProcess), OwnedHandle::from_raw_handle(info.hThread)) };
    drop(thread);
    // The tool holds its own ends now: this process's copies go, so its output ends with it.
    drop(ends);
    Ok(Created { pid: info.dwProcessId, process, stdout: stdout.into(), stderr: stderr.into() })
}

/// Waits `ms` milliseconds at most for `process` to exit: its exit code, or none yet.
pub fn exit_code(process: &OwnedHandle, ms: u32) -> std::io::Result<Option<u32>> {
    // SAFETY: the process handle is open for the call.
    let waited = unsafe { WaitForSingleObject(process.as_raw_handle(), ms) };
    if waited == WAIT_TIMEOUT {
        return Ok(None);
    }
    if waited != WAIT_OBJECT_0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut code = 0u32;
    // SAFETY: the process handle is open; `code` is writable.
    if unsafe { GetExitCodeProcess(process.as_raw_handle(), &raw mut code) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(Some(code))
}

/// Ends the process `process` is a handle to.
pub fn terminate_process(process: &OwnedHandle) {
    use windows_sys::Win32::System::Threading::TerminateProcess;
    // SAFETY: the process handle is open for the call.
    unsafe {
        TerminateProcess(process.as_raw_handle(), 1);
    }
}

/// `s` as a NUL-terminated UTF-16 string.
fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

/// The environment block of `vars`: `name=value` strings, each NUL-terminated, then a NUL.
fn block(vars: &[(OsString, OsString)]) -> Vec<u16> {
    let mut out = Vec::new();
    for (name, value) in vars {
        out.extend(name.encode_wide());
        out.push(u16::from(b'='));
        out.extend(value.encode_wide());
        out.push(0);
    }
    if vars.is_empty() {
        out.push(0);
    }
    out.push(0);
    out
}

/// Lets a process created with handle inheritance inherit `h`.
fn inheritable(h: &OwnedHandle) -> std::io::Result<()> {
    // SAFETY: `h` is an open handle this process owns; only its inherit flag changes.
    if unsafe { SetHandleInformation(h.as_raw_handle(), HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// A process attribute list (`STARTUPINFOEXW`'s), deleted when dropped.
struct Attributes {
    /// Its buffer, of the size Windows asked for (`usize`s, for the alignment it needs).
    buffer: Vec<usize>,
}

impl Attributes {
    /// A list with room for `count` attributes.
    fn new(count: u32) -> std::io::Result<Attributes> {
        let mut size = 0usize;
        // SAFETY: the sizing call: no list, only `size` written (it "fails" with the size needed).
        unsafe {
            InitializeProcThreadAttributeList(std::ptr::null_mut(), count, 0, &raw mut size);
        }
        let mut buffer = vec![0usize; size.div_ceil(std::mem::size_of::<usize>())];
        // SAFETY: `buffer` is at least `size` bytes, aligned for a pointer, and lives as long as
        // the list (it is the list's).
        if unsafe { InitializeProcThreadAttributeList(buffer.as_mut_ptr().cast(), count, 0, &raw mut size) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Attributes { buffer })
    }

    fn list(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.buffer.as_mut_ptr().cast()
    }

    /// Sets `attribute` to the handles `value`, which must outlive the list's use.
    fn set(&mut self, attribute: u32, value: &[HANDLE]) -> std::io::Result<()> {
        let list = self.list();
        let attribute = usize::try_from(attribute).map_err(std::io::Error::other)?;
        // SAFETY: `list` was initialized by `new`; `value` points to `value.len()` handles, which
        // the caller keeps alive until the process is created.
        let set = unsafe {
            UpdateProcThreadAttribute(list, 0, attribute, value.as_ptr().cast(), std::mem::size_of_val(value), std::ptr::null_mut(), std::ptr::null())
        };
        if set == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
}

impl Drop for Attributes {
    fn drop(&mut self) {
        let list = self.list();
        // SAFETY: `list` was initialized by `new` and is deleted once, its buffer still alive.
        unsafe {
            DeleteProcThreadAttributeList(list);
        }
    }
}
