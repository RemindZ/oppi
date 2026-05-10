#![cfg(windows)]

//! Windows restricted process launcher for the sandbox adapter.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString, c_void};
use std::fs::File;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::AsRawHandle;
use std::os::windows::io::FromRawHandle;
use std::os::windows::process::ExitStatusExt;
use std::process::ExitStatus;
use std::ptr::{null, null_mut};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use oppi_protocol::{ManagedNetworkConfig, NetworkPolicy, SandboxExecPlan};
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, HANDLE, HANDLE_FLAG_INHERIT, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOB_OBJECT_UILIMIT_DESKTOP, JOB_OBJECT_UILIMIT_DISPLAYSETTINGS, JOB_OBJECT_UILIMIT_EXITWINDOWS,
    JOB_OBJECT_UILIMIT_GLOBALATOMS, JOB_OBJECT_UILIMIT_HANDLES, JOB_OBJECT_UILIMIT_READCLIPBOARD,
    JOB_OBJECT_UILIMIT_SYSTEMPARAMETERS, JOB_OBJECT_UILIMIT_WRITECLIPBOARD,
    JOBOBJECT_BASIC_UI_RESTRICTIONS, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectBasicUIRestrictions, JobObjectExtendedLimitInformation, SetInformationJobObject,
    TerminateJobObject,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::StationsAndDesktops::{
    CloseDesktop, CreateDesktopW, DESKTOP_CREATEWINDOW, DESKTOP_ENUMERATE, DESKTOP_READOBJECTS,
    DESKTOP_WRITEOBJECTS, HDESK,
};
use windows_sys::Win32::System::Threading::{
    CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateProcessAsUserW, GetCurrentProcessId,
    GetExitCodeProcess, PROCESS_INFORMATION, STARTF_USESTDHANDLES, STARTUPINFOW,
    WaitForSingleObject,
};

use super::{
    CapturedOutput, create_restricted_low_integrity_primary_token,
    create_restricted_low_integrity_primary_token_for_credentials, read_bounded,
};

pub(super) fn run_windows_restricted_plan(
    plan: &SandboxExecPlan,
    timeout_ms: u64,
    max_output_bytes: usize,
) -> Result<CapturedOutput, String> {
    let credentials = dedicated_sandbox_credentials_from_env();
    ensure_windows_network_enforcement(plan, credentials.as_ref())?;
    let token = if let Some(credentials) = credentials.as_ref() {
        create_restricted_low_integrity_primary_token_for_credentials(
            &credentials.username,
            credentials.domain.as_deref(),
            &credentials.password,
        )?
    } else {
        create_restricted_low_integrity_primary_token()?
    };
    let mut stdout_pipe = InheritablePipe::new().map_err(|error| error.to_string())?;
    let mut stderr_pipe = InheritablePipe::new().map_err(|error| error.to_string())?;
    let mut desktop = PrivateDesktop::create()?;
    let job = JobGuard::create()?;

    let mut command_line = to_wide_mut(&format!("cmd.exe /D /C {}", plan.command));
    let cwd = to_wide(OsStr::new(&plan.cwd));
    let mut env = environment_block_for_plan(plan);
    let mut desktop_name = desktop.startup_desktop_name();

    let mut startup: STARTUPINFOW = unsafe { std::mem::zeroed() };
    startup.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
    startup.dwFlags = STARTF_USESTDHANDLES;
    startup.hStdOutput = stdout_pipe.write;
    startup.hStdError = stderr_pipe.write;
    startup.hStdInput = null_mut();
    startup.lpDesktop = desktop_name.as_mut_ptr();

    let mut process_info: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    let created = unsafe {
        CreateProcessAsUserW(
            token.as_handle(),
            null(),
            command_line.as_mut_ptr(),
            null(),
            null(),
            1,
            CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT,
            env.as_mut_ptr() as *const c_void,
            cwd.as_ptr(),
            &startup,
            &mut process_info,
        )
    };
    if created == 0 {
        return Err(format_last_error("CreateProcessAsUserW"));
    }

    let process = ProcessGuard::new(process_info.hProcess, process_info.hThread);
    job.assign(process.process)?;
    stdout_pipe.close_write();
    stderr_pipe.close_write();

    let mut stdout = stdout_pipe.take_read_file()?;
    let mut stderr = stderr_pipe.take_read_file()?;
    let stdout_reader = thread::spawn(move || read_bounded(&mut stdout, max_output_bytes));
    let stderr_reader = thread::spawn(move || read_bounded(&mut stderr, max_output_bytes));

    unsafe {
        windows_sys::Win32::System::Threading::ResumeThread(process.thread);
    }

    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let mut timed_out = false;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let wait_ms = remaining.as_millis().min(50) as u32;
        let wait = unsafe { WaitForSingleObject(process.process, wait_ms) };
        if wait == WAIT_OBJECT_0 {
            break;
        }
        if wait != WAIT_TIMEOUT {
            return Err(format_last_error("WaitForSingleObject"));
        }
        if Instant::now() >= deadline {
            timed_out = true;
            job.terminate(1);
            unsafe {
                WaitForSingleObject(process.process, 5_000);
            }
            break;
        }
    }

    let mut exit_code = 0_u32;
    let exit_ok = unsafe { GetExitCodeProcess(process.process, &mut exit_code) };
    if exit_ok == 0 {
        return Err(format_last_error("GetExitCodeProcess"));
    }

    let stdout = stdout_reader
        .join()
        .map_err(|_| "stdout reader panicked".to_string())?
        .map_err(|error| error.to_string())?;
    let stderr = stderr_reader
        .join()
        .map_err(|_| "stderr reader panicked".to_string())?
        .map_err(|error| error.to_string())?;

    // Keep the private desktop alive until after the process and pipe readers are done.
    desktop.close();

    Ok(CapturedOutput {
        exit_code: Some(exit_code as i32),
        stdout,
        stderr,
        timed_out,
    })
}

