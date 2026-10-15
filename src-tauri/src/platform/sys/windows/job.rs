//! Job objects: a tool's whole process tree, ended with it.

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_BASIC_LIMIT_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
};

/// A job object a tool's process (and every process it starts) runs in.
pub struct Job(HANDLE);

// SAFETY: a job handle is a kernel handle, usable from any thread.
unsafe impl Send for Job {}
// SAFETY: as above; every call on it is a kernel call the OS serializes.
unsafe impl Sync for Job {}

impl Job {
    /// A job ending its processes when closed, with `process` in it. `None` when Windows
    /// refuses (the tool then runs as before, its tree not tracked).
    pub fn for_process(process: std::os::windows::io::RawHandle) -> Option<Job> {
        // SAFETY: plain Win32 calls on handles owned here (`process` is the live child's).
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return None;
            }
            let job = Job(job);
            if !job.set_kill_on_close(true) || AssignProcessToJobObject(job.0, process) == 0 {
                return None;
            }
            Some(job)
        }
    }

    fn set_kill_on_close(&self, on: bool) -> bool {
        let info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
            BasicLimitInformation: JOBOBJECT_BASIC_LIMIT_INFORMATION {
                LimitFlags: if on { JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE } else { 0 },
                ..JOBOBJECT_BASIC_LIMIT_INFORMATION::default()
            },
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

    /// The tool is done: closing the job no longer ends what it left running.
    pub fn release(&self) {
        self.set_kill_on_close(false);
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

/// Resumes every thread of process `pid`; whether one was.
pub fn resume_threads_of(pid: u32) -> bool {
    use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};
    let Ok(size) = u32::try_from(std::mem::size_of::<THREADENTRY32>()) else { return false };
    let mut resumed = false;
    // SAFETY: a thread snapshot walked with a correctly sized entry; each thread handle is
    // opened, used and closed here, and the snapshot is closed once done.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return false;
        }
        let mut entry: THREADENTRY32 = std::mem::zeroed();
        entry.dwSize = size;
        let mut more = Thread32First(snapshot, &raw mut entry) != 0;
        while more {
            if entry.th32OwnerProcessID == pid {
                let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                if !thread.is_null() {
                    resumed |= ResumeThread(thread) != u32::MAX;
                    CloseHandle(thread);
                }
            }
            more = Thread32Next(snapshot, &raw mut entry) != 0;
        }
        CloseHandle(snapshot);
    }
    resumed
}

/// Ends process `pid` (one that couldn't be let run).
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
