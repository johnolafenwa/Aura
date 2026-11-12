//! Process groups on Windows.
//!
//! A Unix process group is a set of processes one signal reaches: the
//! child and everything it starts. Windows has no such signal target. Its
//! nearest thing is a job object, which holds a process and its
//! descendants, can end them all at once, and counts the ones still
//! running.
//!
//! A process joins a job only after it exists. To keep a child from
//! starting a process of its own before it joins, the runtime creates the
//! child suspended, assigns it, and then resumes it.

use std::io;
use std::os::windows::io::AsRawHandle;
use std::process::Child;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicAccountingInformation,
    QueryInformationJobObject, TerminateJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
};

/// The process creation flag that leaves the new process's first thread
/// suspended.
pub const CREATE_SUSPENDED: u32 = windows_sys::Win32::System::Threading::CREATE_SUSPENDED;

#[link(name = "ntdll")]
extern "system" {
    /// Resumes every thread of a process. Windows documents no call that
    /// resumes a process by its handle alone; this one has been stable
    /// since Windows XP.
    fn NtResumeProcess(process: HANDLE) -> i32;
}

/// Resumes a child created with [`CREATE_SUSPENDED`].
pub fn resume(child: &Child) -> io::Result<()> {
    // SAFETY: the handle is the live child's.
    let status = unsafe { NtResumeProcess(child.as_raw_handle() as HANDLE) };
    if status < 0 {
        return Err(io::Error::other(format!(
            "could not resume the child process (status {status:#x})"
        )));
    }
    Ok(())
}

/// A job object that holds a child and its descendants.
pub struct JobGroup {
    job: HANDLE,
}

// SAFETY: a job handle may be used from any thread, and the kernel
// serializes the calls made on it.
unsafe impl Send for JobGroup {}
// SAFETY: as above.
unsafe impl Sync for JobGroup {}

impl JobGroup {
    /// Creates an empty job.
    pub fn create() -> io::Result<Self> {
        // SAFETY: both arguments may be null, for default security and no
        // name.
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if job.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { job })
    }

    /// Puts `child` in the job. Processes it starts afterwards join too.
    pub fn assign(&self, child: &Child) -> io::Result<()> {
        // SAFETY: both handles are live.
        if unsafe { AssignProcessToJobObject(self.job, child.as_raw_handle() as HANDLE) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Ends every process in the job.
    pub fn terminate(&self) -> io::Result<()> {
        // SAFETY: the job handle is live.
        if unsafe { TerminateJobObject(self.job, 1) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Whether any process in the job is still running.
    pub fn alive(&self) -> bool {
        let mut information: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION =
            // SAFETY: the structure is plain integers, and zero is valid.
            unsafe { std::mem::zeroed() };
        // SAFETY: the pointer and size name `information`, and the job
        // handle is live.
        let queried = unsafe {
            QueryInformationJobObject(
                self.job,
                JobObjectBasicAccountingInformation,
                (&raw mut information).cast(),
                std::mem::size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                std::ptr::null_mut(),
            )
        };
        queried != 0 && information.ActiveProcesses > 0
    }
}

impl Drop for JobGroup {
    fn drop(&mut self) {
        // SAFETY: the handle is live and closed once.
        unsafe { CloseHandle(self.job) };
    }
}