#[derive(Debug)]
pub(super) struct WindowsRestrictedBackgroundProcess {
    process: ProcessGuard,
    job: JobGuard,
    _desktop: PrivateDesktop,
}

// The process, job, and desktop handles are owned by this wrapper and are only
// closed from its methods/drop path, so moving the wrapper between worker
// threads preserves single ownership.
unsafe impl Send for WindowsRestrictedBackgroundProcess {}

impl WindowsRestrictedBackgroundProcess {
    pub(super) fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.process.try_wait()
    }

    pub(super) fn wait(&mut self) -> io::Result<ExitStatus> {
        self.process.wait()
    }

    pub(super) fn kill(&mut self) -> io::Result<()> {
        self.job.terminate(1);
        let _ = self.process.wait();
        Ok(())
    }
}

pub(super) fn spawn_windows_restricted_background_plan(
    plan: &SandboxExecPlan,
    stdout: File,
    stderr: File,
) -> Result<WindowsRestrictedBackgroundProcess, String> {
    let credentials = dedicated_sandbox_credentials_from_env();
    ensure_windows_network_enforcement(plan, credentials.as_ref())?;
    let token = if let Some(credentials) = credentials.as_ref() {
        create_restricted_low_integrity_primary_token_for_credentials(
            &credentials.username,
            credentials.domain.as_deref(),
            &credentials.password,
        )?
    } else {
        create_restricted_low_integrity_primary_token()?
    };
    let desktop = PrivateDesktop::create()?;
    let job = JobGuard::create()?;

    let mut command_line = to_wide_mut(&format!("cmd.exe /D /C {}", plan.command));
    let cwd = to_wide(OsStr::new(&plan.cwd));
    let mut env = environment_block_for_plan(plan);
    let mut desktop_name = desktop.startup_desktop_name();
    let stdout_handle = stdout.as_raw_handle() as HANDLE;
    let stderr_handle = stderr.as_raw_handle() as HANDLE;
    set_handle_inherit(stdout_handle, true)?;
    if stderr_handle != stdout_handle {
        set_handle_inherit(stderr_handle, true)?;
    }

    let create_result = (|| {
        let mut startup: STARTUPINFOW = unsafe { std::mem::zeroed() };
        startup.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        startup.dwFlags = STARTF_USESTDHANDLES;
        startup.hStdOutput = stdout_handle;
        startup.hStdError = stderr_handle;
        startup.hStdInput = null_mut();
        startup.lpDesktop = desktop_name.as_mut_ptr();

        let mut process_info: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
        let created = unsafe {
            CreateProcessAsUserW(
                token.as_handle(),
                null(),
                command_line.as_mut_ptr(),
                null(),
                null(),
                1,
                CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT,
                env.as_mut_ptr() as *const c_void,
                cwd.as_ptr(),
                &startup,
                &mut process_info,
            )
        };
        if created == 0 {
            return Err(format_last_error("CreateProcessAsUserW"));
        }

        let process = ProcessGuard::new(process_info.hProcess, process_info.hThread);
        job.assign(process.process)?;
        unsafe {
            windows_sys::Win32::System::Threading::ResumeThread(process.thread);
        }
        Ok(process)
    })();

    let clear_stdout = set_handle_inherit(stdout_handle, false);
    let clear_stderr = if stderr_handle != stdout_handle {
        set_handle_inherit(stderr_handle, false)
    } else {
        Ok(())
    };
    clear_stdout?;
    clear_stderr?;

    let process = create_result?;
    Ok(WindowsRestrictedBackgroundProcess {
        process,
        job,
        _desktop: desktop,
    })
}

struct InheritablePipe {
    read: HANDLE,
    write: HANDLE,
}

impl InheritablePipe {
    fn new() -> io::Result<Self> {
        let attrs = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: null_mut(),
            bInheritHandle: 1,
        };
        let mut read = null_mut();
        let mut write = null_mut();
        let created = unsafe { CreatePipe(&mut read, &mut write, &attrs, 0) };
        if created == 0 {
            return Err(io::Error::last_os_error());
        }
        let inherit_cleared = unsafe {
            windows_sys::Win32::Foundation::SetHandleInformation(read, HANDLE_FLAG_INHERIT, 0)
        };
        if inherit_cleared == 0 {
            unsafe {
                CloseHandle(read);
                CloseHandle(write);
            }
            return Err(io::Error::last_os_error());
        }
        Ok(Self { read, write })
    }

    fn close_write(&mut self) {
        if !self.write.is_null() {
            unsafe {
                CloseHandle(self.write);
            }
            self.write = null_mut();
        }
    }

    fn take_read_file(&mut self) -> Result<File, String> {
        if self.read.is_null() {
            return Err("pipe read handle was already taken".to_string());
        }
        let read = self.read;
        self.read = null_mut();
        Ok(unsafe { File::from_raw_handle(read as _) })
    }
}

impl Drop for InheritablePipe {
    fn drop(&mut self) {
        if !self.read.is_null() {
            unsafe {
                CloseHandle(self.read);
            }
        }
        if !self.write.is_null() {
            unsafe {
                CloseHandle(self.write);
            }
        }
    }
}

#[derive(Debug)]
struct ProcessGuard {
    process: HANDLE,
    thread: HANDLE,
}

impl ProcessGuard {
    fn new(process: HANDLE, thread: HANDLE) -> Self {
        Self { process, thread }
    }

    fn try_wait(&self) -> io::Result<Option<ExitStatus>> {
        let mut exit_code = 0_u32;
        let ok = unsafe { GetExitCodeProcess(self.process, &mut exit_code) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        if exit_code == 259 {
            Ok(None)
        } else {
            Ok(Some(ExitStatus::from_raw(exit_code)))
        }
    }

    fn wait(&self) -> io::Result<ExitStatus> {
        let wait = unsafe { WaitForSingleObject(self.process, u32::MAX) };
        if wait != WAIT_OBJECT_0 {
            return Err(io::Error::last_os_error());
        }
        self.try_wait()?
            .ok_or_else(|| io::Error::other("process still active after wait"))
    }
}

impl Drop for ProcessGuard {
    fn drop(&mut self) {
        unsafe {
            if !self.thread.is_null() {
                CloseHandle(self.thread);
            }
            if !self.process.is_null() {
                CloseHandle(self.process);
            }
        }
    }
}

#[derive(Debug)]
struct JobGuard {
    handle: HANDLE,
}

impl JobGuard {
    fn create() -> Result<Self, String> {
        unsafe {
            let job = CreateJobObjectW(null(), null());
            if job.is_null() {
                return Err(format_last_error("CreateJobObjectW"));
            }
            let guard = Self { handle: job };
            guard.configure()?;
            Ok(guard)
        }
    }

    fn configure(&self) -> Result<(), String> {
        unsafe {
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                self.handle,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) == 0
            {
                return Err(format_last_error("SetInformationJobObject"));
            }
            let ui_limits = JOBOBJECT_BASIC_UI_RESTRICTIONS {
                UIRestrictionsClass: JOB_OBJECT_UILIMIT_DESKTOP
                    | JOB_OBJECT_UILIMIT_DISPLAYSETTINGS
                    | JOB_OBJECT_UILIMIT_EXITWINDOWS
                    | JOB_OBJECT_UILIMIT_GLOBALATOMS
                    | JOB_OBJECT_UILIMIT_HANDLES
                    | JOB_OBJECT_UILIMIT_READCLIPBOARD
                    | JOB_OBJECT_UILIMIT_SYSTEMPARAMETERS
                    | JOB_OBJECT_UILIMIT_WRITECLIPBOARD,
            };
            if SetInformationJobObject(
                self.handle,
                JobObjectBasicUIRestrictions,
                &ui_limits as *const _ as *const _,
                std::mem::size_of::<JOBOBJECT_BASIC_UI_RESTRICTIONS>() as u32,
            ) == 0
            {
                return Err(format_last_error("SetInformationJobObject UI restrictions"));
            }
        }
        Ok(())
    }

    fn assign(&self, process: HANDLE) -> Result<(), String> {
        let assigned = unsafe { AssignProcessToJobObject(self.handle, process) };
        if assigned == 0 {
            return Err(format_last_error("AssignProcessToJobObject"));
        }
        Ok(())
    }

    fn terminate(&self, exit_code: u32) {
        unsafe {
            TerminateJobObject(self.handle, exit_code);
        }
    }
}

impl Drop for JobGuard {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe {
                CloseHandle(self.handle);
            }
        }
    }
}

#[derive(Debug)]
struct PrivateDesktop {
    handle: HDESK,
    name: String,
}

impl PrivateDesktop {
    fn create() -> Result<Self, String> {
        let name = format!(
            "oppi-sandbox-{}-{}",
            unsafe { GetCurrentProcessId() },
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        let wide_name = to_wide(OsStr::new(&name));
        let access =
            DESKTOP_CREATEWINDOW | DESKTOP_ENUMERATE | DESKTOP_READOBJECTS | DESKTOP_WRITEOBJECTS;
        let handle =
            unsafe { CreateDesktopW(wide_name.as_ptr(), null(), null(), 0, access, null()) };
        if handle.is_null() {
            return Err(format_last_error("CreateDesktopW"));
        }
        Ok(Self { handle, name })
    }

    fn startup_desktop_name(&self) -> Vec<u16> {
        to_wide(OsStr::new(&format!("winsta0\\{}", self.name)))
    }

    fn close(&mut self) {
        if !self.handle.is_null() {
            unsafe {
                CloseDesktop(self.handle);
            }
            self.handle = null_mut();
        }
    }
}

impl Drop for PrivateDesktop {
    fn drop(&mut self) {
        self.close();
    }
}

#[derive(Debug, Clone)]
struct DedicatedSandboxCredentials {
    username: String,
    domain: Option<String>,
    password: String,
}

fn dedicated_sandbox_credentials_from_env() -> Option<DedicatedSandboxCredentials> {
    let raw_username = std::env::var("OPPI_WINDOWS_SANDBOX_USERNAME").ok()?;
    let password = std::env::var("OPPI_WINDOWS_SANDBOX_PASSWORD").ok()?;
    let explicit_domain = std::env::var("OPPI_WINDOWS_SANDBOX_DOMAIN")
        .ok()
        .filter(|domain| !domain.is_empty());
    let (domain, username) = if let Some((domain, username)) = raw_username.split_once('\\') {
        (Some(domain.to_string()), username.to_string())
    } else {
        (explicit_domain, raw_username)
    };
    Some(DedicatedSandboxCredentials {
        username,
        domain,
        password,
    })
}

fn ensure_windows_network_enforcement(
    plan: &SandboxExecPlan,
    credentials: Option<&DedicatedSandboxCredentials>,
) -> Result<(), String> {
    if plan.network != NetworkPolicy::Disabled {
        return Ok(());
    }
    if credentials.is_none() {
        return Err("Windows disabled-network sandbox execution requires OPPI_WINDOWS_SANDBOX_USERNAME/OPPI_WINDOWS_SANDBOX_PASSWORD for a dedicated sandbox account with matching WFP filters; refusing to run instead of providing review-only network isolation".to_string());
    }
    if std::env::var("OPPI_WINDOWS_SANDBOX_WFP_READY").as_deref() != Ok("1") {
        return Err("Windows disabled-network sandbox execution requires OPPI_WINDOWS_SANDBOX_WFP_READY=1 after installing OPPi WFP filters for the dedicated sandbox account; refusing to run".to_string());
    }
    Ok(())
}

fn environment_block_for_plan(plan: &SandboxExecPlan) -> Vec<u16> {
    let mut env: BTreeMap<String, OsString> = std::env::vars_os()
        .filter_map(|(key, value)| Some((key.into_string().ok()?, value)))
        .collect();
    if plan.network == NetworkPolicy::Disabled {
        for key in [
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "ALL_PROXY",
            "NO_PROXY",
            "http_proxy",
            "https_proxy",
            "all_proxy",
            "no_proxy",
        ] {
            env.remove(key);
        }
    } else if let Some(config) = &plan.managed_network {
        apply_proxy_env_to_map(&mut env, config);
    }

    let mut block = Vec::new();
    for (key, value) in env {
        block.extend(OsStr::new(&key).encode_wide());
        block.push('=' as u16);
        block.extend(value.encode_wide());
        block.push(0);
    }
    block.push(0);
    block
}

fn apply_proxy_env_to_map(env: &mut BTreeMap<String, OsString>, config: &ManagedNetworkConfig) {
    if let Some(proxy) = &config.http_proxy {
        env.insert("HTTP_PROXY".to_string(), OsString::from(proxy));
        env.insert("http_proxy".to_string(), OsString::from(proxy));
    }
    if let Some(proxy) = &config.https_proxy {
        env.insert("HTTPS_PROXY".to_string(), OsString::from(proxy));
        env.insert("https_proxy".to_string(), OsString::from(proxy));
    }
    if let Some(proxy) = &config.all_proxy {
        env.insert("ALL_PROXY".to_string(), OsString::from(proxy));
        env.insert("all_proxy".to_string(), OsString::from(proxy));
    }
    if !config.no_proxy.is_empty() || config.allow_loopback {
        let mut entries = config.no_proxy.clone();
        if config.allow_loopback {
            entries.extend([
                "localhost".to_string(),
                "127.0.0.1".to_string(),
                "::1".to_string(),
            ]);
        }
        let no_proxy = entries.join(",");
        env.insert("NO_PROXY".to_string(), OsString::from(&no_proxy));
        env.insert("no_proxy".to_string(), OsString::from(no_proxy));
    }
}

fn set_handle_inherit(handle: HANDLE, inherit: bool) -> Result<(), String> {
    let flags = if inherit { HANDLE_FLAG_INHERIT } else { 0 };
    let ok = unsafe {
        windows_sys::Win32::Foundation::SetHandleInformation(handle, HANDLE_FLAG_INHERIT, flags)
    };
    if ok == 0 {
        return Err(format_last_error("SetHandleInformation"));
    }
    Ok(())
}

fn to_wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

fn to_wide_mut(value: &str) -> Vec<u16> {
    to_wide(OsStr::new(value))
}

fn format_last_error(operation: &str) -> String {
    let error = unsafe { GetLastError() };
    format!("{operation} failed: {error}")
}
